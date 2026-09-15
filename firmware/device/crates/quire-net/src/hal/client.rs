//! The HTTP(S) client the fetchers share: one request at a time over the session's
//! stack, DNS through the stack, TLS through [`super::tls`], the response framed by
//! reqwless (status line, headers, content-length and chunked bodies). Redirects are
//! followed (five at most, never from https down to http), `Range` requests resume
//! downloads, and bodies stream to a sink in pieces of at most 4 KB, so a book never
//! sits in RAM. Every buffer is allocated for the request and freed with it.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::{IpAddress, IpEndpoint, Stack};
use embassy_time::{with_timeout, Duration};
use embedded_io_async::{ErrorKind, ErrorType, Read, Write};
use reqwless::response::Response;

use super::tls;
use crate::proto::url::Url;

/// TCP receive buffer (the download window).
const TCP_RX: usize = 4 * 1024;
/// TCP send buffer.
const TCP_TX: usize = 1536;
/// Response header buffer.
const HEADER_BUF: usize = 3 * 1024;
/// Body pieces handed to the sink.
const CHUNK: usize = 4 * 1024;
/// Redirects followed at most.
const MAX_REDIRECTS: usize = 5;
/// Idle limit on the socket (a stalled download ends with an error, not a hang).
const IO_TIMEOUT: Duration = Duration::from_secs(25);
const DNS_TIMEOUT: Duration = Duration::from_secs(8);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HEADERS_TIMEOUT: Duration = Duration::from_secs(30);

/// Request methods.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// GET.
    Get,
    /// HEAD.
    Head,
    /// PUT.
    Put,
    /// POST.
    Post,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Put => "PUT",
            Method::Post => "POST",
        }
    }
    fn reqwless(self) -> reqwless::request::Method {
        match self {
            Method::Get => reqwless::request::Method::GET,
            Method::Head => reqwless::request::Method::HEAD,
            Method::Put => reqwless::request::Method::PUT,
            Method::Post => reqwless::request::Method::POST,
        }
    }
}

/// A request.
pub struct Request<'a> {
    /// Method.
    pub method: Method,
    /// Absolute URL.
    pub url: &'a str,
    /// Extra headers.
    pub headers: &'a [(&'a str, &'a str)],
    /// Body (with its `Content-Type` among the headers).
    pub body: Option<&'a [u8]>,
    /// Ask for the body from this offset (`Range`); the reply's [`Head::range_start`]
    /// says whether the server honoured it.
    pub range_from: Option<u64>,
}

impl<'a> Request<'a> {
    /// A GET.
    pub fn get(url: &'a str) -> Self {
        Request { method: Method::Get, url, headers: &[], body: None, range_from: None }
    }
}

/// What a response said before its body.
#[derive(Clone, Debug, Default)]
pub struct Head {
    /// Status code.
    pub status: u16,
    /// `Content-Length` (of the range, for a 206).
    pub content_length: Option<u64>,
    /// The first byte offset of a `206 Partial Content` reply.
    pub range_start: Option<u64>,
    /// `Content-Type`, lower-cased.
    pub content_type: String,
    /// The file name from `Content-Disposition`, if any.
    pub file_name: Option<String>,
    /// The URL that answered (after redirects).
    pub url: String,
}

/// Why a request failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The URL is not an absolute http(s) URL.
    Url,
    /// The name did not resolve.
    Dns,
    /// The connection was refused or timed out.
    Connect,
    /// The TLS handshake failed (certificate rejected, protocol mismatch).
    Tls,
    /// The server stopped answering.
    Timeout,
    /// The connection broke.
    Io,
    /// More than five redirects.
    Redirects,
    /// A non-success status.
    Status(u16),
    /// The sink refused data (the card, or a cancel).
    Sink(String),
    /// The link is not up.
    Offline,
}

impl Error {
    /// A sentence for the screens.
    pub fn text(&self, url: &str) -> String {
        let host = Url::parse(url).map(|u| u.host).unwrap_or_default();
        match self {
            Error::Url => String::from("That is not a web address."),
            Error::Dns => alloc::format!("Could not find {host}."),
            Error::Connect => alloc::format!("{host} is not answering."),
            Error::Tls => alloc::format!("Secure connection to {host} failed."),
            Error::Timeout => alloc::format!("{host} stopped answering."),
            Error::Io => String::from("Connection lost."),
            Error::Redirects => String::from("Too many redirects."),
            Error::Status(404) => alloc::format!("{host} has no such page."),
            Error::Status(429) => alloc::format!("{host} is limiting requests."),
            Error::Status(401) | Error::Status(403) => alloc::format!("{host} refused (check the login)."),
            Error::Status(s) => alloc::format!("{host} answered {s}."),
            Error::Sink(s) => s.clone(),
            Error::Offline => String::from("Not connected to Wi-Fi."),
        }
    }
}

/// A body sink: `Ok(true)` to go on, `Ok(false)` to stop early (enough), `Err` to abort.
pub type Sink<'s> = &'s mut dyn FnMut(&[u8]) -> Result<bool, String>;

/// Plain or TLS (the TLS state is large, but one connection exists at a time and it
/// lives in the job's boxed future).
#[allow(clippy::large_enum_variant)]
enum Conn<'a> {
    Plain(TcpSocket<'a>),
    Tls(tls::Tls<'a>),
}

impl ErrorType for Conn<'_> {
    type Error = ErrorKind;
}

impl Read for Conn<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ErrorKind> {
        match self {
            Conn::Plain(s) => s.read(buf).await.map_err(|_| ErrorKind::ConnectionReset),
            Conn::Tls(t) => t.read(buf).await.map_err(|_| ErrorKind::Other),
        }
    }
}

impl Write for Conn<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, ErrorKind> {
        match self {
            Conn::Plain(s) => s.write(buf).await.map_err(|_| ErrorKind::ConnectionReset),
            Conn::Tls(t) => t.write(buf).await.map_err(|_| ErrorKind::Other),
        }
    }
    async fn flush(&mut self) -> Result<(), ErrorKind> {
        match self {
            Conn::Plain(s) => s.flush().await.map_err(|_| ErrorKind::ConnectionReset),
            Conn::Tls(t) => t.flush().await.map_err(|_| ErrorKind::Other),
        }
    }
}

/// One hop's outcome.
enum Hop {
    Done(Head),
    Redirect(u16, String),
}

/// Perform `req`, calling `on_head` once the headers of a success reply are in (an
/// `Err` aborts) and `sink` with each piece of the body.
pub async fn fetch(
    stack: Stack<'_>,
    req: &Request<'_>,
    on_head: &mut dyn FnMut(&Head) -> Result<(), String>,
    sink: Sink<'_>,
) -> Result<Head, Error> {
    if !stack.is_link_up() || stack.config_v4().is_none() {
        return Err(Error::Offline);
    }
    let mut url = Url::parse(req.url).ok_or(Error::Url)?;
    let mut method = req.method;
    let mut body = req.body;
    for _ in 0..=MAX_REDIRECTS {
        crate::touch(super::now_ms());
        match hop(stack, &url, method, req.headers, body, req.range_from, on_head, sink).await? {
            Hop::Done(h) => return Ok(h),
            Hop::Redirect(status, location) => {
                log::info!("http: {status} -> {location}");
                let next = url.resolve(&location).ok_or(Error::Url)?;
                // Never step down from HTTPS: for updates the channel is the integrity check.
                if url.https && !next.https {
                    log::warn!("http: refusing redirect to plain http");
                    return Err(Error::Url);
                }
                url = next;
                if status == 303 || (matches!(status, 301 | 302) && method == Method::Post) {
                    method = Method::Get;
                    body = None;
                }
            }
        }
    }
    Err(Error::Redirects)
}

/// Fetch a small document into memory (at most `max` bytes; longer bodies are cut).
pub async fn fetch_to_vec(stack: Stack<'_>, req: &Request<'_>, max: usize) -> Result<(Head, Vec<u8>), Error> {
    let mut out = Vec::new();
    let mut sink = |chunk: &[u8]| {
        let room = max.saturating_sub(out.len());
        out.extend_from_slice(&chunk[..chunk.len().min(room)]);
        Ok(out.len() < max)
    };
    let head = fetch(stack, req, &mut |_| Ok(()), &mut sink).await?;
    Ok((head, out))
}

async fn resolve(stack: Stack<'_>, host: &str) -> Result<IpAddress, Error> {
    if let Ok(ip) = host.parse::<core::net::Ipv4Addr>() {
        return Ok(IpAddress::Ipv4(ip));
    }
    match with_timeout(DNS_TIMEOUT, stack.dns_query(host, DnsQueryType::A)).await {
        Ok(Ok(addrs)) => addrs.first().copied().ok_or(Error::Dns),
        Ok(Err(e)) => {
            log::warn!("dns {host}: {e:?}");
            Err(Error::Dns)
        }
        Err(_) => Err(Error::Timeout),
    }
}

fn header_value(name: &[u8], value: &[u8], want: &str) -> Option<String> {
    name.eq_ignore_ascii_case(want.as_bytes()).then(|| String::from(core::str::from_utf8(value).unwrap_or("").trim()))
}

/// `filename="x.epub"` or `filename*=UTF-8''x%20y.epub` out of a Content-Disposition.
fn disposition_name(v: &str) -> Option<String> {
    for part in v.split(';').map(str::trim) {
        if let Some(rest) = part.strip_prefix("filename*=") {
            let name = rest.rsplit("''").next().unwrap_or(rest);
            return Some(quire_fs::percent_decode(name.trim_matches('"')));
        }
    }
    for part in v.split(';').map(str::trim) {
        if let Some(rest) = part.strip_prefix("filename=") {
            return Some(String::from(rest.trim_matches('"')));
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
async fn hop(
    stack: Stack<'_>,
    url: &Url,
    method: Method,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    range_from: Option<u64>,
    on_head: &mut dyn FnMut(&Head) -> Result<(), String>,
    sink: Sink<'_>,
) -> Result<Hop, Error> {
    let ip = resolve(stack, &url.host).await?;
    let mut rx: Box<[u8]> = alloc::vec![0u8; TCP_RX].into_boxed_slice();
    let mut tx: Box<[u8]> = alloc::vec![0u8; TCP_TX].into_boxed_slice();
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
    socket.set_timeout(Some(IO_TIMEOUT));
    match with_timeout(CONNECT_TIMEOUT, socket.connect(IpEndpoint::new(ip, url.port))).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            log::warn!("connect {}: {e:?}", url.host);
            return Err(Error::Connect);
        }
        Err(_) => return Err(Error::Connect),
    }
    // TLS buffers live only while this hop does.
    let mut tls_rx: Box<[u8]> = if url.https { alloc::vec![0u8; tls::READ_BUF].into_boxed_slice() } else { Box::new([]) };
    let mut tls_tx: Box<[u8]> = if url.https { alloc::vec![0u8; tls::WRITE_BUF].into_boxed_slice() } else { Box::new([]) };
    let mut conn = if url.https {
        match tls::open(socket, &url.host, &mut tls_rx, &mut tls_tx).await {
            Ok(t) => Conn::Tls(t),
            Err(e) => {
                log::warn!("tls {}: {e:?}", url.host);
                return Err(Error::Tls);
            }
        }
    } else {
        Conn::Plain(socket)
    };
    log::debug!("http: {} {} heap {}", method.as_str(), url.to_text(), esp_alloc::HEAP.free());

    // The request head.
    let mut head = String::with_capacity(256);
    let _ = write!(head, "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: quire-x3/1 (+https://github.com/arrowassassin/quire)\r\nConnection: close\r\nAccept-Encoding: identity\r\n", method.as_str(), url.path, url.host_header());
    if let Some(from) = range_from {
        let _ = write!(head, "Range: bytes={from}-\r\n");
    }
    if let Some(b) = body {
        let _ = write!(head, "Content-Length: {}\r\n", b.len());
    }
    for (k, v) in headers {
        if k.contains(['\r', '\n']) || v.contains(['\r', '\n']) {
            return Err(Error::Url);
        }
        let _ = write!(head, "{k}: {v}\r\n");
    }
    head.push_str("\r\n");
    let io = |_: ErrorKind| Error::Io;
    conn.write_all(head.as_bytes()).await.map_err(io)?;
    drop(head);
    if let Some(b) = body {
        conn.write_all(b).await.map_err(io)?;
    }
    conn.flush().await.map_err(io)?;

    // The response head.
    let mut header_buf: Box<[u8]> = alloc::vec![0u8; HEADER_BUF].into_boxed_slice();
    let response = match with_timeout(HEADERS_TIMEOUT, Response::read(&mut conn, method.reqwless(), &mut header_buf)).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            log::warn!("http {}: {e:?}", url.host);
            return Err(Error::Io);
        }
        Err(_) => return Err(Error::Timeout),
    };
    let status = response.status.0;
    let mut info = Head { status, content_length: response.content_length.map(|n| n as u64), url: url.to_text(), ..Default::default() };
    let mut location = None;
    for h in response.headers() {
        let (name, value) = (h.0.as_bytes(), h.1);
        if let Some(v) = header_value(name, value, "location") {
            location = Some(v);
        } else if let Some(v) = header_value(name, value, "content-type") {
            info.content_type = v.to_ascii_lowercase();
        } else if let Some(v) = header_value(name, value, "content-disposition") {
            info.file_name = disposition_name(&v);
        } else if let Some(v) = header_value(name, value, "content-range") {
            // bytes start-end/total
            let start = v.trim_start_matches("bytes").trim().split('-').next().and_then(|s| s.trim().parse::<u64>().ok());
            info.range_start = start;
        }
    }
    log::debug!("http: {} {}", info.status, url.host);
    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        if let Some(l) = location {
            return Ok(Hop::Redirect(status, l));
        }
    }
    if !(200..300).contains(&status) {
        return Err(Error::Status(status));
    }
    if status != 206 {
        info.range_start = None;
    }
    on_head(&info).map_err(Error::Sink)?;
    if method == Method::Head {
        return Ok(Hop::Done(info));
    }

    // The body, streamed.
    let mut reader = response.body().reader();
    let mut chunk: Box<[u8]> = alloc::vec![0u8; CHUNK].into_boxed_slice();
    let mut received = 0u64;
    loop {
        let n = match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                log::warn!("http body {}: {e:?}", url.host);
                // A close-delimited body that ends is fine; a known length cut short is not.
                if info.content_length.is_some_and(|len| received < len) {
                    return Err(Error::Io);
                }
                break;
            }
        };
        received += n as u64;
        match sink(&chunk[..n]) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => return Err(Error::Sink(e)),
        }
        if let Some(len) = info.content_length {
            if received >= len {
                break;
            }
        }
    }
    Ok(Hop::Done(info))
}

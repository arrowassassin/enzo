//! The Calibre wireless-device server: answers Calibre's UDP discovery, then speaks the
//! smart-device protocol (`proto::calibre`) on the port from settings — handshake, the
//! library as Calibre's device book list, books streamed to `/Books`, deletions. No
//! password (Calibre's connection password is not supported). Runs while the Calibre
//! screen is open and a station session is up.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_futures::join::join;
use embassy_futures::select::select;
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Duration;
use embedded_io_async::Write;
use quire_library::Library;
use quire_ui::net::{DownloadState, NetEvent};
use quire_ui::{Event, Settings};

use super::now_ms;
use crate::proto::calibre::{self as proto, op};
use crate::{post, try_post, with, CardFs, DynFs, NetToMain};

static ON: Signal<CriticalSectionRawMutex, bool> = Signal::new();
static WANTED: AtomicBool = AtomicBool::new(false);

/// Socket receive buffer (books arrive through it).
const TCP_RX: usize = 4 * 1024;
const TCP_TX: usize = 2 * 1024;
/// Idle limit per read while a connection is open.
const IO_TIMEOUT: Duration = Duration::from_secs(90);

/// Start or stop the server (the Calibre screen opening and closing).
pub fn set(on: bool) {
    WANTED.store(on, Ordering::Relaxed);
    ON.signal(on);
}

fn status(text: &str) {
    with(|i| i.calibre_status = String::from(text));
    try_post(NetToMain::Ui(Event::Net(NetEvent::Calibre(String::from(text)))));
}

/// Run for the session: serves whenever the screen wants it.
pub async fn run(stack: Stack<'_>, fs: &'static dyn CardFs, hostname: &str) {
    loop {
        while !WANTED.load(Ordering::Relaxed) {
            ON.wait().await;
        }
        let port = match Settings::load(&DynFs(fs)).calibre_port {
            0 => 9090,
            p => p,
        };
        let ip = stack.config_v4().map(|c| alloc::format!("{}", c.address.address())).unwrap_or_default();
        status(&alloc::format!("Waiting for Calibre… {ip}:{port}"));
        let off = async {
            loop {
                if !ON.wait().await {
                    return;
                }
            }
        };
        select(join(discovery(stack, hostname, port), serve(stack, fs, hostname, port)), off).await;
        status("");
    }
}

/// Answer "hello" broadcasts with where the server is.
async fn discovery(stack: Stack<'_>, hostname: &str, port: u16) {
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx_buf: Box<[u8]> = alloc::vec![0u8; 512].into_boxed_slice();
    let mut tx_buf: Box<[u8]> = alloc::vec![0u8; 256].into_boxed_slice();
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if socket.bind(proto::DISCOVERY_PORTS[0]).is_err() {
        log::warn!("calibre: discovery bind failed");
        core::future::pending::<()>().await;
    }
    let reply = proto::discovery_reply(hostname, port);
    let mut buf = [0u8; 128];
    loop {
        let Ok((n, meta)) = socket.recv_from(&mut buf).await else { continue };
        if buf[..n].starts_with(b"hello") {
            let _ = socket.send_to(reply.as_bytes(), meta.endpoint).await;
        }
    }
}

async fn serve(stack: Stack<'_>, fs: &'static dyn CardFs, hostname: &str, port: u16) {
    let mut rx: Box<[u8]> = alloc::vec![0u8; TCP_RX].into_boxed_slice();
    let mut tx: Box<[u8]> = alloc::vec![0u8; TCP_TX].into_boxed_slice();
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
        socket.set_timeout(Some(IO_TIMEOUT));
        if socket.accept(port).await.is_err() {
            embassy_time::Timer::after(Duration::from_secs(1)).await;
            continue;
        }
        let peer = socket.remote_endpoint().map(|e| alloc::format!("{}", e.addr)).unwrap_or_default();
        status(&alloc::format!("Calibre connected from {peer}"));
        crate::touch(now_ms());
        let r = session(&mut socket, fs, hostname).await;
        log::info!("calibre: session over: {r:?}");
        socket.close();
        let _ = socket.flush().await;
        socket.abort();
        status(&alloc::format!("Waiting for Calibre… (last connection from {peer})"));
    }
}

/// A device-side book record.
struct Entry {
    lpath: String,
    title: String,
    authors: Vec<String>,
    size: u64,
}

async fn send(socket: &mut TcpSocket<'_>, opcode: u8, payload: &str) -> Result<(), ()> {
    let msg = proto::encode(opcode, payload);
    socket.write_all(&msg).await.map_err(|_| ())?;
    socket.flush().await.map_err(|_| ())
}

async fn session(socket: &mut TcpSocket<'_>, fs: &'static dyn CardFs, hostname: &str) -> Result<(), &'static str> {
    let version = with(|i| i.status.version.clone());
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk: Box<[u8]> = alloc::vec![0u8; 2048].into_boxed_slice();
    let mut entries: Vec<Entry> = Vec::new();
    loop {
        // One frame.
        let (opcode, dict) = loop {
            match proto::decode(&buf) {
                Ok(Some(f)) => {
                    let out = (f.opcode, f.dict.0.to_vec());
                    let used = f.consumed;
                    buf.drain(..used);
                    break out;
                }
                Ok(None) => {}
                Err(_) => return Err("bad frame"),
            }
            if buf.len() > proto::MAX_MESSAGE {
                return Err("frame too long");
            }
            let n = socket.read(&mut chunk).await.map_err(|_| "read")?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..n]);
            crate::touch(now_ms());
        };
        let dict = crate::proto::jsonlite::Value(&dict);
        match opcode {
            op::GET_INITIALIZATION_INFO => send(socket, op::OK, &proto::init_reply(hostname, &version)).await.map_err(|_| "write")?,
            op::GET_DEVICE_INFORMATION => {
                let uuid = alloc::format!("quire-{hostname}");
                send(socket, op::OK, &proto::device_info_reply(&uuid, hostname, &version)).await.map_err(|_| "write")?
            }
            op::SET_CALIBRE_DEVICE_INFO | op::SET_CALIBRE_DEVICE_NAME | op::SET_LIBRARY_INFO | op::NOOP => {
                send(socket, op::OK, &proto::empty()).await.map_err(|_| "write")?
            }
            op::TOTAL_SPACE => {
                let total = with(|i| i.card_total);
                send(socket, op::OK, &proto::space_reply(true, total)).await.map_err(|_| "write")?
            }
            op::FREE_SPACE => {
                let free = fs.free_bytes().unwrap_or(0);
                send(socket, op::OK, &proto::space_reply(false, free)).await.map_err(|_| "write")?
            }
            op::GET_BOOK_COUNT => {
                entries = library_entries(fs);
                send(socket, op::OK, &proto::book_count_reply(entries.len())).await.map_err(|_| "write")?;
                for (i, e) in entries.iter().enumerate() {
                    let rec = proto::book_record(i, &e.lpath, &e.title, &e.authors, e.size, "2000-01-01T00:00:00+00:00");
                    send(socket, op::OK, &rec).await.map_err(|_| "write")?;
                }
            }
            op::GET_BOOK_METADATA => {
                let i = dict.get("priKey").and_then(|k| k.as_i64()).unwrap_or(-1);
                let payload = match usize::try_from(i).ok().and_then(|i| entries.get(i).map(|e| (i, e))) {
                    Some((i, e)) => proto::book_record(i, &e.lpath, &e.title, &e.authors, e.size, "2000-01-01T00:00:00+00:00"),
                    None => proto::error("no such book"),
                };
                send(socket, op::OK, &payload).await.map_err(|_| "write")?
            }
            op::SEND_BOOK => {
                let Some((lpath, length, title, author)) = proto::send_book_fields(&dict) else {
                    send(socket, op::ERROR, &proto::error("bad SEND_BOOK")).await.map_err(|_| "write")?;
                    continue;
                };
                let wants_ok = dict.get("wantsSendOkToSendbook").and_then(|v| v.as_bool()).unwrap_or(false);
                let Some(path) = proto::card_path(&lpath) else {
                    send(socket, op::ERROR, &proto::error("bad path")).await.map_err(|_| "write")?;
                    continue;
                };
                if wants_ok {
                    send(socket, op::OK, &proto::ok_lpath(&lpath)).await.map_err(|_| "write")?;
                }
                receive_book(socket, fs, &mut buf, &mut chunk, &path, &lpath, &title, &author, length).await?;
            }
            op::DELETE_BOOK => {
                if let Some(lpath) = dict.get("lpath").and_then(|l| l.as_str()) {
                    if let Some(path) = proto::card_path(&lpath) {
                        if fs.remove(&path).is_ok() {
                            post(NetToMain::Ui(Event::BooksChanged)).await;
                        }
                    }
                }
                send(socket, op::OK, &proto::ok_uuid()).await.map_err(|_| "write")?
            }
            op::GET_BOOK_FILE_SEGMENT => send(socket, op::ERROR, &proto::error("not supported")).await.map_err(|_| "write")?,
            op::DISPLAY_MESSAGE => {
                if let Some(m) = dict.get("message").and_then(|m| m.as_str()) {
                    status(&m);
                }
            }
            op::SEND_BOOKLISTS | op::SEND_BOOK_METADATA | op::BOOK_DONE | op::CALIBRE_BUSY | op::OK | op::ERROR => {}
            other => log::info!("calibre: opcode {other} ignored"),
        }
    }
}

/// The library as Calibre sees it (real files only; lpaths are card paths without the
/// leading slash, as Calibre's device folders).
fn library_entries(fs: &dyn CardFs) -> Vec<Entry> {
    let lib = Library::load(&DynFs(fs));
    lib.books
        .iter()
        .filter(|b| !b.missing && b.path.starts_with('/'))
        .take(500)
        .map(|b| Entry {
            lpath: String::from(b.path.trim_start_matches('/')),
            title: b.title.clone(),
            authors: b.authors.clone(),
            size: b.size,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn receive_book(
    socket: &mut TcpSocket<'_>,
    fs: &'static dyn CardFs,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
    path: &str,
    lpath: &str,
    title: &str,
    author: &str,
    length: u64,
) -> Result<(), &'static str> {
    let url = alloc::format!("calibre:{lpath}");
    crate::transfer_started(title, &url, Some(length), 0);
    with(|i| {
        if let Some(d) = i.downloads.iter_mut().find(|d| d.title == title) {
            d.author = String::from(author);
        }
    });
    try_post(crate::downloads_event_msg());
    let part = alloc::format!("{path}.part");
    let parent = quire_fs::parent(path);
    if !parent.is_empty() && !fs.exists(parent) {
        let _ = fs.mkdir_all(parent);
    }
    let result: Result<(), String> = async {
        let mut w = fs.create(&part).map_err(|e| alloc::format!("card: {e:?}"))?;
        let mut got = 0u64;
        let mut last_report = now_ms();
        // Bytes that arrived with the frame first, then the socket.
        let take = (buf.len() as u64).min(length) as usize;
        if take > 0 {
            w.write_all(&buf[..take]).map_err(|e| alloc::format!("card: {e:?}"))?;
            buf.drain(..take);
            got += take as u64;
        }
        while got < length {
            let want = chunk.len().min((length - got) as usize);
            let n = socket.read(&mut chunk[..want]).await.map_err(|_| String::from("connection lost"))?;
            if n == 0 {
                return Err(String::from("connection lost"));
            }
            w.write_all(&chunk[..n]).map_err(|e| match e {
                quire_fs::FsError::Full => String::from("card full"),
                other => alloc::format!("card: {other:?}"),
            })?;
            got += n as u64;
            let now = now_ms();
            if now.wrapping_sub(last_report) >= 700 {
                last_report = now;
                crate::transfer_progress(title, got);
                try_post(crate::downloads_event_msg());
                crate::touch(now);
            }
        }
        w.flush().map_err(|e| alloc::format!("card: {e:?}"))?;
        drop(w);
        if fs.exists(path) {
            let _ = fs.remove(path);
        }
        fs.rename(&part, path).map_err(|e| alloc::format!("card: {e:?}"))
    }
    .await;
    let lost = matches!(&result, Err(e) if e == "connection lost");
    match &result {
        Ok(()) => {
            crate::transfer_finished(title, Ok(()));
            post(crate::downloads_event_msg()).await;
            post(NetToMain::Ui(Event::BooksChanged)).await;
        }
        Err(e) => {
            crate::download_state(title, DownloadState::Failed(e.clone()));
            let _ = fs.remove(&part);
            post(crate::downloads_event_msg()).await;
        }
    }
    if lost {
        Err("connection lost")
    } else {
        Ok(())
    }
}

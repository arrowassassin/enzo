//! A streaming `multipart/form-data` splitter for the `POST /upload` fallback: bytes are
//! pushed in as they arrive and parts come out as headers, data slices and ends, so a
//! file is never held in RAM.

use alloc::string::String;
use alloc::vec::Vec;

/// The boundary from a `Content-Type: multipart/form-data; boundary=...` value.
pub fn boundary_from_content_type(ct: &str) -> Option<String> {
    let (kind, params) = ct.split_once(';')?;
    if !kind.trim().eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    for p in params.split(';') {
        let (k, v) = p.trim().split_once('=')?;
        if k.trim().eq_ignore_ascii_case("boundary") {
            let v = v.trim().trim_matches('"');
            if v.is_empty() || v.len() > 70 {
                return None;
            }
            return Some(String::from(v));
        }
    }
    None
}

/// One piece of a part.
#[derive(Debug, PartialEq, Eq)]
pub enum Item<'a> {
    /// A part begins: the form field name and the file name, if any.
    Headers {
        /// `name` from `Content-Disposition`.
        name: String,
        /// `filename` from `Content-Disposition`.
        filename: Option<String>,
    },
    /// Body bytes of the current part.
    Data(&'a [u8]),
    /// The current part ended.
    End,
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    Preamble,
    Headers,
    Body,
    Done,
}

/// The splitter.
pub struct Multipart {
    delim: Vec<u8>,
    buf: Vec<u8>,
    state: State,
}

impl Multipart {
    /// A splitter for `boundary`.
    pub fn new(boundary: &str) -> Self {
        let mut delim = Vec::with_capacity(boundary.len() + 4);
        delim.extend_from_slice(b"\r\n--");
        delim.extend_from_slice(boundary.as_bytes());
        Multipart { delim, buf: Vec::new(), state: State::Preamble }
    }

    /// Whether the closing boundary was seen.
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// Feed a chunk; `sink` receives the items it completes, in order.
    pub fn feed(&mut self, chunk: &[u8], sink: &mut dyn FnMut(Item<'_>)) {
        self.buf.extend_from_slice(chunk);
        loop {
            match self.state {
                State::Done => {
                    self.buf.clear();
                    return;
                }
                State::Preamble => {
                    // The first boundary has no leading CRLF.
                    let first = &self.delim[2..];
                    match find(&self.buf, first) {
                        // Two bytes after the boundary decide between "--" and CRLF.
                        Some(i) if self.buf.len() >= i + first.len() + 2 => {
                            self.buf.drain(..i + first.len());
                            self.after_delimiter();
                        }
                        Some(_) => return,
                        None => {
                            let keep = (first.len() - 1).min(self.buf.len());
                            let cut = self.buf.len() - keep;
                            self.buf.drain(..cut);
                            return;
                        }
                    }
                }
                State::Headers => match find(&self.buf, b"\r\n\r\n") {
                    Some(i) => {
                        let (name, filename) = parse_disposition(&self.buf[..i]);
                        sink(Item::Headers { name, filename });
                        self.buf.drain(..i + 4);
                        self.state = State::Body;
                    }
                    None => return,
                },
                State::Body => match find(&self.buf, &self.delim) {
                    Some(i) if self.buf.len() >= i + self.delim.len() + 2 => {
                        if i > 0 {
                            sink(Item::Data(&self.buf[..i]));
                        }
                        sink(Item::End);
                        self.buf.drain(..i + self.delim.len());
                        self.after_delimiter();
                    }
                    Some(i) => {
                        // A delimiter, but its tail has not arrived: emit what precedes it.
                        if i > 0 {
                            sink(Item::Data(&self.buf[..i]));
                            self.buf.drain(..i);
                        }
                        return;
                    }
                    None => {
                        // Emit everything that cannot be the start of a delimiter.
                        let keep = (self.delim.len() - 1).min(self.buf.len());
                        let cut = self.buf.len() - keep;
                        if cut > 0 {
                            sink(Item::Data(&self.buf[..cut]));
                            self.buf.drain(..cut);
                        }
                        return;
                    }
                },
            }
        }
    }

    /// After a boundary (with at least two bytes buffered): `--` ends the message, CRLF
    /// starts the next part's headers.
    fn after_delimiter(&mut self) {
        if self.buf.starts_with(b"--") {
            self.state = State::Done;
            self.buf.clear();
            return;
        }
        if self.buf.starts_with(b"\r\n") {
            self.buf.drain(..2);
        }
        self.state = State::Headers;
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `name` and `filename` from a part's header block.
fn parse_disposition(headers: &[u8]) -> (String, Option<String>) {
    let text = core::str::from_utf8(headers).unwrap_or("");
    let mut name = String::new();
    let mut filename = None;
    for line in text.split("\r\n") {
        let Some((k, v)) = line.split_once(':') else { continue };
        if !k.trim().eq_ignore_ascii_case("content-disposition") {
            continue;
        }
        for p in v.split(';') {
            let p = p.trim();
            if let Some(n) = p.strip_prefix("name=") {
                name = String::from(n.trim_matches('"'));
            } else if let Some(f) = p.strip_prefix("filename*=") {
                // RFC 5987: charset'lang'percent-encoded
                if let Some((_, enc)) = f.rsplit_once('\'') {
                    filename = Some(quire_fs::percent_decode(enc));
                }
            } else if let Some(f) = p.strip_prefix("filename=") {
                if filename.is_none() {
                    let f = f.trim_matches('"');
                    // Browsers send only the base name, but be safe.
                    let base = f.rsplit(['/', '\\']).next().unwrap_or(f);
                    filename = Some(String::from(base));
                }
            }
        }
    }
    (name, filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(body: &[u8], boundary: &str, chunk: usize) -> Vec<String> {
        let mut m = Multipart::new(boundary);
        let mut out: Vec<String> = Vec::new();
        let mut data = Vec::new();
        let mut sink = |it: Item<'_>| match it {
            Item::Headers { name, filename } => out.push(alloc::format!("H {name} {filename:?}")),
            Item::Data(d) => data.extend_from_slice(d),
            Item::End => {
                out.push(alloc::format!("D {}", String::from_utf8_lossy(&data)));
                data.clear();
                out.push(String::from("E"));
            }
        };
        for c in body.chunks(chunk) {
            m.feed(c, &mut sink);
        }
        assert!(m.is_done(), "closing boundary not seen (chunk {chunk})");
        out
    }

    const BODY: &[u8] = b"--XX\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a b.epub\"\r\nContent-Type: application/epub+zip\r\n\r\nhello\r\n--world--\r\n\r\n--XX\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nT\r\n--XX--\r\n";

    #[test]
    fn splits_at_every_chunk_size() {
        let expect = ["H files Some(\"a b.epub\")", "D hello\r\n--world--\r\n", "E", "H title None", "D T", "E"];
        for chunk in 1..=BODY.len() {
            let got = run(BODY, "XX", chunk);
            assert_eq!(got, expect, "chunk size {chunk}");
        }
    }

    #[test]
    fn boundary_header() {
        assert_eq!(
            boundary_from_content_type("multipart/form-data; boundary=----WebKitFormBoundaryabc").as_deref(),
            Some("----WebKitFormBoundaryabc")
        );
        assert_eq!(boundary_from_content_type("multipart/form-data; charset=utf-8; boundary=\"q\"").as_deref(), Some("q"));
        assert_eq!(boundary_from_content_type("application/json"), None);
    }

    #[test]
    fn rfc5987_filename() {
        let (n, f) = parse_disposition(b"Content-Disposition: form-data; name=\"files\"; filename*=UTF-8''caf%C3%A9.txt");
        assert_eq!(n, "files");
        assert_eq!(f.as_deref(), Some("café.txt"));
    }
}

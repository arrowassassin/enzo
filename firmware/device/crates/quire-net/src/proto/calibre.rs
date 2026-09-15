//! The Calibre wireless-device ("smart device app") protocol, the pure part: message
//! framing (`<decimal length><JSON array [opcode, dict]>`), the opcodes, the replies a
//! device sends, the UDP discovery answer, and where a book Calibre sends lands on
//! the card. The socket loop is `hal::calibre`.

use alloc::string::String;
use alloc::vec::Vec;

use super::json::Json;
use super::jsonlite::Value;
use super::routes;

/// Opcodes Calibre sends and expects (its `driver.py` table).
pub mod op {
    /// Success reply.
    pub const OK: u8 = 0;
    /// Calibre's driver info to store.
    pub const SET_CALIBRE_DEVICE_INFO: u8 = 1;
    /// Name for the device.
    pub const SET_CALIBRE_DEVICE_NAME: u8 = 2;
    /// Device identity request.
    pub const GET_DEVICE_INFORMATION: u8 = 3;
    /// Total card space.
    pub const TOTAL_SPACE: u8 = 4;
    /// Free card space.
    pub const FREE_SPACE: u8 = 5;
    /// The device's book list.
    pub const GET_BOOK_COUNT: u8 = 6;
    /// Calibre's view of the device's books (no reply).
    pub const SEND_BOOKLISTS: u8 = 7;
    /// A book follows.
    pub const SEND_BOOK: u8 = 8;
    /// The handshake.
    pub const GET_INITIALIZATION_INFO: u8 = 9;
    /// Book finished.
    pub const BOOK_DONE: u8 = 11;
    /// Keep-alive.
    pub const NOOP: u8 = 12;
    /// Delete one book (`lpath`).
    pub const DELETE_BOOK: u8 = 13;
    /// A slice of a book file (not supported).
    pub const GET_BOOK_FILE_SEGMENT: u8 = 14;
    /// Metadata for one book.
    pub const GET_BOOK_METADATA: u8 = 15;
    /// Metadata update (no reply).
    pub const SEND_BOOK_METADATA: u8 = 16;
    /// Show a message.
    pub const DISPLAY_MESSAGE: u8 = 17;
    /// Calibre is busy.
    pub const CALIBRE_BUSY: u8 = 18;
    /// The library's identity.
    pub const SET_LIBRARY_INFO: u8 = 19;
    /// Failure reply.
    pub const ERROR: u8 = 20;
}

/// Longest message accepted.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// The ports Calibre broadcasts discovery on.
pub const DISCOVERY_PORTS: [u16; 5] = [54982, 48123, 39001, 44044, 59678];
/// Formats accepted, best first (Calibre picks the first it has or can convert to).
pub const EXTENSIONS: [&str; 6] = ["epub", "kepub", "fb2", "txt", "html", "cbz"];

/// Frame an outgoing message.
pub fn encode(opcode: u8, payload: &str) -> Vec<u8> {
    let body = alloc::format!("[{opcode}, {payload}]");
    let mut out = alloc::format!("{}", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Bytes that are not a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadFrame;

/// A decoded frame: opcode, the dict, and the bytes consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    /// Opcode.
    pub opcode: u8,
    /// The message dict.
    pub dict: Value<'a>,
    /// Bytes this frame took from the buffer.
    pub consumed: usize,
}

/// Decode the frame at the start of `buf`: `Ok(None)` when incomplete, `Err` when it
/// is not a frame at all.
pub fn decode(buf: &[u8]) -> Result<Option<Frame<'_>>, BadFrame> {
    let digits = buf.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return if buf.is_empty() { Ok(None) } else { Err(BadFrame) };
    }
    if digits > 7 {
        return Err(BadFrame);
    }
    if digits == buf.len() {
        return Ok(None);
    }
    let len: usize = core::str::from_utf8(&buf[..digits]).map_err(|_| BadFrame)?.parse().map_err(|_| BadFrame)?;
    if len > MAX_MESSAGE {
        return Err(BadFrame);
    }
    if buf.len() < digits + len {
        return Ok(None);
    }
    let body = &buf[digits..digits + len];
    let arr = Value(body);
    let mut items = arr.items();
    let opcode = items.next().and_then(|v| v.as_i64()).ok_or(BadFrame)?;
    if !(0..=255).contains(&opcode) || body.first() != Some(&b'[') {
        return Err(BadFrame);
    }
    let dict = items.next().unwrap_or(Value(b"{}"));
    Ok(Some(Frame { opcode: opcode as u8, dict, consumed: digits + len }))
}

/// The reply to `GET_INITIALIZATION_INFO`.
pub fn init_reply(device_name: &str, version: &str) -> String {
    let mut j = Json::new();
    j.obj()
        .kv_str("appName", "Quire")
        .kv_str("deviceKind", "Quire")
        .kv_str("deviceName", device_name)
        .kv_str("deviceVersion", version)
        .kv_bool("versionOK", true)
        .kv_num("maxPacketSize", 4096)
        .kv_bool("canStreamBooks", true)
        .kv_bool("canStreamMetadata", true)
        .kv_bool("canReceiveBookBinary", true)
        .kv_bool("canDeleteMultipleBooks", false)
        .kv_bool("canUseCachedMetadata", false)
        .kv_bool("canSendOkToSendbook", true)
        .kv_bool("canAcceptLibraryInfo", true)
        .kv_bool("canSupportUpdateBooks", false)
        .kv_bool("canSupportLpathChanges", true)
        .kv_bool("cacheUsesLpaths", true)
        .kv_bool("useUuidFileNames", false)
        .kv_num("coverHeight", 0)
        .kv_num("ccVersionNumber", 0)
        .kv_str("passwordHash", "")
        .key("acceptedExtensions")
        .arr();
    for e in EXTENSIONS {
        j.str(e);
    }
    j.end().key("extensionPathLengths").obj();
    for e in EXTENSIONS {
        j.kv_num(e, 37);
    }
    j.end().end();
    j.finish()
}

/// The reply to `GET_DEVICE_INFORMATION`.
pub fn device_info_reply(uuid: &str, device_name: &str, version: &str) -> String {
    let mut j = Json::new();
    j.obj().key("device_info").obj().kv_str("device_store_uuid", uuid).kv_str("device_name", device_name).end();
    j.kv_str("version", version).kv_str("device_version", version).end();
    j.finish()
}

/// The reply to `FREE_SPACE` / `TOTAL_SPACE`.
pub fn space_reply(total: bool, bytes: u64) -> String {
    let mut j = Json::new();
    j.obj().kv_u64(if total { "total_space_on_device" } else { "free_space_on_device" }, bytes).end();
    j.finish()
}

/// The reply to `GET_BOOK_COUNT`: `count` streamed book records follow.
pub fn book_count_reply(count: usize) -> String {
    let mut j = Json::new();
    j.obj().kv_num("count", count as i64).kv_bool("willStream", true).kv_bool("willScan", true).end();
    j.finish()
}

/// One book record for the listing (and the reply to `GET_BOOK_METADATA`).
pub fn book_record(index: usize, lpath: &str, title: &str, authors: &[String], size: u64, last_modified: &str) -> String {
    let mut j = Json::new();
    j.obj().kv_num("priKey", index as i64).kv_str("lpath", lpath).kv_str("uuid", "").kv_str("title", title).key("authors").arr();
    for a in authors {
        j.str(a);
    }
    j.end().kv_u64("size", size).kv_str("last_modified", last_modified).end();
    j.finish()
}

/// `{}`.
pub fn empty() -> String {
    String::from("{}")
}

/// The `OK` before a book's bytes (`SEND_BOOK` with `wantsSendOkToSendbook`).
pub fn ok_lpath(lpath: &str) -> String {
    let mut j = Json::new();
    j.obj().kv_str("lpath", lpath).end();
    j.finish()
}

/// The reply to `DELETE_BOOK`.
pub fn ok_uuid() -> String {
    let mut j = Json::new();
    j.obj().kv_str("uuid", "").end();
    j.finish()
}

/// An `ERROR` payload.
pub fn error(message: &str) -> String {
    let mut j = Json::new();
    j.obj().kv_str("message", message).end();
    j.finish()
}

/// The answer to a discovery broadcast.
pub fn discovery_reply(hostname: &str, port: u16) -> String {
    alloc::format!("calibre wireless device client (on {hostname});{port},{port}")
}

/// Where a book with Calibre's `lpath` goes on the card: its file name under `/Books`
/// (Calibre's author folders are flattened; the name is sanitized like an upload).
pub fn card_path(lpath: &str) -> Option<String> {
    let name = lpath.rsplit(['/', '\\']).next()?.trim();
    if name.is_empty() {
        return None;
    }
    let cleaned: String = name.chars().filter(|c| !c.is_control() && !"\\:*?\"<>|".contains(*c)).collect();
    routes::sanitize_path(&alloc::format!("{}/{cleaned}", routes::BOOKS_DIR))
}

/// The lpath Calibre wants, from a `SEND_BOOK` dict.
pub fn send_book_fields(dict: &Value) -> Option<(String, u64, String, String)> {
    let lpath = dict.get("lpath")?.as_str()?;
    let length = dict.get("length")?.as_i64()?.max(0) as u64;
    let meta = dict.get("metadata");
    let title = meta.and_then(|m| m.get("title")).and_then(|t| t.as_str()).unwrap_or_else(|| lpath.clone());
    let author = meta.and_then(|m| m.get("authors")).and_then(|a| a.items().next()).and_then(|a| a.as_str()).unwrap_or_default();
    Some((lpath, length, title, author))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing() {
        let msg = encode(op::OK, "{}");
        assert_eq!(msg, b"7[0, {}]");
        let f = decode(&msg).unwrap().unwrap();
        assert_eq!((f.opcode, f.dict.0, f.consumed), (0, &b"{}"[..], 8));
        assert_eq!(decode(b"7[0, {").unwrap(), None);
        assert_eq!(decode(b"12").unwrap(), None);
        assert_eq!(decode(b"").unwrap(), None);
        assert!(decode(b"x").is_err());
        assert!(decode(b"99999999[").is_err());
        let two = [encode(op::NOOP, "{}"), encode(op::SEND_BOOK, r#"{"lpath":"a.epub","length":5}"#)].concat();
        let first = decode(&two).unwrap().unwrap();
        assert_eq!(first.opcode, op::NOOP);
        let second = decode(&two[first.consumed..]).unwrap().unwrap();
        assert_eq!(second.opcode, op::SEND_BOOK);
        assert_eq!(send_book_fields(&second.dict).unwrap().1, 5);
    }

    #[test]
    fn replies_are_json() {
        for s in [
            init_reply("Quire", "0.1.0"),
            device_info_reply("u", "n", "v"),
            space_reply(false, 5),
            book_count_reply(2),
            book_record(0, "a/b.epub", "T", &[String::from("A")], 9, "2026-01-01T00:00:00+00:00"),
            ok_lpath("x"),
            ok_uuid(),
            error("e"),
        ] {
            assert!(serde_json::from_str::<serde_json::Value>(&s).is_ok(), "{s}");
        }
        let init: serde_json::Value = serde_json::from_str(&init_reply("Quire", "0.1.0")).unwrap();
        assert_eq!(init["acceptedExtensions"][0], "epub");
        assert_eq!(init["extensionPathLengths"]["epub"], 37);
        assert_eq!(discovery_reply("quire", 9090), "calibre wireless device client (on quire);9090,9090");
    }

    #[test]
    fn paths() {
        assert_eq!(card_path("Jane Austen/Pride and Prejudice (12).epub").as_deref(), Some("/Books/Pride and Prejudice (12).epub"));
        assert_eq!(card_path("a\\b:c?.epub").as_deref(), Some("/Books/bc.epub"));
        assert_eq!(card_path("dir/"), None);
        assert_eq!(card_path("../../x.epub").as_deref(), Some("/Books/x.epub"));
    }
}

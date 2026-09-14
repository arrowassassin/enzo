//! WebSocket message codec for the Drop page's Window tab: keys and typed text come in,
//! progress and status events go out. Mirror frames are binary and never JSON.

use alloc::string::String;
use quire_ui::{Key, KeyEvent, KeyKind};

use super::json::Json;

/// A message from the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Incoming {
    /// A remote key.
    Key(KeyEvent),
    /// The whole text of the phone's field (delivered while a device field is focused).
    Text(String),
    /// Subscribe to (or drop) the live mirror.
    Mirror(bool),
    /// A keep-alive.
    Ping,
}

fn key_from_name(s: &str) -> Option<Key> {
    Some(match s {
        "Left" => Key::Left,
        "Back" => Key::Back,
        "Confirm" => Key::Confirm,
        "Right" => Key::Right,
        "Up" => Key::Up,
        "Down" => Key::Down,
        "Power" => Key::Power,
        _ => return None,
    })
}

/// Parse a text frame; `None` for anything not understood.
pub fn parse_incoming(text: &str) -> Option<Incoming> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let o = v.as_object()?;
    if let Some(k) = o.get("key").and_then(|k| k.as_str()) {
        let key = key_from_name(k)?;
        let kind = match o.get("kind").and_then(|k| k.as_str()).unwrap_or("press") {
            "long" => KeyKind::Long,
            "repeat" => KeyKind::Repeat,
            _ => KeyKind::Press,
        };
        return Some(Incoming::Key(KeyEvent { key, kind }));
    }
    if let Some(t) = o.get("text").and_then(|t| t.as_str()) {
        // Keep the device side bounded: a search field never needs more.
        let t: String = t.chars().take(512).collect();
        return Some(Incoming::Text(t));
    }
    if let Some(m) = o.get("mirror").and_then(|m| m.as_bool()) {
        return Some(Incoming::Mirror(m));
    }
    if o.contains_key("ping") {
        return Some(Incoming::Ping);
    }
    None
}

/// Upload progress echo: `{"ev":"upload","name":..,"done":..,"total":..,"state":..}`.
pub fn upload_event(name: &str, done: u64, total: u64, state: &str) -> String {
    let mut j = Json::with_capacity(96 + name.len());
    j.obj().kv_str("ev", "upload").kv_str("name", name).kv_u64("done", done).kv_u64("total", total).kv_str("state", state).end();
    j.finish()
}

/// `{"ev":"books","added":N}`.
pub fn books_event(added: u32) -> String {
    let mut j = Json::new();
    j.obj().kv_str("ev", "books").kv_num("added", added).end();
    j.finish()
}

/// `{"ev":"typing","on":bool}`: the device shows a text field (or stopped).
pub fn typing_event(on: bool) -> String {
    let mut j = Json::new();
    j.obj().kv_str("ev", "typing").kv_bool("on", on).end();
    j.finish()
}

/// `{"ev":"status"}`: the page should refresh its status strip.
pub fn status_event() -> String {
    String::from(r#"{"ev":"status"}"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_and_text() {
        assert_eq!(parse_incoming(r#"{"key":"Confirm","kind":"press"}"#), Some(Incoming::Key(KeyEvent::press(Key::Confirm))));
        assert_eq!(parse_incoming(r#"{"key":"Power","kind":"long"}"#), Some(Incoming::Key(KeyEvent::long(Key::Power))));
        assert_eq!(parse_incoming(r#"{"key":"Left"}"#), Some(Incoming::Key(KeyEvent::press(Key::Left))));
        assert_eq!(parse_incoming(r#"{"key":"Menu"}"#), None);
        assert_eq!(parse_incoming(r#"{"text":"café \"q\""}"#), Some(Incoming::Text(String::from("café \"q\""))));
        assert_eq!(parse_incoming(r#"{"mirror":true}"#), Some(Incoming::Mirror(true)));
        assert_eq!(parse_incoming(r#"{"ping":1}"#), Some(Incoming::Ping));
        assert_eq!(parse_incoming("nope"), None);
        assert_eq!(parse_incoming("[1]"), None);
    }

    #[test]
    fn events() {
        assert_eq!(
            upload_event("a.epub", 5, 10, "uploading"),
            r#"{"ev":"upload","name":"a.epub","done":5,"total":10,"state":"uploading"}"#
        );
        assert_eq!(books_event(3), r#"{"ev":"books","added":3}"#);
        assert_eq!(typing_event(true), r#"{"ev":"typing","on":true}"#);
    }
}

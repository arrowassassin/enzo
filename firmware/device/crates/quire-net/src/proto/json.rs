//! A tiny JSON writer: builds objects and arrays straight into a `String` so a library
//! listing never becomes a tree of values first.

use alloc::string::String;
use core::fmt::Write;

/// Append `s` as a JSON string literal (with quotes).
pub fn push_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A JSON string literal.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    push_str(&mut out, s);
    out
}

/// Writes one object or array; `Drop` is not used so the close is explicit.
pub struct Json {
    out: String,
    /// Whether a comma is due before the next item, per nesting level.
    stack: heapless::Vec<bool, 16>,
}

impl Default for Json {
    fn default() -> Self {
        Self::new()
    }
}

impl Json {
    /// Empty writer.
    pub fn new() -> Self {
        Json { out: String::new(), stack: heapless::Vec::new() }
    }
    /// Writer with capacity.
    pub fn with_capacity(n: usize) -> Self {
        Json { out: String::with_capacity(n), stack: heapless::Vec::new() }
    }
    fn sep(&mut self) {
        if let Some(due) = self.stack.last_mut() {
            if *due {
                self.out.push(',');
            }
            *due = true;
        }
    }
    /// Start an object (as a value).
    pub fn obj(&mut self) -> &mut Self {
        self.sep();
        self.out.push('{');
        let _ = self.stack.push(false);
        self
    }
    /// Start an array (as a value).
    pub fn arr(&mut self) -> &mut Self {
        self.sep();
        self.out.push('[');
        let _ = self.stack.push(false);
        self
    }
    /// Close the innermost object or array.
    pub fn end(&mut self) -> &mut Self {
        let _ = self.stack.pop();
        self.out.push(self.close_char());
        self
    }
    fn close_char(&self) -> char {
        // Walk back to find the matching opener.
        let mut depth = 0i32;
        for b in self.out.bytes().rev() {
            match b {
                b'}' | b']' => depth += 1,
                b'{' | b'[' => {
                    if depth == 0 {
                        return if b == b'{' { '}' } else { ']' };
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        '}'
    }
    /// A key inside an object; follow with a value call.
    pub fn key(&mut self, k: &str) -> &mut Self {
        self.sep();
        push_str(&mut self.out, k);
        self.out.push(':');
        // The value that follows must not add a comma.
        if let Some(due) = self.stack.last_mut() {
            *due = false;
        }
        self
    }
    /// A string value.
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.sep();
        push_str(&mut self.out, s);
        self
    }
    /// An integer value.
    pub fn num(&mut self, n: impl Into<i64>) -> &mut Self {
        self.sep();
        let _ = write!(self.out, "{}", n.into());
        self
    }
    /// An unsigned 64-bit value.
    pub fn u64(&mut self, n: u64) -> &mut Self {
        self.sep();
        let _ = write!(self.out, "{n}");
        self
    }
    /// A boolean value.
    pub fn bool(&mut self, b: bool) -> &mut Self {
        self.sep();
        self.out.push_str(if b { "true" } else { "false" });
        self
    }
    /// `null`.
    pub fn null(&mut self) -> &mut Self {
        self.sep();
        self.out.push_str("null");
        self
    }
    /// Pre-rendered JSON.
    pub fn raw(&mut self, json: &str) -> &mut Self {
        self.sep();
        self.out.push_str(json);
        self
    }
    /// Key and string in one call.
    pub fn kv_str(&mut self, k: &str, v: &str) -> &mut Self {
        self.key(k).str(v)
    }
    /// Key and integer in one call.
    pub fn kv_num(&mut self, k: &str, v: impl Into<i64>) -> &mut Self {
        self.key(k).num(v)
    }
    /// Key and u64 in one call.
    pub fn kv_u64(&mut self, k: &str, v: u64) -> &mut Self {
        self.key(k).u64(v)
    }
    /// Key and boolean in one call.
    pub fn kv_bool(&mut self, k: &str, v: bool) -> &mut Self {
        self.key(k).bool(v)
    }
    /// Key and optional string (`null` when none).
    pub fn kv_opt_str(&mut self, k: &str, v: Option<&str>) -> &mut Self {
        match v {
            Some(s) => self.key(k).str(s),
            None => self.key(k).null(),
        }
    }
    /// The text so far.
    pub fn as_str(&self) -> &str {
        &self.out
    }
    /// Take the text.
    pub fn finish(self) -> String {
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes() {
        assert_eq!(quote("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\u0001\"");
        assert_eq!(quote("ünï"), "\"ünï\"");
    }

    #[test]
    fn nested() {
        let mut j = Json::new();
        j.obj().kv_str("a", "x").key("b").arr().num(1).num(2).obj().kv_bool("c", true).end().end().kv_opt_str("d", None).end();
        assert_eq!(j.as_str(), r#"{"a":"x","b":[1,2,{"c":true}],"d":null}"#);
        let v: serde_json::Value = serde_json::from_str(j.as_str()).unwrap();
        assert_eq!(v["b"][2]["c"], serde_json::Value::Bool(true));
    }

    #[test]
    fn empty_containers() {
        let mut j = Json::new();
        j.obj().key("a").arr().end().key("b").obj().end().end();
        assert_eq!(j.as_str(), r#"{"a":[],"b":{}}"#);
    }
}

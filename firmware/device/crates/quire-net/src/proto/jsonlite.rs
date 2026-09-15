//! A JSON scanner that never builds a tree: a [`Value`] is a byte slice covering exactly
//! one value, and object members / array items are found by skipping siblings. Used on
//! small, bounded responses (weather, Wikipedia, GitHub releases, sync, sleep-pack
//! manifests) where `serde_json::Value` would cost several times the document in heap.

use alloc::string::String;

/// One JSON value, as its exact text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Value<'a>(pub &'a [u8]);

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    i
}

/// Index just past the string starting at the quote at `i`.
fn skip_string(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'"' => return Some(j + 1),
            b'\\' => j += 2,
            _ => j += 1,
        }
    }
    None
}

/// Index just past the value starting at `i` (after whitespace).
fn skip_value(b: &[u8], i: usize) -> Option<usize> {
    let i = skip_ws(b, i);
    match *b.get(i)? {
        b'"' => skip_string(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => j = skip_string(b, j)?,
                    b'{' | b'[' => {
                        depth += 1;
                        j += 1;
                    }
                    b'}' | b']' => {
                        depth -= 1;
                        j += 1;
                        if depth == 0 {
                            return Some(j);
                        }
                    }
                    _ => j += 1,
                }
            }
            None
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n') {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

/// The whole document as a value (leading/trailing whitespace ignored).
pub fn parse(bytes: &[u8]) -> Option<Value<'_>> {
    let i = skip_ws(bytes, 0);
    let j = skip_value(bytes, i)?;
    Some(Value(&bytes[i..j]))
}

/// Decode a JSON string body (between the quotes) into text.
fn unescape(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let c = raw[i];
        if c != b'\\' {
            // Copy a run of plain bytes as UTF-8.
            let start = i;
            while i < raw.len() && raw[i] != b'\\' {
                i += 1;
            }
            out.push_str(&String::from_utf8_lossy(&raw[start..i]));
            continue;
        }
        i += 1;
        let Some(&e) = raw.get(i) else { break };
        i += 1;
        match e {
            b'n' => out.push('\n'),
            b't' => out.push('\t'),
            b'r' => out.push('\r'),
            b'b' | b'f' => {}
            b'u' => {
                let hex = |s: &[u8]| core::str::from_utf8(s).ok().and_then(|h| u32::from_str_radix(h, 16).ok());
                let Some(mut cp) = raw.get(i..i + 4).and_then(hex) else { break };
                i += 4;
                if (0xd800..0xdc00).contains(&cp) && raw.get(i..i + 2) == Some(b"\\u") {
                    if let Some(lo) = raw.get(i + 2..i + 6).and_then(hex) {
                        if (0xdc00..0xe000).contains(&lo) {
                            cp = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00);
                            i += 6;
                        }
                    }
                }
                out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
            }
            other => out.push(other as char),
        }
    }
    out
}

impl<'a> Value<'a> {
    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<Value<'a>> {
        self.entries().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    /// Nested lookup: `path` like `["current", "temperature_2m"]`.
    pub fn path(&self, path: &[&str]) -> Option<Value<'a>> {
        path.iter().try_fold(*self, |v, k| v.get(k))
    }
    /// The object's members, in order.
    pub fn entries(&self) -> impl Iterator<Item = (String, Value<'a>)> + 'a {
        let b = self.0;
        let mut i = if b.first() == Some(&b'{') { 1 } else { b.len() };
        core::iter::from_fn(move || {
            i = skip_ws(b, i);
            if *b.get(i)? == b',' {
                i = skip_ws(b, i + 1);
            }
            if *b.get(i)? != b'"' {
                return None;
            }
            let key_end = skip_string(b, i)?;
            let key = unescape(&b[i + 1..key_end - 1]);
            i = skip_ws(b, key_end);
            if *b.get(i)? != b':' {
                return None;
            }
            let start = skip_ws(b, i + 1);
            let end = skip_value(b, start)?;
            i = end;
            Some((key, Value(&b[start..end])))
        })
    }
    /// The array's items, in order.
    pub fn items(&self) -> impl Iterator<Item = Value<'a>> + 'a {
        let b = self.0;
        let mut i = if b.first() == Some(&b'[') { 1 } else { b.len() };
        core::iter::from_fn(move || {
            i = skip_ws(b, i);
            if *b.get(i)? == b',' {
                i = skip_ws(b, i + 1);
            }
            if *b.get(i)? == b']' {
                return None;
            }
            let end = skip_value(b, i)?;
            let v = Value(&b[i..end]);
            i = end;
            Some(v)
        })
    }
    /// The text of a string value.
    pub fn as_str(&self) -> Option<String> {
        let b = self.0;
        (b.len() >= 2 && b[0] == b'"' && b[b.len() - 1] == b'"').then(|| unescape(&b[1..b.len() - 1]))
    }
    /// A string, or `default`.
    pub fn str_or(&self, default: &str) -> String {
        self.as_str().unwrap_or_else(|| String::from(default))
    }
    /// An integer (floats truncated).
    pub fn as_i64(&self) -> Option<i64> {
        self.as_f64().map(|f| f as i64)
    }
    /// A number.
    pub fn as_f64(&self) -> Option<f64> {
        let s = core::str::from_utf8(self.0).ok()?;
        parse_f64(s)
    }
    /// A boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self.0 {
            b"true" => Some(true),
            b"false" => Some(false),
            _ => None,
        }
    }
    /// Whether the value is `null`.
    pub fn is_null(&self) -> bool {
        self.0 == b"null"
    }
}

/// A small decimal parser (`-12.5e3`), enough for JSON numbers.
pub fn parse_f64(s: &str) -> Option<f64> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], s[i + 1..].parse::<i32>().ok()?),
        None => (s, 0),
    };
    let (int, frac) = mant.split_once('.').unwrap_or((mant, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    let mut v = 0f64;
    for c in int.bytes() {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10.0 + (c - b'0') as f64;
    }
    let mut scale = 0.1;
    for c in frac.bytes() {
        if !c.is_ascii_digit() {
            return None;
        }
        v += (c - b'0') as f64 * scale;
        scale *= 0.1;
    }
    let mut e = exp;
    while e > 0 {
        v *= 10.0;
        e -= 1;
    }
    while e < 0 {
        v /= 10.0;
        e += 1;
    }
    Some(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &[u8] =
        r#" {"a": "x\"y\u00e9\ud83d\ude00", "n": -12.5, "arr": [1, {"b": true}, "s", null], "o": {"k": [2,3]}, "e": 1e3 } "#.as_bytes();

    #[test]
    fn navigates() {
        let v = parse(DOC).unwrap();
        assert_eq!(v.get("a").unwrap().as_str().unwrap(), "x\"yé😀");
        assert_eq!(v.get("n").unwrap().as_f64(), Some(-12.5));
        assert_eq!(v.get("e").unwrap().as_i64(), Some(1000));
        let items: alloc::vec::Vec<Value> = v.get("arr").unwrap().items().collect();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].as_i64(), Some(1));
        assert_eq!(items[1].get("b").unwrap().as_bool(), Some(true));
        assert!(items[3].is_null());
        assert_eq!(v.path(&["o", "k"]).unwrap().items().nth(1).unwrap().as_i64(), Some(3));
        assert!(v.get("missing").is_none());
        let keys: alloc::vec::Vec<String> = v.entries().map(|(k, _)| k).collect();
        assert_eq!(keys, ["a", "n", "arr", "o", "e"]);
    }

    #[test]
    fn tolerates_truncation() {
        let v = parse(br#"{"tag_name": "v1.2", "body": "notes that never en"#);
        assert!(v.is_none());
        // A truncated document still yields the members before the cut when scanned as an object.
        let v = Value(br#"{"tag_name": "v1.2", "body": "notes that never en"#);
        assert_eq!(v.get("tag_name").unwrap().as_str().unwrap(), "v1.2");
        assert!(v.get("body").is_none());
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_f64("3.25"), Some(3.25));
        assert_eq!(parse_f64("-0.5e1"), Some(-5.0));
        assert_eq!(parse_f64("abc"), None);
        assert_eq!(parse_f64("."), None);
    }
}

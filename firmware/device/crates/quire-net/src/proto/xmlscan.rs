//! A streaming XML tokenizer for feeds: bytes go in a chunk at a time, tags and text
//! come out as they complete, and only the unfinished tail of the current token is kept.
//! No DOM, no namespaces (names keep their prefix), no DTD; comments, processing
//! instructions and CDATA are handled, entities decoded (the five XML ones, numeric,
//! and the common HTML ones feeds use).

use alloc::string::String;
use alloc::vec::Vec;

/// A token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token<'a> {
    /// An opening (or self-closing, which also yields an `End`) tag: name and raw attributes.
    Start {
        /// Element name.
        name: &'a str,
        /// The raw attribute text.
        attrs: &'a str,
    },
    /// A closing tag.
    End {
        /// Element name.
        name: &'a str,
    },
    /// Character data, decoded.
    Text(String),
}

/// The tokenizer.
pub struct Tokenizer {
    buf: Vec<u8>,
    /// Longest token kept; longer text is flushed in pieces, a longer tag is dropped.
    max: usize,
}

impl Tokenizer {
    /// A tokenizer whose pending buffer stays under `max` bytes.
    pub fn new(max: usize) -> Self {
        Tokenizer { buf: Vec::new(), max }
    }

    /// Feed a chunk; `f` gets every token that completes.
    pub fn push(&mut self, chunk: &[u8], f: &mut dyn FnMut(Token)) {
        self.buf.extend_from_slice(chunk);
        let mut pos = 0usize;
        loop {
            let b = &self.buf[pos..];
            if b.is_empty() {
                break;
            }
            if b[0] != b'<' {
                // Text up to the next '<' (or the end, keeping an unfinished entity).
                let end = b.iter().position(|c| *c == b'<');
                let (take, done) = match end {
                    Some(e) => (e, true),
                    None => (b.len(), false),
                };
                let mut take = take;
                if !done {
                    if let Some(amp) = b[..take].iter().rposition(|c| *c == b'&') {
                        if take - amp < 12 && !b[amp..take].contains(&b';') {
                            take = amp;
                        }
                    }
                    if take == 0 && b.len() < self.max {
                        break;
                    }
                    if take == 0 {
                        take = b.len();
                    }
                }
                let text = decode(&String::from_utf8_lossy(&b[..take]));
                if !text.is_empty() {
                    f(Token::Text(text));
                }
                pos += take;
                continue;
            }
            // A markup construct starting at '<'.
            if b.starts_with(b"<!--") {
                match find(b, b"-->", 4) {
                    Some(e) => pos += e + 3,
                    None => break,
                }
                continue;
            }
            if b.starts_with(b"<![CDATA[") {
                match find(b, b"]]>", 9) {
                    Some(e) => {
                        f(Token::Text(String::from_utf8_lossy(&b[9..e]).into_owned()));
                        pos += e + 3;
                    }
                    None => {
                        if b.len() >= self.max {
                            // Flush the CDATA read so far, keeping the last two bytes.
                            let keep = b.len() - 2;
                            f(Token::Text(String::from_utf8_lossy(&b[9..keep]).into_owned()));
                            self.buf.drain(pos + 9..pos + keep);
                        }
                        break;
                    }
                }
                continue;
            }
            if b.starts_with(b"<?") || b.starts_with(b"<!") {
                match b.iter().position(|c| *c == b'>') {
                    Some(e) => pos += e + 1,
                    None => break,
                }
                continue;
            }
            // A tag: find its '>' outside quotes.
            let mut quote = 0u8;
            let mut end = None;
            for (i, c) in b.iter().enumerate().skip(1) {
                match (*c, quote) {
                    (b'"', 0) | (b'\'', 0) => quote = *c,
                    (c, q) if c == q => quote = 0,
                    (b'>', 0) => {
                        end = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            let Some(e) = end else {
                if b.len() >= self.max {
                    // Never-ending tag: drop it.
                    self.buf.clear();
                    return;
                }
                break;
            };
            let inner = core::str::from_utf8(&b[1..e]).unwrap_or("");
            if let Some(name) = inner.strip_prefix('/') {
                f(Token::End { name: name.trim() });
            } else {
                let self_closing = inner.ends_with('/');
                let inner = inner.trim_end_matches('/');
                let (name, attrs) = match inner.find(|c: char| c.is_ascii_whitespace()) {
                    Some(i) => (&inner[..i], inner[i..].trim()),
                    None => (inner, ""),
                };
                f(Token::Start { name, attrs });
                if self_closing {
                    f(Token::End { name });
                }
            }
            pos += e + 1;
        }
        self.buf.drain(..pos);
    }

    /// End of input: flushes trailing text.
    pub fn finish(&mut self, f: &mut dyn FnMut(Token)) {
        if !self.buf.is_empty() && self.buf[0] != b'<' {
            let text = decode(&String::from_utf8_lossy(&self.buf));
            if !text.is_empty() {
                f(Token::Text(text));
            }
        }
        self.buf.clear();
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..hay.len().saturating_sub(needle.len() - 1)).find(|&i| hay[i..].starts_with(needle))
}

/// The value of attribute `name` in raw attribute text, decoded.
pub fn attr(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().rsplit(|c: char| c.is_ascii_whitespace()).next().unwrap_or("");
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        let (value, next) = if quote == '"' || quote == '\'' {
            let body = &after[1..];
            let end = body.find(quote)?;
            (&body[..end], &body[end + 1..])
        } else {
            let end = after.find(|c: char| c.is_ascii_whitespace()).unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        if key == name {
            return Some(decode(value));
        }
        rest = next;
    }
    None
}

/// Decode character references.
pub fn decode(s: &str) -> String {
    if !s.contains('&') {
        return String::from(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "rsquo" => Some('’'),
            "lsquo" => Some('‘'),
            "rdquo" => Some('”'),
            "ldquo" => Some('“'),
            "copy" => Some('©'),
            "eacute" => Some('é'),
            n => n
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(h) => u32::from_str_radix(h, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(chunks: &[&str]) -> Vec<String> {
        let mut t = Tokenizer::new(256);
        let mut out = Vec::new();
        // Adjacent text pieces are one text to a consumer, so they are merged here.
        let mut f = |tok: Token| {
            if let Token::Text(s) = &tok {
                if let Some(last) = out.last_mut().filter(|l: &&mut String| l.starts_with('\'')) {
                    last.pop();
                    last.push_str(s);
                    last.push('\'');
                    return;
                }
            }
            out.push(match tok {
                Token::Start { name, attrs } => alloc::format!("<{name}|{attrs}>"),
                Token::End { name } => alloc::format!("</{name}>"),
                Token::Text(s) => alloc::format!("'{s}'"),
            })
        };
        for c in chunks {
            t.push(c.as_bytes(), &mut f);
        }
        t.finish(&mut f);
        out
    }

    #[test]
    fn splits_across_chunks() {
        let doc = r#"<?xml version="1.0"?><feed xmlns="x"><!-- c --><title>A &amp; B</title><link href="u" rel='alt'/><![CDATA[<p>raw</p>]]>tail"#;
        let whole = tokens(&[doc]);
        let mut pieces = Vec::new();
        let mut i = 0;
        while i < doc.len() {
            let mut j = (i + 7).min(doc.len());
            while !doc.is_char_boundary(j) {
                j += 1;
            }
            pieces.push(&doc[i..j]);
            i = j;
        }
        let split = tokens(&pieces);
        assert_eq!(whole, split);
        assert_eq!(
            whole,
            ["<feed|xmlns=\"x\">", "<title|>", "'A & B'", "</title>", "<link|href=\"u\" rel='alt'>", "</link>", "'<p>raw</p>tail'"]
        );
    }

    #[test]
    fn attributes_and_entities() {
        assert_eq!(attr(r#"rel="x" href='a&amp;b' type=text/html"#, "href").as_deref(), Some("a&b"));
        assert_eq!(attr(r#"rel="x" href='a' type=text/html"#, "type").as_deref(), Some("text/html"));
        assert_eq!(attr(r#"rel="x""#, "href"), None);
        assert_eq!(decode("&#169; &#x41;&unknown; &amp"), "© A&unknown; &amp");
        assert_eq!(decode("plain"), "plain");
    }

    #[test]
    fn entity_at_chunk_edge() {
        assert_eq!(tokens(&["<a>x &am", "p; y</a>"]), ["<a|>", "'x & y'", "</a>"]);
    }
}

//! QTX — Quire text stream.
//!
//! Every document format (EPUB, TXT, Markdown, FB2, `.qbk` …) is ingested once into this
//! token stream, one file per chapter, cached on the SD card. The layout engine has a
//! single input, and a reading position is a byte offset into the stream plus a word
//! index, which stays valid across typography changes.
//!
//! Encoding: each token is serialised with `postcard` and the encodings are concatenated,
//! so a reader can start at any token boundary and stream forward without loading the
//! whole chapter.
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Inline style flags (absolute; a `Style` token replaces the current set).
pub mod style {
    /// Bold.
    pub const BOLD: u8 = 1;
    /// Italic.
    pub const ITALIC: u8 = 2;
    /// Monospace.
    pub const MONO: u8 = 4;
    /// Small capitals.
    pub const SMALLCAPS: u8 = 8;
    /// Superscript.
    pub const SUP: u8 = 16;
    /// Subscript.
    pub const SUB: u8 = 32;
    /// Underline.
    pub const UNDERLINE: u8 = 64;
    /// Strikethrough.
    pub const STRIKE: u8 = 128;
}

/// Paragraph kinds, which decide typography (see the layout crate).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParaKind {
    /// Ordinary prose.
    Body,
    /// Heading level 1–3.
    Heading(u8),
    /// Block quotation.
    Quote,
    /// Verse: ragged right, hanging turns, preserved line breaks.
    Verse,
    /// List item: `ordered` numbering, nesting `level` from 0.
    ListItem {
        /// Numbered rather than bulleted.
        ordered: bool,
        /// Nesting level.
        level: u8,
        /// Item number for ordered lists (1-based), 0 otherwise.
        index: u16,
    },
    /// Code block (monospace, preserved whitespace).
    Code,
    /// Caption under an image or table.
    Caption,
    /// Centred text (title pages, epigraph attributions).
    Centered,
    /// A table row flattened to "label: value" lines.
    TableRow,
}

/// One token of the stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Token {
    /// Begin a paragraph of the given kind. Ends at the next `Para`, block token, or `End`.
    Para(ParaKind),
    /// A run of text in the current style.
    Text(String),
    /// Replace the current inline style flags.
    Style(u8),
    /// A block image. `id` indexes the chapter's image table; dimensions are the stored
    /// (already scaled) pixel size.
    Image {
        /// Image id within the chapter.
        id: u16,
        /// Width in pixels.
        w: u16,
        /// Height in pixels.
        h: u16,
    },
    /// A named position (link target, footnote body, TOC entry).
    Anchor(String),
    /// Start of a link to `target` (an anchor id, possibly `chapter#anchor`).
    Link(String),
    /// End of the current link.
    LinkEnd,
    /// A footnote marker referring to an anchor id; rendered as a superscript number.
    Footnote(String),
    /// Horizontal rule.
    Rule,
    /// Forced line break inside a paragraph.
    Break,
    /// A chapter opening block: optional number and title, typeset like a book.
    ChapterTitle {
        /// Chapter number as text ("20", "XII"), if any.
        number: Option<String>,
        /// Chapter title, if any.
        title: Option<String>,
    },
    /// Explicit end of the current paragraph.
    End,
}

/// Streaming writer.
#[derive(Default, Debug)]
pub struct Writer {
    buf: Vec<u8>,
    chars: u32,
}

impl Writer {
    /// Empty writer.
    pub fn new() -> Self {
        Self::default()
    }
    /// Append a token.
    pub fn push(&mut self, t: &Token) {
        if let Token::Text(s) = t {
            self.chars += s.chars().count() as u32;
        }
        let mut tmp = [0u8; 64];
        match postcard::to_slice(t, &mut tmp) {
            Ok(s) => self.buf.extend_from_slice(s),
            Err(_) => {
                let v = postcard::to_allocvec(t).expect("token serialises");
                self.buf.extend_from_slice(&v);
            }
        }
    }
    /// Convenience: a text run, encoded directly (no intermediate token or buffer).
    pub fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.chars += s.chars().count() as u32;
        self.buf.reserve(s.len() + 6);
        self.buf.push(TAG_TEXT);
        write_varint(&mut self.buf, s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
    }
    /// Convenience: a paragraph start.
    pub fn para(&mut self, k: ParaKind) {
        self.push(&Token::Para(k));
    }
    /// Convenience: set style.
    pub fn style(&mut self, flags: u8) {
        self.push(&Token::Style(flags));
    }
    /// Bytes so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    /// True when nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    /// Characters of text written so far (for progress estimates).
    pub fn char_count(&self) -> u32 {
        self.chars
    }
    /// Finish and take the bytes.
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
    /// Borrow the bytes written so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }
}

/// Streaming reader over an encoded stream.
#[derive(Clone, Copy, Debug)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Start at the beginning.
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }
    /// Start at a byte offset that is a token boundary.
    pub fn at(data: &'a [u8], offset: u32) -> Self {
        Reader { data, pos: (offset as usize).min(data.len()) }
    }
    /// Byte offset of the next token.
    pub fn offset(&self) -> u32 {
        self.pos as u32
    }
    /// Total length.
    pub fn len(&self) -> usize {
        self.data.len()
    }
    /// True when the stream is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    /// Whether the stream is exhausted.
    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }
    /// Peek the next token without consuming it.
    pub fn peek(&self) -> Option<Token> {
        let mut c = *self;
        c.next()
    }
}

impl Iterator for Reader<'_> {
    type Item = Token;
    fn next(&mut self) -> Option<Token> {
        if self.pos >= self.data.len() {
            return None;
        }
        match postcard::take_from_bytes::<Token>(&self.data[self.pos..]) {
            Ok((t, rest)) => {
                self.pos = self.data.len() - rest.len();
                Some(t)
            }
            Err(_) => {
                self.pos = self.data.len(); // corrupt tail: stop cleanly
                None
            }
        }
    }
}

/// Count text characters in a stream (progress denominators) without allocating.
pub fn char_count(data: &[u8]) -> u32 {
    let mut r = Reader::new(data);
    let mut n = 0u32;
    while let Some(tag) = r.peek_tag() {
        if tag == Tag::Text {
            if let Some(b) = r.text_bytes() {
                n += b.iter().filter(|x| (**x & 0xC0) != 0x80).count() as u32;
            }
        } else if r.skip_token().is_none() {
            break;
        }
    }
    n
}

/// Postcard tag of `Token::Text`.
const TAG_TEXT: u8 = 1;

fn write_varint(buf: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        buf.push((v as u8) | 0x80);
        v >>= 7;
    }
    buf.push(v as u8);
}

fn read_varint(data: &[u8], pos: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    let mut shift = 0;
    loop {
        let b = *data.get(*pos)?;
        *pos += 1;
        v |= ((b & 0x7F) as u32) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
}

/// The kind of the next token, read without decoding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    /// `Token::Para`.
    Para,
    /// `Token::Text`.
    Text,
    /// `Token::Style`.
    Style,
    /// `Token::Image`.
    Image,
    /// `Token::Anchor`.
    Anchor,
    /// `Token::Link`.
    Link,
    /// `Token::LinkEnd`.
    LinkEnd,
    /// `Token::Footnote`.
    Footnote,
    /// `Token::Rule`.
    Rule,
    /// `Token::Break`.
    Break,
    /// `Token::ChapterTitle`.
    ChapterTitle,
    /// `Token::End`.
    End,
}

impl<'a> Reader<'a> {
    /// The tag of the next token without consuming anything.
    pub fn peek_tag(&self) -> Option<Tag> {
        let b = *self.data.get(self.pos)?;
        Some(match b {
            0 => Tag::Para,
            1 => Tag::Text,
            2 => Tag::Style,
            3 => Tag::Image,
            4 => Tag::Anchor,
            5 => Tag::Link,
            6 => Tag::LinkEnd,
            7 => Tag::Footnote,
            8 => Tag::Rule,
            9 => Tag::Break,
            10 => Tag::ChapterTitle,
            11 => Tag::End,
            _ => return None,
        })
    }

    /// Skip the next token without decoding it (no allocation). Returns its tag.
    /// (Named `skip_token` because `Iterator::skip` already exists.)
    pub fn skip_token(&mut self) -> Option<Tag> {
        let tag = self.peek_tag()?;
        let d = self.data;
        let mut p = self.pos + 1;
        let skip_string = |p: &mut usize| -> Option<()> {
            let n = read_varint(d, p)? as usize;
            *p = p.checked_add(n)?;
            (*p <= d.len()).then_some(())
        };
        let ok = match tag {
            Tag::Para => (|| {
                let k = read_varint(d, &mut p)?;
                match k {
                    1 => p += 1,
                    4 => {
                        p += 2;
                        read_varint(d, &mut p)?;
                    }
                    _ => {}
                }
                (p <= d.len()).then_some(())
            })(),
            Tag::Text | Tag::Anchor | Tag::Link | Tag::Footnote => skip_string(&mut p),
            Tag::Style => {
                p += 1;
                (p <= d.len()).then_some(())
            }
            Tag::Image => (|| {
                read_varint(d, &mut p)?;
                read_varint(d, &mut p)?;
                read_varint(d, &mut p)?;
                Some(())
            })(),
            Tag::LinkEnd | Tag::Rule | Tag::Break | Tag::End => Some(()),
            Tag::ChapterTitle => (|| {
                for _ in 0..2 {
                    let some = *d.get(p)?;
                    p += 1;
                    if some == 1 {
                        skip_string(&mut p)?;
                    }
                }
                Some(())
            })(),
        };
        match ok {
            Some(()) => {
                self.pos = p;
                Some(tag)
            }
            None => {
                self.pos = d.len();
                None
            }
        }
    }

    /// If the next token is `Text`, consume it and return its UTF-8 bytes without copying.
    pub fn text_bytes(&mut self) -> Option<&'a [u8]> {
        if self.peek_tag()? != Tag::Text {
            return None;
        }
        let mut p = self.pos + 1;
        let n = read_varint(self.data, &mut p)? as usize;
        let end = p.checked_add(n)?;
        if end > self.data.len() {
            self.pos = self.data.len();
            return None;
        }
        self.pos = end;
        Some(&self.data[p..end])
    }

    /// If the next token is a string-bearing one (Anchor, Link, Footnote, Text), consume it and
    /// return the tag and the string bytes without copying.
    pub fn string_bytes(&mut self) -> Option<(Tag, &'a [u8])> {
        let tag = self.peek_tag()?;
        if !matches!(tag, Tag::Text | Tag::Anchor | Tag::Link | Tag::Footnote) {
            return None;
        }
        let mut p = self.pos + 1;
        let n = read_varint(self.data, &mut p)? as usize;
        let end = p.checked_add(n)?;
        if end > self.data.len() {
            self.pos = self.data.len();
            return None;
        }
        self.pos = end;
        Some((tag, &self.data[p..end]))
    }
}

/// Extract plain text (search, dictionary context), paragraphs separated by newlines.
pub fn plain_text(data: &[u8]) -> String {
    let mut out = String::new();
    for t in Reader::new(data) {
        match t {
            Token::Text(s) => out.push_str(&s),
            Token::Para(_) | Token::End | Token::Rule | Token::Image { .. } => {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
            }
            Token::Break => out.push('\n'),
            Token::ChapterTitle { number, title } => {
                if let Some(n) = number {
                    out.push_str(&n);
                    out.push(' ');
                }
                if let Some(t) = title {
                    out.push_str(&t);
                }
                out.push('\n');
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_matches_decode_and_text_fast_path_encodes_identically() {
        let mut w = Writer::new();
        w.para(ParaKind::Heading(2));
        w.text("Héllo wörld, a run long enough to exceed the sixty-four byte stack buffer used by postcard's to_slice call.");
        w.style(style::BOLD | style::ITALIC);
        w.push(&Token::Image { id: 300, w: 480, h: 640 });
        w.push(&Token::Anchor(String::from("ch1")));
        w.push(&Token::Link(String::from("3#note-1")));
        w.push(&Token::LinkEnd);
        w.push(&Token::Footnote(String::from("n1")));
        w.push(&Token::Rule);
        w.push(&Token::Break);
        w.push(&Token::ChapterTitle { number: Some(String::from("XII")), title: None });
        w.push(&Token::ChapterTitle { number: None, title: Some(String::from("Loomings")) });
        w.para(ParaKind::ListItem { ordered: true, level: 2, index: 300 });
        w.push(&Token::End);
        let data = w.finish();
        // The hand-encoded text token decodes like a postcard one.
        let mut w2 = Writer::new();
        w2.push(&Token::Text(String::from(
            "Héllo wörld, a run long enough to exceed the sixty-four byte stack buffer used by postcard's to_slice call.",
        )));
        let mut w3 = Writer::new();
        w3.text("Héllo wörld, a run long enough to exceed the sixty-four byte stack buffer used by postcard's to_slice call.");
        assert_eq!(w2.finish(), w3.finish());
        // skip() lands on the same boundaries as next().
        let mut a = Reader::new(&data);
        let mut b = Reader::new(&data);
        let mut tags = Vec::new();
        loop {
            let t = a.next();
            let s = b.skip_token();
            assert_eq!(a.offset(), b.offset());
            match (t, s) {
                (None, None) => break,
                (Some(_), Some(tag)) => tags.push(tag),
                other => panic!("mismatch {other:?}"),
            }
        }
        assert_eq!(tags[0], Tag::Para);
        assert_eq!(tags[1], Tag::Text);
        assert_eq!(tags.len(), 14);
        assert_eq!(
            char_count(&data),
            "Héllo wörld, a run long enough to exceed the sixty-four byte stack buffer used by postcard's to_slice call.".chars().count()
                as u32
        );
        let mut r = Reader::new(&data);
        r.skip_token();
        assert_eq!(
            r.text_bytes().map(|b| b.len()),
            Some("Héllo wörld, a run long enough to exceed the sixty-four byte stack buffer used by postcard's to_slice call.".len())
        );
    }
    use alloc::string::ToString;

    #[test]
    fn round_trip_and_offsets() {
        let mut w = Writer::new();
        w.para(ParaKind::Body);
        w.text("Hello, ");
        w.style(style::ITALIC);
        w.text("world");
        w.push(&Token::Footnote("n1".to_string()));
        w.push(&Token::Image { id: 3, w: 480, h: 320 });
        let bytes = w.finish();
        let toks: Vec<Token> = Reader::new(&bytes).collect();
        assert_eq!(toks.len(), 6);
        assert_eq!(toks[2], Token::Style(style::ITALIC));
        // offsets are token boundaries
        let mut r = Reader::new(&bytes);
        r.next();
        let off = r.offset();
        let mut r2 = Reader::at(&bytes, off);
        assert_eq!(r2.next(), Some(Token::Text("Hello, ".to_string())));
        assert_eq!(char_count(&bytes), 12);
        assert_eq!(plain_text(&bytes), "Hello, world\n");
    }

    #[test]
    fn corrupt_tail_stops_cleanly() {
        let mut w = Writer::new();
        w.text("abc");
        let mut bytes = w.finish();
        bytes.push(0xFF);
        bytes.push(0xFF);
        let toks: Vec<Token> = Reader::new(&bytes).collect();
        assert_eq!(toks.len(), 1);
    }
}

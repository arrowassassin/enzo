//! FictionBook 2: plain XML with a known vocabulary, mapped directly onto QTX.
//!
//! The file is parsed whole (it is small by nature; the device refuses anything over
//! 1.5 MB). `<binary>` elements are only *located* up front: a binary's base64 is
//! decoded on demand when something references it — the cover first, inline images as
//! they are met — and dropped again straight after, so unreferenced attachments and
//! images not yet reached never occupy the heap.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_qtx::{style, ParaKind, Token, Writer};

use crate::html::{tokenize, Ev};
use crate::image::{Fit, ImageKind};
use crate::{limits, DocError, Metadata, Sink, TocEntry};

/// Largest FB2 we will parse.
#[cfg(target_os = "none")]
const FILE_LIMIT: usize = 1536 * 1024;
/// Largest FB2 we will parse.
#[cfg(not(target_os = "none"))]
const FILE_LIMIT: usize = 24 * 1024 * 1024;

/// Incremental standard base64 decoder (whitespace tolerated); text can arrive in pieces.
#[derive(Default)]
struct Base64 {
    acc: u32,
    n: u32,
    done: bool,
}

impl Base64 {
    fn feed(&mut self, s: &str, out: &mut Vec<u8>) {
        if self.done {
            return;
        }
        out.reserve(s.len() * 3 / 4);
        for c in s.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' | b'-' => 62,
                b'/' | b'_' => 63,
                b'=' => {
                    self.done = true;
                    return;
                }
                _ => continue,
            } as u32;
            self.acc = (self.acc << 6) | v;
            self.n += 6;
            if self.n >= 8 {
                self.n -= 8;
                out.push((self.acc >> self.n) as u8);
                self.acc &= (1 << self.n) - 1;
            }
        }
    }
}

/// Decode standard base64 (whitespace tolerated).
pub fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    Base64::default().feed(s, &mut out);
    out
}

/// A `<binary>` element: id, content type, and the indices of its text events.
struct Binary {
    id: String,
    content_type: String,
    text_events: Vec<usize>,
}

impl Binary {
    /// Decode the base64 now (and only now).
    fn bytes(&self, events: &[Ev<'_>]) -> Vec<u8> {
        let mut d = Base64::default();
        let mut out = Vec::new();
        for &i in &self.text_events {
            if let Some(Ev::Text(t)) = events.get(i) {
                d.feed(t, &mut out);
            }
        }
        out
    }
}

/// Ingest an FB2 file (parsed whole; see the module docs for the memory story).
pub fn ingest<R: ReadAt>(file: &R, sink: &mut dyn Sink) -> Result<(), DocError> {
    let len = file.len() as usize;
    if len > FILE_LIMIT {
        return Err(DocError::TooLarge("fb2 file"));
    }
    let data = file.read_range(0, len)?;
    let text = crate::txt::decode_bytes(&data);
    drop(data);
    let events = tokenize(&text);

    // Pass 1: metadata, and where the binaries are (not their contents).
    let mut meta = Metadata::default();
    let mut binaries: Vec<Binary> = Vec::new();
    let mut cover_href: Option<String> = None;
    {
        let mut path: Vec<String> = Vec::new();
        let mut cur_binary: Option<Binary> = None;
        let mut first: Option<String> = None;
        let mut last: Option<String> = None;
        for (ei, ev) in events.iter().enumerate() {
            match ev {
                Ev::Open { name, attrs, self_closing } => {
                    let n = name.to_ascii_lowercase();
                    if n == "binary" {
                        let id = attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case("id")).map(|(_, v)| v.clone()).unwrap_or_default();
                        let ct =
                            attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.clone()).unwrap_or_default();
                        cur_binary = Some(Binary { id, content_type: ct, text_events: Vec::new() });
                    }
                    if n == "image" && path.iter().any(|p| p == "coverpage") {
                        cover_href = attrs
                            .iter()
                            .find(|(k, _)| k.to_ascii_lowercase().ends_with("href"))
                            .map(|(_, v)| v.trim_start_matches('#').into());
                    }
                    if !*self_closing {
                        path.push(n);
                    }
                }
                Ev::Close(name) => {
                    let n = name.to_ascii_lowercase();
                    if n == "binary" {
                        if let Some(b) = cur_binary.take() {
                            binaries.push(b);
                        }
                    }
                    if n == "author" && path.iter().any(|p| p == "title-info") {
                        let mut a = String::new();
                        if let Some(f) = first.take() {
                            a.push_str(&f);
                        }
                        if let Some(l) = last.take() {
                            if !a.is_empty() {
                                a.push(' ');
                            }
                            a.push_str(&l);
                        }
                        if !a.is_empty() {
                            meta.authors.push(a);
                        }
                    }
                    if let Some(i) = path.iter().rposition(|p| *p == n) {
                        path.truncate(i);
                    }
                }
                Ev::Text(t) => {
                    if let Some(b) = cur_binary.as_mut() {
                        b.text_events.push(ei);
                        continue;
                    }
                    let t = crate::html::collapse_ws(t);
                    if t.is_empty() || !path.iter().any(|p| p == "title-info") {
                        continue;
                    }
                    match path.last().map(|s| s.as_str()) {
                        Some("book-title") if meta.title.is_empty() => meta.title = t,
                        Some("first-name") => first = Some(t),
                        Some("last-name") => last = Some(t),
                        Some("lang") if meta.language.is_empty() => meta.language = t,
                        Some("genre") => meta.subjects.push(t),
                        Some("p") if path.iter().any(|p| p == "annotation") => {
                            let d = meta.description.get_or_insert_with(String::new);
                            if !d.is_empty() {
                                d.push(' ');
                            }
                            d.push_str(&t);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    if meta.title.is_empty() {
        meta.title = "Untitled".into();
    }
    sink.metadata(&meta)?;

    // Cover: decoded once for both the full page and the thumbnail, then dropped.
    if let Some(href) = cover_href.as_deref() {
        if let Some(b) = binaries.iter().find(|b| b.id == href) {
            let bytes = b.bytes(&events);
            let kind = ImageKind::from_hint(&b.content_type);
            if let Ok(mut pair) = crate::image::decode_multi(&bytes, kind, &Fit::cover_pair()) {
                if let (Some(thumb), Some(full)) = (pair.pop(), pair.pop()) {
                    sink.cover(&full, &thumb)?;
                }
            }
        }
    }

    // Pass 2: bodies → chapters (one per top-level section).
    let mut toc: Vec<TocEntry> = Vec::new();
    let mut chapter: u16 = 0;
    let mut w: Option<Writer> = None;
    let mut depth_stack: Vec<String> = Vec::new();
    let mut section_depth = 0u8;
    let mut in_title = false;
    let mut title_buf = String::new();
    let mut in_para = false;
    let mut styleflags = 0u8;
    let mut chapter_title: Option<String> = None;
    let mut in_body = false;
    let mut notes_body = false;

    let start_para = |w: &mut Writer, k: ParaKind, in_para: &mut bool, flags: u8| {
        if *in_para {
            w.push(&Token::End);
        }
        w.para(k);
        if flags != 0 {
            w.style(flags);
        }
        *in_para = true;
    };

    let flush_chapter = |w: &mut Option<Writer>, chapter: &mut u16, sink: &mut dyn Sink, title: &Option<String>| -> Result<(), DocError> {
        if let Some(wr) = w.take() {
            if !wr.is_empty() {
                let chars = wr.char_count();
                sink.begin_chapter(*chapter, title.as_deref())?;
                sink.chapter_bytes(wr.as_bytes())?;
                sink.end_chapter(chars)?;
                *chapter += 1;
            }
        }
        Ok(())
    };

    for ev in &events {
        match ev {
            Ev::Open { name, attrs, self_closing } => {
                let n = name.to_ascii_lowercase();
                match n.as_str() {
                    "body" => {
                        in_body = true;
                        notes_body = attrs.iter().any(|(k, v)| k.eq_ignore_ascii_case("name") && v.eq_ignore_ascii_case("notes"));
                        section_depth = 0;
                    }
                    "section" if in_body && !notes_body => {
                        if section_depth == 0 {
                            if in_para {
                                if let Some(wr) = w.as_mut() {
                                    wr.push(&Token::End);
                                }
                                in_para = false;
                            }
                            flush_chapter(&mut w, &mut chapter, sink, &chapter_title)?;
                            w = Some(Writer::new());
                            chapter_title = None;
                        }
                        section_depth += 1;
                    }
                    "title" if in_body && !notes_body => {
                        in_title = true;
                        title_buf.clear();
                        if w.is_none() {
                            w = Some(Writer::new());
                        }
                        if let Some(wr) = w.as_mut() {
                            start_para(wr, ParaKind::Heading(section_depth.clamp(1, 3)), &mut in_para, styleflags);
                        }
                    }
                    "subtitle" if in_body => {
                        if let Some(wr) = w.as_mut() {
                            start_para(wr, ParaKind::Heading(3), &mut in_para, styleflags);
                        }
                    }
                    "p" | "v" | "text-author" if in_body && !notes_body => {
                        if w.is_none() {
                            w = Some(Writer::new());
                        }
                        let kind = if n == "v" {
                            ParaKind::Verse
                        } else if n == "text-author" || depth_stack.iter().any(|d| d == "epigraph") {
                            ParaKind::Centered
                        } else if depth_stack.iter().any(|d| d == "cite") {
                            ParaKind::Quote
                        } else {
                            ParaKind::Body
                        };
                        if let Some(wr) = w.as_mut() {
                            if in_title {
                                // Paragraphs inside a title continue the heading.
                                if title_buf.is_empty() {
                                    // first paragraph: already started
                                } else {
                                    wr.push(&Token::Break);
                                }
                            } else {
                                start_para(wr, kind, &mut in_para, styleflags);
                            }
                        }
                    }
                    "empty-line" if in_body => {
                        if let Some(wr) = w.as_mut() {
                            if in_para {
                                wr.push(&Token::End);
                                in_para = false;
                            }
                        }
                    }
                    "emphasis" => {
                        styleflags |= style::ITALIC;
                        if let Some(wr) = w.as_mut() {
                            if in_para {
                                wr.style(styleflags);
                            }
                        }
                    }
                    "strong" => {
                        styleflags |= style::BOLD;
                        if let Some(wr) = w.as_mut() {
                            if in_para {
                                wr.style(styleflags);
                            }
                        }
                    }
                    "sup" | "sub" | "strikethrough" | "code" => {
                        styleflags |= match n.as_str() {
                            "sup" => style::SUP,
                            "sub" => style::SUB,
                            "strikethrough" => style::STRIKE,
                            _ => style::MONO,
                        };
                        if let Some(wr) = w.as_mut() {
                            if in_para {
                                wr.style(styleflags);
                            }
                        }
                    }
                    "a" if in_body => {
                        let href = attrs
                            .iter()
                            .find(|(k, _)| k.to_ascii_lowercase().ends_with("href"))
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default();
                        let is_note = attrs.iter().any(|(k, v)| k.eq_ignore_ascii_case("type") && v == "note");
                        if let Some(wr) = w.as_mut() {
                            if in_para {
                                if is_note {
                                    wr.push(&Token::Footnote(href.trim_start_matches('#').into()));
                                } else if !href.is_empty() {
                                    wr.push(&Token::Link(href));
                                }
                            }
                        }
                    }
                    "image" if in_body && !notes_body => {
                        let href = attrs
                            .iter()
                            .find(|(k, _)| k.to_ascii_lowercase().ends_with("href"))
                            .map(|(_, v)| String::from(v.trim_start_matches('#')))
                            .unwrap_or_default();
                        if let Some(b) = binaries.iter().find(|b| b.id == href) {
                            // Decoded from base64 here, used, and dropped with this block.
                            let bytes = b.bytes(&events);
                            let kind = ImageKind::from_hint(&b.content_type);
                            if let Ok(bm) = crate::image::decode(&bytes, kind, Fit::inside(limits::IMAGE_W, limits::IMAGE_H)) {
                                let id = sink.image(&bm)?;
                                if w.is_none() {
                                    w = Some(Writer::new());
                                }
                                if let Some(wr) = w.as_mut() {
                                    if in_para {
                                        wr.push(&Token::End);
                                        in_para = false;
                                    }
                                    wr.push(&Token::Image { id, w: bm.w as u16, h: bm.h as u16 });
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if !*self_closing {
                    depth_stack.push(n);
                }
            }
            Ev::Close(name) => {
                let n = name.to_ascii_lowercase();
                match n.as_str() {
                    "body" => {
                        if in_para {
                            if let Some(wr) = w.as_mut() {
                                wr.push(&Token::End);
                            }
                            in_para = false;
                        }
                        flush_chapter(&mut w, &mut chapter, sink, &chapter_title)?;
                        chapter_title = None;
                        in_body = false;
                        notes_body = false;
                    }
                    "section" if in_body && !notes_body => section_depth = section_depth.saturating_sub(1),
                    "title" if in_body && !notes_body => {
                        in_title = false;
                        let t = crate::html::collapse_ws(&title_buf);
                        if !t.is_empty() {
                            if chapter_title.is_none() {
                                chapter_title = Some(t.clone());
                            }
                            toc.push(TocEntry { title: t, chapter, anchor: None, depth: section_depth.saturating_sub(1) });
                        }
                        if let Some(wr) = w.as_mut() {
                            if in_para {
                                wr.push(&Token::End);
                                in_para = false;
                            }
                        }
                    }
                    "subtitle" | "p" | "v" | "text-author" if in_body => {
                        if !in_title {
                            if let Some(wr) = w.as_mut() {
                                if in_para {
                                    wr.push(&Token::End);
                                    in_para = false;
                                }
                            }
                        }
                    }
                    "emphasis" | "strong" | "sup" | "sub" | "strikethrough" | "code" => {
                        styleflags &= !match n.as_str() {
                            "emphasis" => style::ITALIC,
                            "strong" => style::BOLD,
                            "sup" => style::SUP,
                            "sub" => style::SUB,
                            "strikethrough" => style::STRIKE,
                            _ => style::MONO,
                        };
                        if let Some(wr) = w.as_mut() {
                            if in_para {
                                wr.style(styleflags);
                            }
                        }
                    }
                    "a" => {
                        if let Some(wr) = w.as_mut() {
                            if in_para {
                                wr.push(&Token::LinkEnd);
                            }
                        }
                    }
                    _ => {}
                }
                if let Some(i) = depth_stack.iter().rposition(|d| *d == n) {
                    depth_stack.truncate(i);
                }
            }
            Ev::Text(t) => {
                if !in_body || notes_body {
                    continue;
                }
                let s = crate::html::collapse_ws(t);
                if s.is_empty() {
                    continue;
                }
                if in_title {
                    if !title_buf.is_empty() {
                        title_buf.push(' ');
                    }
                    title_buf.push_str(&s);
                }
                if let Some(wr) = w.as_mut() {
                    if !in_para {
                        start_para(wr, ParaKind::Body, &mut in_para, styleflags);
                    }
                    let lead = t.starts_with(char::is_whitespace);
                    let mut txt = String::new();
                    if lead {
                        txt.push(' ');
                    }
                    txt.push_str(&s);
                    if t.ends_with(char::is_whitespace) {
                        txt.push(' ');
                    }
                    wr.text(&txt);
                }
            }
        }
    }
    if in_para {
        if let Some(wr) = w.as_mut() {
            wr.push(&Token::End);
        }
    }
    flush_chapter(&mut w, &mut chapter, sink, &chapter_title)?;
    if chapter == 0 {
        return Err(DocError::Malformed("fb2 has no body text"));
    }
    if toc.is_empty() {
        toc.push(TocEntry { title: meta.title.clone(), chapter: 0, anchor: None, depth: 0 });
    }
    sink.toc(&toc)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memsink::MemSink;

    #[test]
    fn base64() {
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ="), b"hello world");
        assert_eq!(base64_decode("aGVs\nbG8="), b"hello");
        // Incremental feeding across arbitrary cut points gives the same bytes.
        let mut d = Base64::default();
        let mut out = Vec::new();
        for piece in ["aG", "VsbG8", "gd2", "9ybGQ="] {
            d.feed(piece, &mut out);
        }
        assert_eq!(out, b"hello world");
    }

    #[test]
    fn cover_and_inline_images_are_decoded_only_when_referenced() {
        let png = crate::png::tests::encode(120, 160, 0, &|x, _| [(x * 2) as u8, 0, 0, 255], 0, false);
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for c in png.chunks(3) {
                let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                s.push(T[(n >> 18) as usize & 63] as char);
                s.push(T[(n >> 12) as usize & 63] as char);
                s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
                s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
                if s.len().is_multiple_of(77) {
                    s.push('\n');
                }
            }
            s
        };
        let fb2 = alloc::format!(
            r##"<?xml version="1.0"?><FictionBook xmlns:l="http://www.w3.org/1999/xlink">
<description><title-info><book-title>Pics</book-title><coverpage><image l:href="#cover.png"/></coverpage></title-info></description>
<body><section><title><p>One</p></title><p>Text.</p><image l:href="#inline.png"/><image l:href="#missing.png"/></section></body>
<binary id="cover.png" content-type="image/png">{b64}</binary>
<binary id="inline.png" content-type="image/png">{b64}</binary>
<binary id="unused.bin" content-type="application/octet-stream">not even base64 ****</binary>
</FictionBook>"##
        );
        let mut sink = MemSink::default();
        ingest(&fb2.as_bytes(), &mut sink).expect("ingest");
        let [ff, ft] = Fit::cover_pair();
        let (full, thumb) = sink.cover.as_ref().expect("cover");
        assert_eq!(*full, crate::image::decode(&png, ImageKind::Png, ff).unwrap());
        assert_eq!(*thumb, crate::image::decode(&png, ImageKind::Png, ft).unwrap());
        assert_eq!(sink.images.len(), 1, "only the referenced, existing inline image is stored");
        assert_eq!(sink.images[0], crate::image::decode(&png, ImageKind::Png, Fit::inside(limits::IMAGE_W, limits::IMAGE_H)).unwrap());
    }

    #[test]
    fn fb2_sample_ingests() {
        let fb2 = r##"<?xml version="1.0" encoding="UTF-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
<description><title-info><genre>prose</genre><author><first-name>Anton</first-name><last-name>Chekhov</last-name></author>
<book-title>The Bet</book-title><lang>en</lang><annotation><p>A short story.</p></annotation></title-info></description>
<body><title><p>The Bet</p></title>
<section><title><p>I</p></title><p>It was a dark autumn night. The old banker was <emphasis>walking</emphasis> up and down his study.</p>
<empty-line/><poem><stanza><v>Line one</v><v>Line two</v></stanza></poem><cite><p>Quoted text.</p></cite></section>
<section><title><p>II</p></title><p>Second section text<a l:href="#n1" type="note">1</a>.</p></section>
</body><body name="notes"><section id="n1"><p>A note.</p></section></body></FictionBook>"##;
        let mut sink = MemSink::default();
        ingest(&fb2.as_bytes(), &mut sink).expect("ingest");
        assert_eq!(sink.meta.title, "The Bet");
        assert_eq!(sink.meta.authors, ["Anton Chekhov"]);
        assert_eq!(sink.meta.language, "en");
        assert!(sink.chapters.len() >= 2, "{}", sink.chapters.len());
        let text = sink.all_text();
        assert!(text.contains("dark autumn night"));
        assert!(text.contains("Line one"));
        assert!(!text.contains("A note."), "notes body is not a chapter");
        assert!(sink.toc.iter().any(|t| t.title == "II"));
        let toks: Vec<Token> = sink.chapters.iter().flat_map(|c| quire_qtx::Reader::new(&c.1).collect::<Vec<_>>()).collect();
        assert!(toks.contains(&Token::Para(ParaKind::Verse)));
        assert!(toks.contains(&Token::Para(ParaKind::Quote)));
        assert!(toks.contains(&Token::Footnote("n1".into())));
    }
}

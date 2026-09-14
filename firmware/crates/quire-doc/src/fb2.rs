//! FictionBook 2: plain XML with a known vocabulary, mapped directly onto QTX.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_qtx::{style, ParaKind, Token, Writer};

use crate::html::{tokenize, Ev};
use crate::image::{Fit, ImageKind};
use crate::{limits, DocError, Metadata, Sink, TocEntry};

/// Decode standard base64 (whitespace tolerated).
pub fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut n = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        } as u32;
        acc = (acc << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
            acc &= (1 << n) - 1;
        }
    }
    out
}

/// Ingest an FB2 file (optionally the whole file, which is small by nature).
pub fn ingest<R: ReadAt>(file: &R, sink: &mut dyn Sink) -> Result<(), DocError> {
    let len = file.len() as usize;
    if len > 24 * 1024 * 1024 {
        return Err(DocError::TooLarge("fb2 over 24 MB"));
    }
    let data = file.read_range(0, len)?;
    let text = crate::txt::decode_bytes(&data);
    let events = tokenize(&text);

    // Pass 1: metadata and binaries.
    let mut meta = Metadata::default();
    let mut binaries: Vec<(String, String, Vec<u8>)> = Vec::new(); // (id, content-type, bytes)
    let mut cover_href: Option<String> = None;
    {
        let mut path: Vec<String> = Vec::new();
        let mut cur_binary: Option<(String, String, String)> = None;
        let mut first: Option<String> = None;
        let mut last: Option<String> = None;
        for ev in &events {
            match ev {
                Ev::Open { name, attrs, self_closing } => {
                    let n = name.to_ascii_lowercase();
                    if n == "binary" {
                        let id = attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case("id")).map(|(_, v)| v.clone()).unwrap_or_default();
                        let ct = attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.clone()).unwrap_or_default();
                        cur_binary = Some((id, ct, String::new()));
                    }
                    if n == "image" && path.iter().any(|p| p == "coverpage") {
                        cover_href = attrs.iter().find(|(k, _)| k.to_ascii_lowercase().ends_with("href")).map(|(_, v)| v.trim_start_matches('#').into());
                    }
                    if !*self_closing {
                        path.push(n);
                    }
                }
                Ev::Close(name) => {
                    let n = name.to_ascii_lowercase();
                    if n == "binary" {
                        if let Some((id, ct, b64)) = cur_binary.take() {
                            binaries.push((id, ct, base64_decode(&b64)));
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
                    if let Some((_, _, b)) = cur_binary.as_mut() {
                        b.push_str(t);
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

    // Cover.
    if let Some(href) = cover_href.as_deref() {
        if let Some((_, ct, bytes)) = binaries.iter().find(|(id, _, _)| id == href) {
            let kind = ImageKind::from_hint(ct);
            if let (Ok(full), Ok(thumb)) = (
                crate::image::decode(bytes, kind, Fit::fill(limits::COVER_W, limits::COVER_H)),
                crate::image::decode(bytes, kind, Fit { fs: false, ..Fit::fill(limits::THUMB_W, limits::THUMB_H) }),
            ) {
                sink.cover(&full, &thumb)?;
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
                        let href = attrs.iter().find(|(k, _)| k.to_ascii_lowercase().ends_with("href")).map(|(_, v)| v.clone()).unwrap_or_default();
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
                        let href = attrs.iter().find(|(k, _)| k.to_ascii_lowercase().ends_with("href")).map(|(_, v)| String::from(v.trim_start_matches('#'))).unwrap_or_default();
                        if let Some((_, ct, bytes)) = binaries.iter().find(|(id, _, _)| *id == href) {
                            if let Ok(bm) = crate::image::decode(bytes, ImageKind::from_hint(ct), Fit::inside(limits::IMAGE_W, limits::IMAGE_H)) {
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

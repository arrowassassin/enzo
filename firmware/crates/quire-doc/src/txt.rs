//! Plain text: encoding detection, paragraph splitting and reflow of hard-wrapped
//! Gutenberg-style files, streamed in chunks.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_qtx::{ParaKind, Writer};

use crate::{DocError, Metadata, Sink, TocEntry};

/// Decode bytes as text: UTF-8 (with or without BOM), UTF-16 with BOM, else Windows-1252.
pub fn decode_bytes(data: &[u8]) -> String {
    if let Some(rest) = data.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if data.len() >= 2 && (data[..2] == [0xFF, 0xFE] || data[..2] == [0xFE, 0xFF]) {
        let be = data[0] == 0xFE;
        let units: Vec<u16> =
            data[2..].as_chunks::<2>().0.iter().map(|c| if be { u16::from_be_bytes(*c) } else { u16::from_le_bytes(*c) }).collect();
        return char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect();
    }
    match core::str::from_utf8(data) {
        Ok(s) => String::from(s),
        Err(_) => data.iter().map(|&b| cp1252(b)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    match b {
        0x80 => '€',
        0x82 => '‚',
        0x83 => 'ƒ',
        0x84 => '„',
        0x85 => '…',
        0x86 => '†',
        0x87 => '‡',
        0x88 => 'ˆ',
        0x89 => '‰',
        0x8A => 'Š',
        0x8B => '‹',
        0x8C => 'Œ',
        0x8E => 'Ž',
        0x91 => '‘',
        0x92 => '’',
        0x93 => '“',
        0x94 => '”',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0x98 => '˜',
        0x99 => '™',
        0x9A => 'š',
        0x9B => '›',
        0x9C => 'œ',
        0x9E => 'ž',
        0x9F => 'Ÿ',
        0x81 | 0x8D | 0x8F | 0x90 | 0x9D => '\u{FFFD}',
        _ => b as char,
    }
}

/// Decide whether a text is hard-wrapped (lines broken at ~70 columns inside paragraphs).
fn is_hard_wrapped(sample: &str) -> bool {
    let mut lines = 0usize;
    let mut short_breaks = 0usize;
    let mut prev_len = 0usize;
    for line in sample.lines() {
        let l = line.trim_end();
        if l.is_empty() {
            prev_len = 0;
            continue;
        }
        lines += 1;
        if prev_len > 0 && (40..=90).contains(&prev_len) && !l.starts_with(char::is_whitespace) {
            short_breaks += 1;
        }
        prev_len = l.chars().count();
    }
    lines > 8 && short_breaks * 2 > lines
}

/// Ingest a text file.
pub fn ingest<R: ReadAt>(file: &R, name: &str, sink: &mut dyn Sink) -> Result<(), DocError> {
    let len = file.len() as usize;
    // Sample the head for encoding and wrapping decisions.
    let head = file.read_range(0, len.min(64 * 1024))?;
    let sample = decode_bytes(&head);
    let wrapped = is_hard_wrapped(&sample);
    let utf16 = head.len() >= 2 && (head[..2] == [0xFF, 0xFE] || head[..2] == [0xFE, 0xFF]);

    let title = first_title(&sample).unwrap_or_else(|| crate::title_from_name(name));
    sink.metadata(&Metadata { title: title.clone(), ..Default::default() })?;

    // Chapters: split on Gutenberg-style "CHAPTER ..." headings, else every ~40 KB of text.
    let mut chapter = 0u16;
    let mut toc: Vec<TocEntry> = Vec::new();
    let mut w = Writer::new();
    let mut para = String::new();
    let mut chapter_chars = 0u32;
    let mut chapter_open = false;

    let flush_para = |para: &mut String,
                      w: &mut Writer,
                      sink: &mut dyn Sink,
                      chapter: &mut u16,
                      toc: &mut Vec<TocEntry>,
                      chapter_open: &mut bool,
                      chapter_chars: &mut u32|
     -> Result<(), DocError> {
        let text = para.trim();
        if text.is_empty() {
            para.clear();
            return Ok(());
        }
        let heading = looks_like_heading(text);
        if heading && *chapter_open && *chapter_chars > 2000 {
            // Close the current chapter and start a new one at this heading.
            sink.chapter_bytes(w.as_bytes())?;
            sink.end_chapter(*chapter_chars)?;
            *w = Writer::new();
            *chapter += 1;
            *chapter_open = false;
            *chapter_chars = 0;
        }
        if !*chapter_open {
            let t = if heading { collapse(text) } else { String::new() };
            sink.begin_chapter(*chapter, if heading { Some(&t) } else { None })?;
            if heading {
                toc.push(TocEntry { title: t.clone(), chapter: *chapter, anchor: None, depth: 0 });
            }
            *chapter_open = true;
        }
        if heading {
            w.para(ParaKind::Heading(2));
            w.text(&collapse(text));
        } else {
            w.para(ParaKind::Body);
            w.text(&collapse(text));
        }
        *chapter_chars += text.chars().count() as u32;
        if w.len() > 48 * 1024 {
            sink.chapter_bytes(w.as_bytes())?;
            *w = Writer::new();
        }
        para.clear();
        Ok(())
    };

    let mut offset = 0usize;
    let mut carry = String::new();
    let mut buf = alloc::vec![0u8; 32 * 1024];
    let mut first = true;
    loop {
        let n = file.read_at(offset as u64, &mut buf)?;
        if n == 0 {
            break;
        }
        offset += n;
        let mut chunk = if utf16 {
            // UTF-16 must be decoded from the start; re-read whole for such (rare) files.
            if first {
                let all = file.read_range(0, len)?;
                offset = len;
                decode_bytes(&all)
            } else {
                String::new()
            }
        } else {
            let mut raw = &buf[..n];
            if first {
                raw = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(raw);
            }
            // Keep incomplete UTF-8 tail for the next chunk.
            let valid_to = match core::str::from_utf8(raw) {
                Ok(_) => raw.len(),
                Err(e) => e.valid_up_to(),
            };
            let (good, rest) = raw.split_at(valid_to);
            let mut s = carry.clone();
            carry.clear();
            match core::str::from_utf8(good) {
                Ok(t) => s.push_str(t),
                Err(_) => s.push_str(&decode_bytes(good)),
            }
            if !rest.is_empty() && rest.len() < 4 {
                carry = String::from_utf8_lossy(rest).into_owned(); // never valid; treat as lossy
                carry.clear();
                // Store raw bytes for the next round instead.
                let _ = rest;
            }
            s
        };
        first = false;
        chunk = chunk.replace("\r\n", "\n").replace('\r', "\n");
        // Split into paragraphs on blank lines; when hard-wrapped, single newlines are spaces.
        let mut i = 0usize;
        let bytes = chunk.as_bytes();
        let mut line_start = 0usize;
        while i <= bytes.len() {
            if i == bytes.len() || bytes[i] == b'\n' {
                let line = &chunk[line_start..i];
                if i == bytes.len() {
                    // Incomplete last line of the chunk: carry it.
                    if !line.is_empty() {
                        if !para.is_empty() && !wrapped {
                            para.push('\n');
                        } else if !para.is_empty() {
                            para.push(' ');
                        }
                        para.push_str(line);
                    }
                    break;
                }
                if line.trim().is_empty() {
                    flush_para(&mut para, &mut w, sink, &mut chapter, &mut toc, &mut chapter_open, &mut chapter_chars)?;
                } else {
                    if !para.is_empty() {
                        para.push(if wrapped { ' ' } else { '\n' });
                    }
                    para.push_str(line);
                    if !wrapped {
                        // Each line is its own paragraph when not hard-wrapped.
                        flush_para(&mut para, &mut w, sink, &mut chapter, &mut toc, &mut chapter_open, &mut chapter_chars)?;
                    }
                }
                line_start = i + 1;
            }
            i += 1;
        }
        sink.progress(offset as u32, len as u32);
    }
    flush_para(&mut para, &mut w, sink, &mut chapter, &mut toc, &mut chapter_open, &mut chapter_chars)?;
    if !chapter_open {
        sink.begin_chapter(0, Some(&title))?;
        w.para(ParaKind::Body);
        w.text(" ");
    }
    sink.chapter_bytes(w.as_bytes())?;
    sink.end_chapter(chapter_chars)?;
    if toc.is_empty() {
        toc.push(TocEntry { title, chapter: 0, anchor: None, depth: 0 });
    }
    sink.toc(&toc)?;
    Ok(())
}

fn collapse(s: &str) -> String {
    crate::html::collapse_ws(s)
}

fn looks_like_heading(text: &str) -> bool {
    let t = text.trim();
    if t.chars().count() > 80 || t.contains('\n') {
        return false;
    }
    let up = t.to_ascii_uppercase();
    up.starts_with("CHAPTER ")
        || up.starts_with("BOOK ")
        || up.starts_with("PART ")
        || up.starts_with("LETTER ")
        || (t.len() > 3 && t == up && t.chars().any(|c| c.is_alphabetic()) && !t.ends_with('.'))
}

fn first_title(sample: &str) -> Option<String> {
    for line in sample.lines().take(40) {
        let l = line.trim();
        if let Some(t) = l.strip_prefix("Title:") {
            return Some(t.trim().into());
        }
        if let Some(t) = l.strip_prefix("The Project Gutenberg eBook of ") {
            return Some(t.split(", by").next().unwrap_or(t).trim().into());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings() {
        assert_eq!(decode_bytes("caf\u{e9}".as_bytes()), "café");
        assert_eq!(decode_bytes(b"\xEF\xBB\xBFhi"), "hi");
        assert_eq!(decode_bytes(b"caf\xE9 \x93q\x94"), "café “q”");
        let mut u16le = alloc::vec![0xFF, 0xFE];
        for c in "héllo".encode_utf16() {
            u16le.extend_from_slice(&c.to_le_bytes());
        }
        assert_eq!(decode_bytes(&u16le), "héllo");
    }

    #[test]
    fn wrapped_detection() {
        let wrapped = "It is a truth universally acknowledged, that a single man in possession of a\ngood fortune, must be in want of a wife. However little known the feelings or\nviews of such a man may be on his first entering a neighbourhood, this truth is\nso well fixed in the minds of the surrounding families, that he is considered\nthe rightful property of some one or other of their daughters. My dear Mr.\nBennet, said his lady to him one day, have you heard that Netherfield Park is\nlet at last? Mr. Bennet replied that he had not. But it is, returned she; for\nMrs. Long has just been here, and she told me all about it. Mr. Bennet made no\nanswer. Do you not want to know who has taken it? cried his wife impatiently.\nYou want to tell me, and I have no objection to hearing it.\n";
        assert!(is_hard_wrapped(wrapped));
        let flowing = "One paragraph that is long enough to wrap in an editor but has no newlines inside it at all, because modern files flow.\n\nAnother paragraph, the same.\n\nThird.\n";
        assert!(!is_hard_wrapped(flowing));
    }
}

//! Markdown via pulldown-cmark, mapped straight onto QTX.

use alloc::string::String;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use quire_fs::ReadAt;
use quire_qtx::{style, ParaKind, Token, Writer};

use crate::{DocError, Metadata, Sink, TocEntry};

/// Convert Markdown text to QTX, returning bytes, char count, first heading and TOC.
pub fn to_qtx(src: &str) -> (alloc::vec::Vec<u8>, u32, Option<String>, alloc::vec::Vec<(String, u8)>) {
    let mut w = Writer::new();
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    let parser = Parser::new_ext(src, opts);
    let mut styleflags = 0u8;
    let mut in_para = false;
    let mut list_stack: alloc::vec::Vec<(bool, u16)> = alloc::vec::Vec::new();
    let mut first_heading: Option<String> = None;
    let mut heading_buf: Option<(u8, String)> = None;
    let mut toc = alloc::vec::Vec::new();
    let mut image_ids = 0u16;
    let mut quote_depth = 0u32;
    let mut code_block = false;

    let start = |w: &mut Writer, k: ParaKind, in_para: &mut bool, styleflags: u8| {
        if *in_para {
            w.push(&Token::End);
        }
        w.para(k);
        if styleflags != 0 {
            w.style(styleflags);
        }
        *in_para = true;
    };

    for ev in parser {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => start(&mut w, if quote_depth > 0 { ParaKind::Quote } else { ParaKind::Body }, &mut in_para, styleflags),
                Tag::Heading { level, .. } => {
                    let l = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        _ => 3,
                    };
                    start(&mut w, ParaKind::Heading(l), &mut in_para, styleflags);
                    heading_buf = Some((l, String::new()));
                }
                Tag::BlockQuote(_) => quote_depth += 1,
                Tag::CodeBlock(_) => {
                    start(&mut w, ParaKind::Code, &mut in_para, styleflags);
                    code_block = true;
                }
                Tag::List(first) => list_stack.push((first.is_some(), first.unwrap_or(1) as u16)),
                Tag::Item => {
                    let level = list_stack.len().saturating_sub(1) as u8;
                    let (ordered, idx) = list_stack.last().copied().unwrap_or((false, 1));
                    start(&mut w, ParaKind::ListItem { ordered, level, index: idx }, &mut in_para, styleflags);
                    if let Some(t) = list_stack.last_mut() {
                        t.1 += 1;
                    }
                }
                Tag::Emphasis => {
                    styleflags |= style::ITALIC;
                    if in_para {
                        w.style(styleflags);
                    }
                }
                Tag::Strong => {
                    styleflags |= style::BOLD;
                    if in_para {
                        w.style(styleflags);
                    }
                }
                Tag::Strikethrough => {
                    styleflags |= style::STRIKE;
                    if in_para {
                        w.style(styleflags);
                    }
                }
                Tag::Link { dest_url, .. } => {
                    if in_para {
                        w.push(&Token::Link(String::from(&*dest_url)));
                    }
                }
                Tag::Image { dest_url, .. } => {
                    if in_para {
                        w.push(&Token::End);
                        in_para = false;
                    }
                    let _ = dest_url;
                    w.push(&Token::Image { id: image_ids, w: 0, h: 0 });
                    image_ids += 1;
                }
                Tag::Table(_) => {}
                Tag::TableHead | Tag::TableRow => start(&mut w, ParaKind::TableRow, &mut in_para, styleflags),
                Tag::TableCell => {}
                Tag::FootnoteDefinition(_) => start(&mut w, ParaKind::Body, &mut in_para, styleflags),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::TableHead | TagEnd::TableRow | TagEnd::FootnoteDefinition => {
                    if let (TagEnd::Heading(_), Some((l, t))) = (&tag, heading_buf.take()) {
                        if first_heading.is_none() {
                            first_heading = Some(t.clone());
                        }
                        toc.push((t, l));
                    }
                    if in_para {
                        w.push(&Token::End);
                        in_para = false;
                    }
                }
                TagEnd::CodeBlock => {
                    code_block = false;
                    if in_para {
                        w.push(&Token::End);
                        in_para = false;
                    }
                }
                TagEnd::BlockQuote(_) => quote_depth = quote_depth.saturating_sub(1),
                TagEnd::List(_) => {
                    list_stack.pop();
                }
                TagEnd::Emphasis => {
                    styleflags &= !style::ITALIC;
                    if in_para {
                        w.style(styleflags);
                    }
                }
                TagEnd::Strong => {
                    styleflags &= !style::BOLD;
                    if in_para {
                        w.style(styleflags);
                    }
                }
                TagEnd::Strikethrough => {
                    styleflags &= !style::STRIKE;
                    if in_para {
                        w.style(styleflags);
                    }
                }
                TagEnd::Link => {
                    if in_para {
                        w.push(&Token::LinkEnd);
                    }
                }
                TagEnd::TableCell => {
                    if in_para {
                        w.text(" · ");
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                if !in_para {
                    start(&mut w, ParaKind::Body, &mut in_para, styleflags);
                }
                if code_block {
                    let mut first = true;
                    for line in t.split('\n') {
                        if !first {
                            w.push(&Token::Break);
                        }
                        first = false;
                        w.text(line);
                    }
                } else {
                    w.text(&t);
                }
                if let Some((_, h)) = heading_buf.as_mut() {
                    h.push_str(&t);
                }
            }
            Event::Code(t) => {
                if !in_para {
                    start(&mut w, ParaKind::Body, &mut in_para, styleflags);
                }
                w.style(styleflags | style::MONO);
                w.text(&t);
                w.style(styleflags);
            }
            Event::SoftBreak => {
                if in_para {
                    w.text(" ");
                }
            }
            Event::HardBreak => {
                if in_para {
                    w.push(&Token::Break);
                }
            }
            Event::Rule => {
                if in_para {
                    w.push(&Token::End);
                    in_para = false;
                }
                w.push(&Token::Rule);
            }
            Event::FootnoteReference(id) => {
                if in_para {
                    w.push(&Token::Footnote(String::from(&*id)));
                }
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                // Strip tags, keep text.
                let (bytes, _, _) = crate::html::to_qtx(&h);
                for t in quire_qtx::Reader::new(&bytes) {
                    if let Token::Text(s) = t {
                        if !in_para {
                            start(&mut w, ParaKind::Body, &mut in_para, styleflags);
                        }
                        w.text(&s);
                    }
                }
            }
            _ => {}
        }
    }
    if in_para {
        w.push(&Token::End);
    }
    let chars = w.char_count();
    (w.finish(), chars, first_heading, toc)
}

/// Ingest a Markdown file as a single chapter.
pub fn ingest<R: ReadAt>(file: &R, name: &str, sink: &mut dyn Sink) -> Result<(), DocError> {
    let len = file.len() as usize;
    if len > 4 * 1024 * 1024 {
        return Err(DocError::TooLarge("markdown over 4 MB"));
    }
    let data = file.read_range(0, len)?;
    let text = crate::txt::decode_bytes(&data);
    let (bytes, chars, heading, toc) = to_qtx(&text);
    let title = heading.unwrap_or_else(|| crate::title_from_name(name));
    sink.metadata(&Metadata { title: title.clone(), ..Default::default() })?;
    sink.begin_chapter(0, Some(&title))?;
    sink.chapter_bytes(&bytes)?;
    sink.end_chapter(chars)?;
    let entries: alloc::vec::Vec<TocEntry> = if toc.is_empty() {
        alloc::vec![TocEntry { title, chapter: 0, anchor: None, depth: 0 }]
    } else {
        toc.into_iter().map(|(t, l)| TocEntry { title: t, chapter: 0, anchor: None, depth: l.saturating_sub(1) }).collect()
    };
    sink.toc(&entries)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_qtx::Reader;

    #[test]
    fn markdown_maps_to_qtx() {
        let md = "# Title\n\nSome *italic* and **bold** with `code`.\n\n- a\n- b\n\n1. one\n\n> quote\n\n```\nlet x = 1;\nlet y = 2;\n```\n\n---\n";
        let (bytes, _, h, toc) = to_qtx(md);
        assert_eq!(h.as_deref(), Some("Title"));
        assert_eq!(toc.len(), 1);
        let toks: alloc::vec::Vec<Token> = Reader::new(&bytes).collect();
        assert!(toks.contains(&Token::Para(ParaKind::Heading(1))));
        assert!(toks.contains(&Token::Style(style::ITALIC)));
        assert!(toks.contains(&Token::Style(style::BOLD)));
        assert!(toks.contains(&Token::Style(style::MONO)));
        assert!(toks.contains(&Token::Para(ParaKind::ListItem { ordered: false, level: 0, index: 1 })));
        assert!(toks.contains(&Token::Para(ParaKind::ListItem { ordered: true, level: 0, index: 1 })));
        assert!(toks.contains(&Token::Para(ParaKind::Quote)));
        assert!(toks.contains(&Token::Para(ParaKind::Code)));
        assert!(toks.contains(&Token::Break));
        assert!(toks.contains(&Token::Rule));
    }
}

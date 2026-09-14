//! Comic archives and single images: every image becomes a page-sized picture block,
//! in natural sort order, grouped into chapters of twenty pages.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_qtx::{Token, Writer};

use crate::image::{Fit, ImageKind};
use crate::zip::Zip;
use crate::{limits, DocError, Metadata, Sink, TocEntry};

/// Pages per chapter.
const PAGES_PER_CHAPTER: usize = 20;
/// Page box for comics (16 px margins on the panel).
const PAGE_W: u32 = 496;
const PAGE_H: u32 = 744;

/// Natural ("numeric-aware") comparison: page2 < page10.
pub fn natural_cmp(a: &str, b: &str) -> core::cmp::Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return core::cmp::Ordering::Equal,
            (None, _) => return core::cmp::Ordering::Less,
            (_, None) => return core::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = 0u64;
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na = na.saturating_mul(10).saturating_add(c as u64 - '0' as u64);
                    ai.next();
                }
                let mut nb = 0u64;
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb = nb.saturating_mul(10).saturating_add(c as u64 - '0' as u64);
                    bi.next();
                }
                if na != nb {
                    return na.cmp(&nb);
                }
            }
            (Some(x), Some(y)) => {
                let (lx, ly) = (x.to_ascii_lowercase(), y.to_ascii_lowercase());
                if lx != ly {
                    return lx.cmp(&ly);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

fn is_image_name(n: &str) -> bool {
    let l = n.to_ascii_lowercase();
    !quire_fs::file_name(&l).starts_with('.') && (l.ends_with(".jpg") || l.ends_with(".jpeg") || l.ends_with(".png") || l.ends_with(".bmp"))
}

/// Ingest a CBZ.
pub fn ingest<R: ReadAt>(file: &R, name: &str, sink: &mut dyn Sink) -> Result<(), DocError> {
    let zip = Zip::open(file)?;
    let mut pages: Vec<crate::zip::Entry> = zip.entries.iter().filter(|e| is_image_name(&e.name) && e.usize_ > 0).cloned().collect();
    pages.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    if pages.is_empty() {
        return Err(DocError::Malformed("no images in archive"));
    }
    let meta = Metadata { title: crate::title_from_name(name), ..Default::default() };
    sink.metadata(&meta)?;
    let total = pages.len();
    let mut toc = Vec::new();
    let mut chapter = 0u16;
    let mut w = Writer::new();
    let mut in_chapter = false;
    for (i, e) in pages.iter().enumerate() {
        if i % PAGES_PER_CHAPTER == 0 {
            if in_chapter {
                sink.chapter_bytes(w.as_bytes())?;
                sink.end_chapter(0)?;
                w = Writer::new();
                chapter += 1;
            }
            let title = alloc::format!("Pages {}–{}", i + 1, (i + PAGES_PER_CHAPTER).min(total));
            sink.begin_chapter(chapter, Some(&title))?;
            toc.push(TocEntry { title, chapter, anchor: None, depth: 0 });
            in_chapter = true;
        }
        let bytes = zip.read(e, 32 * 1024 * 1024)?;
        match crate::image::decode(&bytes, ImageKind::from_hint(&e.name), Fit::inside(PAGE_W, PAGE_H)) {
            Ok(bm) => {
                let id = sink.image(&bm)?;
                w.push(&Token::Image { id, w: bm.w as u16, h: bm.h as u16 });
                if i == 0 {
                    // First page doubles as the cover.
                    if let (Ok(full), Ok(thumb)) = (
                        crate::image::decode(&bytes, ImageKind::from_hint(&e.name), Fit::fill(limits::COVER_W, limits::COVER_H)),
                        crate::image::decode(&bytes, ImageKind::from_hint(&e.name), Fit { fs: false, ..Fit::fill(limits::THUMB_W, limits::THUMB_H) }),
                    ) {
                        sink.cover(&full, &thumb)?;
                    }
                }
            }
            Err(_) => {
                w.push(&Token::Image { id: u16::MAX, w: 0, h: 0 });
            }
        }
        sink.progress(i as u32 + 1, total as u32);
    }
    if in_chapter {
        sink.chapter_bytes(w.as_bytes())?;
        sink.end_chapter(0)?;
    }
    sink.toc(&toc)?;
    Ok(())
}

/// A lone image file becomes a one-page book.
pub fn ingest_single_image<R: ReadAt>(file: &R, name: &str, sink: &mut dyn Sink) -> Result<(), DocError> {
    let len = file.len() as usize;
    if len > 32 * 1024 * 1024 {
        return Err(DocError::TooLarge("image over 32 MB"));
    }
    let bytes = file.read_range(0, len)?;
    let kind = ImageKind::from_hint(name);
    let bm = crate::image::decode(&bytes, kind, Fit::inside(PAGE_W, PAGE_H))?;
    let title = crate::title_from_name(name);
    sink.metadata(&Metadata { title: title.clone(), ..Default::default() })?;
    if let (Ok(full), Ok(thumb)) = (crate::image::decode(&bytes, kind, Fit::fill(limits::COVER_W, limits::COVER_H)), crate::image::decode(&bytes, kind, Fit { fs: false, ..Fit::fill(limits::THUMB_W, limits::THUMB_H) })) {
        sink.cover(&full, &thumb)?;
    }
    sink.begin_chapter(0, Some(&title))?;
    let id = sink.image(&bm)?;
    let mut w = Writer::new();
    w.push(&Token::Image { id, w: bm.w as u16, h: bm.h as u16 });
    sink.chapter_bytes(w.as_bytes())?;
    sink.end_chapter(0)?;
    sink.toc(&[TocEntry { title, chapter: 0, anchor: None, depth: 0 }])?;
    let _ = String::new();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memsink::MemSink;

    #[test]
    fn natural_order() {
        let mut v = alloc::vec!["p10.jpg", "p2.jpg", "P1.jpg", "p1b.jpg"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["P1.jpg", "p1b.jpg", "p2.jpg", "p10.jpg"]);
    }

    #[test]
    fn cbz_of_generated_pages() {
        let mut b = crate::zip::testzip::Builder::new();
        for i in 1..=23 {
            let png = crate::png::tests::encode(300, 400, 0, &|x, y| [((x + y + i * 7) % 256) as u8, 0, 0, 255], 0, false);
            b.add(&alloc::format!("page{i}.png"), &png, true);
        }
        b.add("ComicInfo.xml", b"<x/>", false);
        let bytes = b.finish();
        let mut sink = MemSink::default();
        ingest(&bytes, "My Comic.cbz", &mut sink).expect("ingest");
        assert_eq!(sink.meta.title, "My Comic");
        assert_eq!(sink.chapters.len(), 2);
        assert_eq!(sink.images.len(), 23);
        assert!(sink.cover.is_some());
        assert_eq!(sink.toc[0].title, "Pages 1–20");
        assert_eq!(sink.toc[1].title, "Pages 21–23");
    }
}

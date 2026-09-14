//! Document parsers. Every format ends as QTX chapters plus scaled 1-bit images, written
//! through the ingest sink so the device never holds a whole book in RAM.
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod cbz;
pub mod epub;
pub mod fb2;
pub mod html;
pub mod image;
pub mod inflate;
pub mod jpeg;
pub mod md;
pub mod pdf;
pub mod png;
pub mod qbk;
pub mod txt;
pub mod zip;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Which parser handles a file, by extension and sniffing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    /// EPUB 2/3 (and KEPUB).
    Epub,
    /// Plain text.
    Txt,
    /// Markdown.
    Markdown,
    /// FictionBook 2.
    Fb2,
    /// A single HTML/XHTML file.
    Html,
    /// Comic book ZIP.
    Cbz,
    /// PDF.
    Pdf,
    /// Quire's own pre-laid-out book.
    Qbk,
    /// A lone image (JPEG/PNG/BMP) shown as a one-page book.
    Image,
}

impl Format {
    /// Detect by extension, then by magic bytes.
    pub fn detect(path: &str, head: &[u8]) -> Option<Format> {
        let ext = quire_fs::extension(path);
        let by_ext = match ext.as_str() {
            "epub" | "kepub" => Some(Format::Epub),
            "txt" | "text" | "log" => Some(Format::Txt),
            "md" | "markdown" | "mkd" => Some(Format::Markdown),
            "fb2" => Some(Format::Fb2),
            "html" | "htm" | "xhtml" => Some(Format::Html),
            "cbz" => Some(Format::Cbz),
            "pdf" => Some(Format::Pdf),
            "qbk" => Some(Format::Qbk),
            "jpg" | "jpeg" | "png" | "bmp" => Some(Format::Image),
            _ => None,
        };
        if by_ext.is_some() {
            return by_ext;
        }
        if head.starts_with(b"%PDF") {
            return Some(Format::Pdf);
        }
        if head.starts_with(b"PK\x03\x04") {
            return Some(Format::Epub);
        }
        if head.starts_with(b"QBK1") {
            return Some(Format::Qbk);
        }
        if head.starts_with(b"\xFF\xD8\xFF") || head.starts_with(b"\x89PNG") {
            return Some(Format::Image);
        }
        if head.starts_with(b"<?xml") && head.windows(11).any(|w| w == b"FictionBook") {
            return Some(Format::Fb2);
        }
        if head.starts_with(b"<!DOCTYPE") || head.starts_with(b"<html") || head.starts_with(b"<?xml") {
            return Some(Format::Html);
        }
        if head.iter().all(|&b| b == 9 || b == 10 || b == 13 || (32..127).contains(&b) || b >= 128) {
            return Some(Format::Txt);
        }
        None
    }
}

/// Book metadata gathered at ingest.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// Title.
    pub title: String,
    /// Authors, display order.
    pub authors: Vec<String>,
    /// Language code (BCP-47 prefix).
    pub language: String,
    /// Series name, if any.
    pub series: Option<String>,
    /// Position in the series (× 10, so 1.5 is 15).
    pub series_index: Option<u16>,
    /// Publisher.
    pub publisher: Option<String>,
    /// Year of publication, if known.
    pub year: Option<u16>,
    /// Description or blurb.
    pub description: Option<String>,
    /// Subjects / tags.
    pub subjects: Vec<String>,
    /// A stable identifier from the file (ISBN, URN), if any.
    pub identifier: Option<String>,
}

/// A table-of-contents entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TocEntry {
    /// Display title.
    pub title: String,
    /// Chapter index this entry points into.
    pub chapter: u16,
    /// Anchor within the chapter, if any.
    pub anchor: Option<String>,
    /// Nesting depth from 0.
    pub depth: u8,
}

/// Where ingest writes its output. Implemented by the library over a filesystem, and
/// by an in-memory sink in tests.
pub trait Sink {
    /// Begin chapter `index` (0-based). Returns nothing; the sink accumulates bytes.
    fn begin_chapter(&mut self, index: u16, title: Option<&str>) -> Result<(), DocError>;
    /// Append QTX bytes to the current chapter.
    fn chapter_bytes(&mut self, data: &[u8]) -> Result<(), DocError>;
    /// Finish the current chapter, reporting its character count.
    fn end_chapter(&mut self, chars: u32) -> Result<(), DocError>;
    /// Store a scaled 1-bit image for the current chapter; returns its id.
    fn image(&mut self, bitmap: &quire_gfx::Bitmap) -> Result<u16, DocError>;
    /// Store the cover as a full-page 2-bit-capable bitmap and a thumbnail.
    fn cover(&mut self, full: &quire_gfx::Bitmap, thumb: &quire_gfx::Bitmap) -> Result<(), DocError>;
    /// Metadata, once known.
    fn metadata(&mut self, meta: &Metadata) -> Result<(), DocError>;
    /// Table of contents, once known.
    fn toc(&mut self, toc: &[TocEntry]) -> Result<(), DocError>;
    /// Progress for the UI: (done, total) in arbitrary units.
    fn progress(&mut self, _done: u32, _total: u32) {}
}

/// Parser errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocError {
    /// Storage failure.
    Fs(quire_fs::FsError),
    /// The file is not what the extension claims, or is corrupt.
    Malformed(&'static str),
    /// A feature the device does not support; message for the user.
    Unsupported(&'static str),
    /// Encrypted / DRM-protected content.
    Drm,
    /// Out of memory or over a safety limit.
    TooLarge(&'static str),
}

impl From<quire_fs::FsError> for DocError {
    fn from(e: quire_fs::FsError) -> Self {
        DocError::Fs(e)
    }
}

impl core::fmt::Display for DocError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DocError::Fs(e) => write!(f, "storage: {e}"),
            DocError::Malformed(m) => write!(f, "malformed: {m}"),
            DocError::Unsupported(m) => write!(f, "unsupported: {m}"),
            DocError::Drm => write!(f, "This book is DRM-protected. Remove the DRM on your computer with Calibre, then send it again."),
            DocError::TooLarge(m) => write!(f, "too large: {m}"),
        }
    }
}

/// Ingest a file of a detected format.
pub fn ingest<R: quire_fs::ReadAt>(format: Format, file: &R, name: &str, sink: &mut dyn Sink) -> Result<(), DocError> {
    match format {
        Format::Epub => epub::ingest(file, sink),
        Format::Txt => txt::ingest(file, name, sink),
        Format::Markdown => md::ingest(file, name, sink),
        Format::Fb2 => fb2::ingest(file, sink),
        Format::Html => html::ingest_file(file, name, sink),
        Format::Cbz => cbz::ingest(file, name, sink),
        Format::Pdf => pdf::ingest(file, name, sink),
        Format::Qbk => qbk::ingest(file, sink),
        Format::Image => cbz::ingest_single_image(file, name, sink),
    }
}

/// A title derived from a file name: "the_left-hand.of.darkness.epub" → "The left hand of darkness".
pub fn title_from_name(name: &str) -> String {
    let base = quire_fs::file_name(name);
    let stem = match base.rfind('.') {
        Some(i) if i > 0 => &base[..i],
        _ => base,
    };
    let mut out = String::with_capacity(stem.len());
    let mut first = true;
    for ch in stem.chars() {
        let c = if ch == '_' || ch == '-' { ' ' } else { ch };
        if first && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            first = false;
        } else {
            out.push(c);
        }
    }
    out.trim().into()
}

/// Limits that keep ingest inside the device's memory budget.
pub mod limits {
    /// Largest single decompressed text chunk we hold at once.
    pub const TEXT_CHUNK: usize = 16 * 1024;
    /// Largest image we will decode on the device (pixels).
    pub const IMAGE_PIXELS: u32 = 4096 * 4096;
    /// Target width for inline images (the text block at 16 px margins).
    pub const IMAGE_W: u32 = 480;
    /// Target height for inline images.
    pub const IMAGE_H: u32 = 640;
    /// Cover thumbnail size.
    pub const THUMB_W: u32 = 152;
    /// Cover thumbnail height.
    pub const THUMB_H: u32 = 228;
    /// Full-page cover for sleep screens.
    pub const COVER_W: u32 = 528;
    /// Full-page cover height.
    pub const COVER_H: u32 = 792;
}

/// An in-memory sink for tests and the simulator.
#[cfg(any(test, feature = "std"))]
pub mod memsink {
    use super::*;
    use quire_gfx::Bitmap;

    /// Collected output of an ingest.
    #[derive(Default, Debug)]
    pub struct MemSink {
        /// Chapters as (title, qtx bytes, chars).
        pub chapters: Vec<(Option<String>, Vec<u8>, u32)>,
        /// Images by id.
        pub images: Vec<Bitmap>,
        /// Cover (full, thumb).
        pub cover: Option<(Bitmap, Bitmap)>,
        /// Metadata.
        pub meta: Metadata,
        /// TOC.
        pub toc: Vec<TocEntry>,
        cur: Option<(Option<String>, Vec<u8>)>,
    }

    impl Sink for MemSink {
        fn begin_chapter(&mut self, _index: u16, title: Option<&str>) -> Result<(), DocError> {
            self.cur = Some((title.map(String::from), Vec::new()));
            Ok(())
        }
        fn chapter_bytes(&mut self, data: &[u8]) -> Result<(), DocError> {
            if let Some((_, v)) = self.cur.as_mut() {
                v.extend_from_slice(data);
            }
            Ok(())
        }
        fn end_chapter(&mut self, chars: u32) -> Result<(), DocError> {
            if let Some((t, v)) = self.cur.take() {
                self.chapters.push((t, v, chars));
            }
            Ok(())
        }
        fn image(&mut self, bitmap: &Bitmap) -> Result<u16, DocError> {
            self.images.push(bitmap.clone());
            Ok((self.images.len() - 1) as u16)
        }
        fn cover(&mut self, full: &Bitmap, thumb: &Bitmap) -> Result<(), DocError> {
            self.cover = Some((full.clone(), thumb.clone()));
            Ok(())
        }
        fn metadata(&mut self, meta: &Metadata) -> Result<(), DocError> {
            self.meta = meta.clone();
            Ok(())
        }
        fn toc(&mut self, toc: &[TocEntry]) -> Result<(), DocError> {
            self.toc = toc.to_vec();
            Ok(())
        }
    }

    impl MemSink {
        /// All chapter text concatenated.
        pub fn all_text(&self) -> String {
            let mut s = String::new();
            for (_, b, _) in &self.chapters {
                s.push_str(&quire_qtx::plain_text(b));
            }
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memsink::MemSink;

    #[test]
    fn detects_formats() {
        assert_eq!(Format::detect("a/b.EPUB", b""), Some(Format::Epub));
        assert_eq!(Format::detect("x", b"%PDF-1.4"), Some(Format::Pdf));
        assert_eq!(Format::detect("x", b"PK\x03\x04junk"), Some(Format::Epub));
        assert_eq!(Format::detect("x", b"plain words here"), Some(Format::Txt));
        assert_eq!(Format::detect("x", b"\x00\x01\x02"), None);
        assert_eq!(title_from_name("/books/the_left-hand.of.darkness.epub"), "The left hand.of.darkness");
    }

    #[test]
    fn epub3_moby_dick_ingests_completely() {
        let data = include_bytes!("../fixtures/moby-dick.epub");
        let mut sink = MemSink::default();
        ingest(Format::Epub, &&data[..], "moby-dick.epub", &mut sink).expect("ingest");
        assert_eq!(sink.meta.title, "Moby-Dick");
        assert_eq!(sink.meta.authors, ["Herman Melville"]);
        assert_eq!(sink.meta.language, "en-US");
        assert!(sink.chapters.len() > 130, "spine chapters: {}", sink.chapters.len());
        let text = sink.all_text();
        assert!(text.contains("Call me Ishmael"), "chapter 1 text present");
        assert!(text.contains("Loomings"), "chapter heading present");
        assert!(sink.toc.len() > 100, "toc entries: {}", sink.toc.len());
        assert!(sink.toc.iter().any(|t| t.title.contains("Loomings")), "toc has Loomings");
        // Chapter char counts add up to something book-sized.
        let chars: u32 = sink.chapters.iter().map(|c| c.2).sum();
        assert!(chars > 1_000_000, "chars {chars}");
    }

    #[test]
    fn epub3_wasteland_and_childrens_literature() {
        for (bytes, title) in [
            (&include_bytes!("../fixtures/wasteland.epub")[..], "The Waste Land"),
            (&include_bytes!("../fixtures/childrens-literature.epub")[..], "Children's Literature"),
        ] {
            let mut sink = MemSink::default();
            ingest(Format::Epub, &bytes, "x.epub", &mut sink).expect("ingest");
            assert_eq!(sink.meta.title, title);
            assert!(!sink.chapters.is_empty());
            assert!(!sink.toc.is_empty());
            let text = sink.all_text();
            assert!(text.len() > 5000, "{title}: {} chars", text.len());
        }
    }

    #[test]
    fn txt_gutenberg_style_reflows_and_chapters() {
        let mut body = String::from("Title: A Test Book\n\nCHAPTER I\n\n");
        for _ in 0..40 {
            body.push_str("It is a truth universally acknowledged, that a single man in possession of a\ngood fortune, must be in want of a wife. However little known the feelings or\nviews of such a man may be on his first entering a neighbourhood, this truth is\nso well fixed in the minds of the surrounding families.\n\n");
        }
        body.push_str("CHAPTER II\n\nSecond chapter text here.\n");
        let mut sink = MemSink::default();
        ingest(Format::Txt, &body.as_bytes(), "book.txt", &mut sink).unwrap();
        assert_eq!(sink.meta.title, "A Test Book");
        assert_eq!(sink.chapters.len(), 2, "two chapters: {:?}", sink.chapters.iter().map(|c| &c.0).collect::<Vec<_>>());
        let text = sink.all_text();
        assert!(text.contains("possession of a good fortune"), "hard wraps joined");
        assert!(sink.toc.iter().any(|t| t.title == "CHAPTER II"));
    }
}

//! The per-book cache under `/.quire/books/<id>/`:
//!
//! ```text
//! meta.bin        postcard(Metadata)
//! toc.bin         postcard(Vec<TocEntry>)
//! sections.bin    postcard(Vec<SectionInfo>)   chapter files, in reading order
//! ch/NNNN.qtx     token stream of one section (a chapter, or part of a long one)
//! img/NNNN.pbm    pre-scaled 1-bit images (P4)
//! cover.pbm       full-page cover, thumb.pbm the shelf thumbnail
//! pages/HHHHHHHH/NNNN.idx   page starts per typography profile (see `pages`)
//! marks.bin       bookmarks, highlights, notes
//! ```
//!
//! [`CacheSink`] is the [`quire_doc::Sink`] that fills it during ingest. Long chapters
//! are split into sections of at most [`SECTION_BYTES`] at paragraph boundaries so the
//! layout engine never holds more than that in RAM.

use alloc::string::String;
use alloc::vec::Vec;
use quire_doc::{DocError, Metadata, Sink, TocEntry};
use quire_fs::{Fs, WriteFile};
use quire_gfx::Bitmap;
use quire_qtx::{Reader, Token};
use serde::{Deserialize, Serialize};

use crate::{LibError, LibResult};

/// Largest section file.
pub const SECTION_BYTES: usize = 48 * 1024;
/// Below this, a split is not worth it.
const MIN_SPLIT: usize = 8 * 1024;
/// Images per book.
const MAX_IMAGES: u16 = 2000;
/// Sections per book.
const MAX_SECTIONS: usize = 4000;

/// One section (chapter file).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionInfo {
    /// The ingest chapter this section came from.
    pub source: u16,
    /// Part number within that chapter (0 for the first).
    pub part: u8,
    /// Chapter title, if any.
    pub title: Option<String>,
    /// Characters in this section.
    pub chars: u32,
    /// Bytes of QTX.
    pub bytes: u32,
}

/// Summary of a finished ingest.
#[derive(Clone, Debug, Default)]
pub struct CacheSummary {
    /// Sections written.
    pub sections: Vec<SectionInfo>,
    /// Metadata as reported by the parser.
    pub meta: Metadata,
    /// Table of contents (chapter indexes are ingest chapters).
    pub toc: Vec<TocEntry>,
    /// Whether a cover was stored.
    pub has_cover: bool,
    /// Images stored.
    pub images: u16,
}

impl CacheSummary {
    /// Total characters.
    pub fn chars(&self) -> u32 {
        self.sections.iter().map(|s| s.chars).sum()
    }
}

/// The ingest sink writing a book cache.
pub struct CacheSink<'a, F: Fs> {
    fs: &'a F,
    dir: String,
    sections: Vec<SectionInfo>,
    cur: Option<(u16, Option<String>, Vec<u8>, u8)>,
    images: u16,
    meta: Metadata,
    toc: Vec<TocEntry>,
    has_cover: bool,
    /// Progress callback data: (done, total) from the parser.
    pub progress: (u32, u32),
}

impl<'a, F: Fs> CacheSink<'a, F> {
    /// Start a fresh cache in `dir` (any previous contents are removed).
    pub fn new(fs: &'a F, dir: &str) -> LibResult<Self> {
        if fs.exists(dir) {
            clear_dir(fs, dir, 0)?;
        }
        fs.mkdir_all(&quire_fs::join(dir, "ch"))?;
        fs.mkdir_all(&quire_fs::join(dir, "img"))?;
        Ok(CacheSink {
            fs,
            dir: dir.into(),
            sections: Vec::new(),
            cur: None,
            images: 0,
            meta: Metadata::default(),
            toc: Vec::new(),
            has_cover: false,
            progress: (0, 0),
        })
    }

    fn write_section(&mut self, source: u16, title: Option<String>, part: u8, data: &[u8]) -> Result<(), DocError> {
        if self.sections.len() >= MAX_SECTIONS {
            return Err(DocError::TooLarge("sections"));
        }
        let n = self.sections.len();
        let path = section_path(&self.dir, n);
        let tmp = alloc::format!("{path}.tmp");
        {
            let mut w = self.fs.create(&tmp)?;
            w.write_all(data)?;
            w.flush()?;
        }
        self.fs.rename(&tmp, &path)?;
        self.sections.push(SectionInfo { source, part, title, chars: quire_qtx::char_count(data), bytes: data.len() as u32 });
        Ok(())
    }

    /// Split the current buffer at a paragraph boundary when it is large.
    fn maybe_split(&mut self) -> Result<(), DocError> {
        loop {
            let Some((source, title, buf, part)) = self.cur.as_mut() else { return Ok(()) };
            if buf.len() <= SECTION_BYTES {
                return Ok(());
            }
            // Find the last Para token boundary at or below the limit.
            let mut r = Reader::new(buf);
            let mut cut = 0usize;
            loop {
                let off = r.offset() as usize;
                if off > SECTION_BYTES {
                    break;
                }
                match r.next() {
                    Some(Token::Para(_)) | Some(Token::ChapterTitle { .. }) => {
                        if off >= MIN_SPLIT {
                            cut = off;
                        }
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            if cut == 0 {
                // One enormous paragraph: cut at any token boundary past the minimum.
                let mut r = Reader::new(buf);
                loop {
                    let off = r.offset() as usize;
                    if off > SECTION_BYTES || r.next().is_none() {
                        break;
                    }
                    if off >= MIN_SPLIT {
                        cut = off;
                    }
                }
            }
            if cut == 0 {
                return Ok(());
            }
            let head: Vec<u8> = buf[..cut].to_vec();
            let rest: Vec<u8> = buf[cut..].to_vec();
            *buf = rest;
            let (source, title, part_no) = (*source, title.clone(), *part);
            *part = part.saturating_add(1);
            self.write_section(source, title, part_no, &head)?;
        }
    }

    /// Finish: write the section table, metadata and TOC.
    pub fn finish(mut self) -> LibResult<CacheSummary> {
        if let Some((source, title, buf, part)) = self.cur.take() {
            if !buf.is_empty() || part == 0 {
                self.write_section(source, title, part, &buf)?;
            }
        }
        let enc = |v: &[u8]| -> Vec<u8> { v.to_vec() };
        let sections = postcard::to_allocvec(&self.sections).map_err(|_| LibError::Corrupt("sections encode"))?;
        self.fs.write_atomic(&quire_fs::join(&self.dir, "sections.bin"), &enc(&sections))?;
        let meta = postcard::to_allocvec(&self.meta).map_err(|_| LibError::Corrupt("meta encode"))?;
        self.fs.write_atomic(&quire_fs::join(&self.dir, "meta.bin"), &meta)?;
        let toc = postcard::to_allocvec(&self.toc).map_err(|_| LibError::Corrupt("toc encode"))?;
        self.fs.write_atomic(&quire_fs::join(&self.dir, "toc.bin"), &toc)?;
        Ok(CacheSummary { sections: self.sections, meta: self.meta, toc: self.toc, has_cover: self.has_cover, images: self.images })
    }
}

impl<F: Fs> Sink for CacheSink<'_, F> {
    fn begin_chapter(&mut self, index: u16, title: Option<&str>) -> Result<(), DocError> {
        if let Some((source, t, buf, part)) = self.cur.take() {
            if !buf.is_empty() || part == 0 {
                self.write_section(source, t, part, &buf)?;
            }
        }
        self.cur = Some((index, title.map(String::from), Vec::new(), 0));
        Ok(())
    }
    fn chapter_bytes(&mut self, data: &[u8]) -> Result<(), DocError> {
        if let Some((_, _, buf, _)) = self.cur.as_mut() {
            buf.extend_from_slice(data);
        }
        self.maybe_split()
    }
    fn end_chapter(&mut self, _chars: u32) -> Result<(), DocError> {
        if let Some((source, t, buf, part)) = self.cur.take() {
            if !buf.is_empty() || part == 0 {
                self.write_section(source, t, part, &buf)?;
            }
        }
        Ok(())
    }
    fn image(&mut self, bitmap: &Bitmap) -> Result<u16, DocError> {
        if self.images >= MAX_IMAGES {
            return Err(DocError::TooLarge("images"));
        }
        let id = self.images;
        let path = image_path(&self.dir, id);
        write_pbm(self.fs, &path, bitmap)?;
        self.images += 1;
        Ok(id)
    }
    fn cover(&mut self, full: &Bitmap, thumb: &Bitmap) -> Result<(), DocError> {
        write_pbm(self.fs, &quire_fs::join(&self.dir, "cover.pbm"), full)?;
        write_pbm(self.fs, &quire_fs::join(&self.dir, "thumb.pbm"), thumb)?;
        self.has_cover = true;
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
    fn progress(&mut self, done: u32, total: u32) {
        self.progress = (done, total);
    }
}

/// Path of section `n`.
pub fn section_path(dir: &str, n: usize) -> String {
    alloc::format!("{dir}/ch/{n:04}.qtx")
}
/// Path of image `id`.
pub fn image_path(dir: &str, id: u16) -> String {
    alloc::format!("{dir}/img/{id:04}.pbm")
}

/// Write a bitmap as binary PBM (P4, 1 = ink).
pub fn write_pbm<F: Fs>(fs: &F, path: &str, bm: &Bitmap) -> Result<(), DocError> {
    let tmp = alloc::format!("{path}.tmp");
    {
        let mut w = fs.create(&tmp)?;
        w.write_all(alloc::format!("P4\n{} {}\n", bm.w, bm.h).as_bytes())?;
        w.write_all(&bm.bits)?;
        w.flush()?;
    }
    fs.rename(&tmp, path)?;
    Ok(())
}

/// Read a P4 PBM into a bitmap.
pub fn read_pbm(bytes: &[u8]) -> Option<Bitmap> {
    if bytes.len() < 4 || &bytes[..2] != b"P4" {
        return None;
    }
    // Header: "P4" ws width ws height single-ws data.
    let mut i = 2;
    let mut nums = [0u32; 2];
    for n in nums.iter_mut() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        *n = core::str::from_utf8(&bytes[start..i]).ok()?.parse().ok()?;
    }
    i += 1;
    let (w, h) = (nums[0], nums[1]);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let stride = (w as usize).div_ceil(8);
    let need = stride * h as usize;
    if bytes.len() < i + need {
        return None;
    }
    Some(Bitmap { w, h, bits: bytes[i..i + need].to_vec() })
}

/// Load a PBM file.
pub fn load_pbm<F: Fs>(fs: &F, path: &str) -> Option<Bitmap> {
    let bytes = fs.read_to_vec(path).ok()?;
    read_pbm(&bytes)
}

/// Remove a directory tree (bounded depth).
pub fn clear_dir<F: Fs>(fs: &F, dir: &str, depth: u32) -> LibResult<()> {
    if depth > 6 {
        return Ok(());
    }
    let entries = fs.read_dir(dir)?;
    for e in entries {
        let p = quire_fs::join(dir, &e.name);
        if e.is_dir {
            clear_dir(fs, &p, depth + 1)?;
        }
        let _ = fs.remove(&p);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbm_round_trip() {
        let mut bm = Bitmap::new(13, 3);
        bm.set(0, 0, true);
        bm.set(12, 2, true);
        let mut bytes = b"P4\n13 3\n".to_vec();
        bytes.extend_from_slice(&bm.bits);
        let back = read_pbm(&bytes).unwrap();
        assert_eq!(back, bm);
        assert!(read_pbm(b"P4\n0 0\n").is_none());
    }
}

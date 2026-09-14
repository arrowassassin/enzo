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
//! anchors.bin     postcard(Vec<AnchorRec>)  where every `Anchor` token sits (TOC, notes, links)
//! ```
//!
//! [`CacheSink`] is the [`quire_doc::Sink`] that fills it during ingest. Long chapters
//! are split into sections of at most [`SECTION_BYTES`] at paragraph boundaries so the
//! layout engine never holds more than that in RAM.

use alloc::string::String;
use alloc::vec::Vec;
use quire_doc::{DocError, Metadata, Sink, TocEntry};
use quire_fs::{Fs, ReadAt, WriteFile};
use quire_gfx::{Bitmap, BlitMode, Frame};
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
/// Anchors recorded per book.
#[cfg(target_os = "none")]
const MAX_ANCHORS: usize = 600;
/// Anchors recorded per book.
#[cfg(not(target_os = "none"))]
const MAX_ANCHORS: usize = 4000;

/// Where an `Anchor` token sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorRec {
    /// FNV-1a hash of the anchor id.
    pub hash: u32,
    /// Section holding it.
    pub section: u16,
    /// Byte offset of the token in the section.
    pub offset: u32,
    /// Characters before it in the section.
    pub chars: u32,
}

/// Hash used for anchor ids.
pub fn anchor_hash(id: &str) -> u32 {
    let h = crate::id::fnv1a(0xcbf29ce484222325, id.as_bytes());
    (h ^ (h >> 32)) as u32
}

/// One section (chapter file).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionInfo {
    /// The ingest chapter this section came from.
    pub source: u16,
    /// Part number within that chapter (0 for the first).
    pub part: u8,
    /// Chapter title (only on the first part of a chapter; parts inherit it).
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
    /// Anchors recorded.
    pub anchors: Vec<AnchorRec>,
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
    anchors: Vec<AnchorRec>,
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
            anchors: Vec::new(),
            progress: (0, 0),
        })
    }

    /// Write one section file (directly: the whole cache is discarded on a failed ingest, so
    /// per-file atomicity would only cost FAT operations), recording its anchors and chars.
    fn write_section(&mut self, source: u16, title: Option<String>, part: u8, data: &[u8]) -> Result<(), DocError> {
        if self.sections.len() >= MAX_SECTIONS {
            return Err(DocError::TooLarge("sections"));
        }
        let n = self.sections.len();
        let path = section_path(&self.dir, n);
        {
            let mut w = self.fs.create(&path)?;
            w.write_all(data)?;
            w.flush()?;
        }
        // One pass over the tokens: character count and anchor positions.
        let mut chars = 0u32;
        let mut r = Reader::new(data);
        while let Some(tag) = r.peek_tag() {
            let off = r.offset();
            match tag {
                quire_qtx::Tag::Text => {
                    if let Some(b) = r.text_bytes() {
                        chars += b.iter().filter(|x| (**x & 0xC0) != 0x80).count() as u32;
                    }
                }
                quire_qtx::Tag::Anchor => {
                    if let Some((_, id)) = r.string_bytes() {
                        if self.anchors.len() < MAX_ANCHORS {
                            if let Ok(id) = core::str::from_utf8(id) {
                                self.anchors.push(AnchorRec { hash: anchor_hash(id), section: n as u16, offset: off, chars });
                            }
                        }
                    }
                }
                _ => {
                    if r.skip_token().is_none() {
                        break;
                    }
                }
            }
        }
        let title = if part == 0 { title } else { None };
        self.sections.push(SectionInfo { source, part, title, chars, bytes: data.len() as u32 });
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
            let (source, title, part_no) = (*source, title.clone(), *part);
            *part = part.saturating_add(1);
            // Take the buffer out so the section can be written without copying its head.
            let mut whole = core::mem::take(buf);
            self.write_section(source, title, part_no, &whole[..cut])?;
            whole.drain(..cut);
            if let Some((_, _, b, _)) = self.cur.as_mut() {
                *b = whole;
            }
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
        self.anchors.sort_by_key(|a| (a.hash, a.section, a.offset));
        let anchors = postcard::to_allocvec(&self.anchors).map_err(|_| LibError::Corrupt("anchors encode"))?;
        self.fs.write_atomic(&quire_fs::join(&self.dir, "anchors.bin"), &anchors)?;
        Ok(CacheSummary {
            sections: self.sections,
            meta: self.meta,
            toc: self.toc,
            has_cover: self.has_cover,
            images: self.images,
            anchors: self.anchors,
        })
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
    let mut w = fs.create(path)?;
    w.write_all(alloc::format!("P4\n{} {}\n", bm.w, bm.h).as_bytes())?;
    w.write_all(&bm.bits)?;
    w.flush()?;
    Ok(())
}

/// Load a book's shelf thumbnail without opening the book.
pub fn load_thumb<F: Fs>(fs: &F, id: crate::BookId) -> Option<Bitmap> {
    load_pbm(fs, &quire_fs::join(&crate::book_dir(id), "thumb.pbm"))
}

/// Load a book's full-page cover without opening the book.
pub fn load_cover<F: Fs>(fs: &F, id: crate::BookId) -> Option<Bitmap> {
    load_pbm(fs, &quire_fs::join(&crate::book_dir(id), "cover.pbm"))
}

/// Parse a P4 PBM header: (width, height, offset of the packed rows).
pub fn pbm_header(bytes: &[u8]) -> Option<(u32, u32, usize)> {
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
    Some((w, h, i))
}

/// Read a P4 PBM into a bitmap (copies the rows out of the slice).
pub fn read_pbm(bytes: &[u8]) -> Option<Bitmap> {
    let (w, h, i) = pbm_header(bytes)?;
    let need = (w as usize).div_ceil(8) * h as usize;
    if bytes.len() < i + need {
        return None;
    }
    Some(Bitmap { w, h, bits: bytes[i..i + need].to_vec() })
}

/// Turn a whole PBM file into a bitmap in place: the header is drained off the front of
/// the vector and the rows stay where they are, so a full-page image costs one buffer.
pub fn pbm_from_vec(mut bytes: Vec<u8>) -> Option<Bitmap> {
    let (w, h, i) = pbm_header(&bytes)?;
    let need = (w as usize).div_ceil(8) * h as usize;
    if bytes.len() < i + need {
        return None;
    }
    bytes.truncate(i + need);
    bytes.drain(..i);
    Some(Bitmap { w, h, bits: bytes })
}

/// Load a PBM file.
pub fn load_pbm<F: Fs>(fs: &F, path: &str) -> Option<Bitmap> {
    let bytes = fs.read_to_vec(path).ok()?;
    pbm_from_vec(bytes)
}

/// Stream a PBM file straight into a frame, a sector at a time, with no heap copy of the
/// image: `place` receives the image size and returns where its top-left corner goes.
/// Rows outside the frame are skipped without reading. Returns the image size.
pub fn load_pbm_into<F: Fs>(
    fs: &F,
    path: &str,
    f: &mut Frame,
    place: impl FnOnce(u32, u32) -> (i32, i32),
    mode: BlitMode,
) -> Option<(u32, u32)> {
    let file = fs.open(path).ok()?;
    let mut chunk = Chunked { file: &file, buf: [0; 512], start: 0, len: 0 };
    chunk.refill(0)?;
    let (w, h, data) = pbm_header(&chunk.buf[..chunk.len])?;
    let stride = (w as usize).div_ceil(8);
    let (x, y) = place(w, h);
    let mut row = alloc::vec![0u8; stride];
    for r in 0..h {
        let dy = y + r as i32;
        if dy < 0 || dy >= f.height() as i32 {
            continue;
        }
        let at = data as u64 + r as u64 * stride as u64;
        if !chunk.read(at, &mut row) {
            break;
        }
        f.blit_row(x, dy, &row, w, mode);
    }
    Some((w, h))
}

/// Stream a book's full-page cover into a frame (see [`load_pbm_into`]).
pub fn load_cover_into<F: Fs>(
    fs: &F,
    id: crate::BookId,
    f: &mut Frame,
    place: impl FnOnce(u32, u32) -> (i32, i32),
    mode: BlitMode,
) -> Option<(u32, u32)> {
    load_pbm_into(fs, &quire_fs::join(&crate::book_dir(id), "cover.pbm"), f, place, mode)
}

/// Sector-sized sequential reads over a `ReadAt`, aligned so each refill is one sector.
struct Chunked<'a, R: ReadAt> {
    file: &'a R,
    buf: [u8; 512],
    start: u64,
    len: usize,
}

impl<R: ReadAt> Chunked<'_, R> {
    fn refill(&mut self, at: u64) -> Option<()> {
        self.start = at - at % 512;
        self.len = self.file.read_at(self.start, &mut self.buf).ok()?;
        (self.len > 0).then_some(())
    }
    /// Fill `out` from file offset `at`; false at end of file.
    fn read(&mut self, mut at: u64, out: &mut [u8]) -> bool {
        let mut done = 0;
        while done < out.len() {
            if at < self.start || at >= self.start + self.len as u64 {
                if self.refill(at).is_none() {
                    return false;
                }
                if at >= self.start + self.len as u64 {
                    return false;
                }
            }
            let i = (at - self.start) as usize;
            let n = (self.len - i).min(out.len() - done);
            out[done..done + n].copy_from_slice(&self.buf[i..i + n]);
            done += n;
            at += n as u64;
        }
        true
    }
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
        assert_eq!(pbm_from_vec(bytes.clone()).unwrap(), bm);
        assert!(read_pbm(b"P4\n0 0\n").is_none());
        assert!(pbm_from_vec(b"P4\n13 3\n".to_vec()).is_none());
    }

    /// Streaming a PBM into a frame paints the same pixels as blitting the decoded bitmap,
    /// including rows that fall outside the frame and an unaligned x.
    #[test]
    fn stream_into_frame_matches_blit() {
        use quire_fs::{DirEntry, FsError, FsResult, WriteFile};
        struct OneFile(Vec<u8>);
        struct NoWrite;
        impl WriteFile for NoWrite {
            fn write_all(&mut self, _: &[u8]) -> FsResult<()> {
                Err(FsError::Io(String::from("read only")))
            }
            fn flush(&mut self) -> FsResult<()> {
                Ok(())
            }
        }
        impl Fs for OneFile {
            type File = Vec<u8>;
            type Writer = NoWrite;
            fn open(&self, path: &str) -> FsResult<Vec<u8>> {
                (path == "/a.pbm").then(|| self.0.clone()).ok_or(FsError::NotFound)
            }
            fn create(&self, _: &str) -> FsResult<NoWrite> {
                Ok(NoWrite)
            }
            fn append(&self, _: &str) -> FsResult<NoWrite> {
                Ok(NoWrite)
            }
            fn exists(&self, path: &str) -> bool {
                path == "/a.pbm"
            }
            fn read_dir(&self, _: &str) -> FsResult<Vec<DirEntry>> {
                Ok(Vec::new())
            }
            fn mkdir_all(&self, _: &str) -> FsResult<()> {
                Ok(())
            }
            fn remove(&self, _: &str) -> FsResult<()> {
                Ok(())
            }
            fn rename(&self, _: &str, _: &str) -> FsResult<()> {
                Ok(())
            }
            fn free_bytes(&self) -> Option<u64> {
                None
            }
        }
        let mut bm = Bitmap::new(203, 700);
        let mut x = 12345u32;
        for b in bm.bits.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = (x >> 9) as u8;
        }
        let mut bytes = b"P4\n# a comment\n203 700\n".to_vec();
        bytes.extend_from_slice(&bm.bits);
        let fs = OneFile(bytes);
        let mut a = Frame::new(240, 300);
        let mut b = Frame::new(240, 300);
        let got = load_pbm_into(&fs, "/a.pbm", &mut a, |w, h| ((240 - w as i32) / 2 + 1, (300 - h as i32) / 2), BlitMode::Or);
        assert_eq!(got, Some((203, 700)));
        b.blit((240 - 203) / 2 + 1, (300 - 700) / 2, bm.as_ref(), BlitMode::Or);
        assert_eq!(a, b);
        assert!(a.ink_count() > 1000);
    }
}

//! QBK — Quire's own book container, produced by the converter for formats the device
//! does not parse (PDF layouts, DjVu, DOCX …) and readable by anything that reads QTX.
//!
//! ```text
//! "QBK1"  u32 section_count
//! sections: [4-byte tag][u32 len][payload]
//!   META  postcard(Metadata)
//!   TOC   postcard(Vec<TocEntry>)
//!   CHAP  u16 index, u32 chars, u16 title_len, title bytes, then QTX bytes
//!   IMG   u16 id, u16 w, u16 h, packed 1-bit rows
//!   COVR  u16 w, u16 h, bits, u16 tw, u16 th, thumb bits
//! ```
//! Chapters reference images by id within the whole book; the reader remaps them to the
//! sink's ids as it goes.
//!
//! Sections may appear in any order. Ingest makes two passes over the file: the first
//! reads only the small sections (META, TOC, IMG, COVR — images go straight to the sink,
//! their bits read with one `read_exact_at` into the bitmap) and notes where each CHAP
//! lives; the second streams each chapter through a `Reader` → `Writer` copy that remaps
//! image ids, handing the sink about 4 KB at a time. No chapter is ever held whole.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_gfx::Bitmap;
use quire_qtx::{Reader, Token, Writer};

use crate::{DocError, Metadata, Sink, TocEntry};

/// Magic.
pub const MAGIC: &[u8; 4] = b"QBK1";

/// Largest section read whole (META, TOC, IMG, COVR), and the largest single QTX token
/// inside a streamed CHAP.
#[cfg(target_os = "none")]
const SECTION_LIMIT: usize = 256 * 1024;
/// Largest section read whole (META, TOC, IMG, COVR), and the largest single QTX token
/// inside a streamed CHAP.
#[cfg(not(target_os = "none"))]
const SECTION_LIMIT: usize = 64 * 1024 * 1024;
/// Streamed chapters are handed to the sink in pieces of about this size.
const FLUSH_BYTES: usize = 4096;
/// Bytes of a CHAP read per top-up.
const READ_CHUNK: usize = 4096;

/// Build a QBK file in memory (used by the converter and by tests).
#[derive(Default)]
pub struct Builder {
    sections: Vec<(&'static [u8; 4], Vec<u8>)>,
}

impl Builder {
    /// Empty book.
    pub fn new() -> Self {
        Self::default()
    }
    /// Metadata section.
    pub fn metadata(&mut self, m: &Metadata) -> &mut Self {
        self.sections.push((b"META", postcard::to_allocvec(m).unwrap_or_default()));
        self
    }
    /// TOC section.
    pub fn toc(&mut self, t: &[TocEntry]) -> &mut Self {
        self.sections.push((b"TOC ", postcard::to_allocvec(&t.to_vec()).unwrap_or_default()));
        self
    }
    /// A chapter of QTX.
    pub fn chapter(&mut self, index: u16, title: Option<&str>, chars: u32, qtx: &[u8]) -> &mut Self {
        let mut p = Vec::with_capacity(qtx.len() + 16);
        p.extend_from_slice(&index.to_le_bytes());
        p.extend_from_slice(&chars.to_le_bytes());
        let t = title.unwrap_or("").as_bytes();
        p.extend_from_slice(&(t.len() as u16).to_le_bytes());
        p.extend_from_slice(t);
        p.extend_from_slice(qtx);
        self.sections.push((b"CHAP", p));
        self
    }
    /// An image.
    pub fn image(&mut self, id: u16, bm: &Bitmap) -> &mut Self {
        let mut p = Vec::with_capacity(bm.bits.len() + 6);
        p.extend_from_slice(&id.to_le_bytes());
        p.extend_from_slice(&(bm.w as u16).to_le_bytes());
        p.extend_from_slice(&(bm.h as u16).to_le_bytes());
        p.extend_from_slice(&bm.bits);
        self.sections.push((b"IMG ", p));
        self
    }
    /// The cover.
    pub fn cover(&mut self, full: &Bitmap, thumb: &Bitmap) -> &mut Self {
        let mut p = Vec::new();
        for bm in [full, thumb] {
            p.extend_from_slice(&(bm.w as u16).to_le_bytes());
            p.extend_from_slice(&(bm.h as u16).to_le_bytes());
            p.extend_from_slice(&bm.bits);
        }
        self.sections.push((b"COVR", p));
        self
    }
    /// Serialise.
    pub fn finish(self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.sections.len() as u32).to_le_bytes());
        for (tag, payload) in self.sections {
            out.extend_from_slice(tag);
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&payload);
        }
        out
    }
}

/// Read a `u16 w, u16 h, bits` bitmap at `pos`, bounded by `end`; the bits go straight
/// into the bitmap with one read. Returns the bitmap and the offset just past it.
fn bitmap_at<R: ReadAt>(file: &R, pos: u64, end: u64) -> Result<(Bitmap, u64), DocError> {
    if pos + 4 > end {
        return Err(DocError::Malformed("qbk bitmap"));
    }
    let mut h = [0u8; 4];
    file.read_exact_at(pos, &mut h)?;
    let w = u16::from_le_bytes([h[0], h[1]]) as u32;
    let hgt = u16::from_le_bytes([h[2], h[3]]) as u32;
    let n = (w as u64).div_ceil(8) * hgt as u64;
    if pos + 4 + n > end || n > SECTION_LIMIT as u64 {
        return Err(DocError::Malformed("qbk bitmap bits"));
    }
    let mut bm = Bitmap::new(w, hgt);
    file.read_exact_at(pos + 4, &mut bm.bits)?;
    Ok((bm, pos + 4 + n))
}

/// Where a chapter's pieces live in the file (pass 1 output).
struct ChapLoc {
    index: u16,
    chars: u32,
    title_at: u64,
    title_len: usize,
    qtx_at: u64,
    qtx_end: u64,
}

/// Stream one chapter's QTX from `[start, end)` to the sink, remapping image ids, in
/// pieces of about `FLUSH_BYTES`. Tokens never straddle a piece: the buffer holds only
/// the unparsed tail and grows (up to `SECTION_LIMIT`) only when a single token needs it.
fn stream_chapter<R: ReadAt>(file: &R, start: u64, end: u64, id_map: &[(u16, u16)], sink: &mut dyn Sink) -> Result<(), DocError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut next = start;
    let mut want = READ_CHUNK;
    let mut w = Writer::new();
    loop {
        // Top up to `want` bytes (or the end of the section).
        while buf.len() < want && next < end {
            let n = (want - buf.len()).min((end - next) as usize);
            let old = buf.len();
            buf.resize(old + n, 0);
            file.read_exact_at(next, &mut buf[old..])?;
            next += n as u64;
        }
        if buf.is_empty() {
            break;
        }
        // Copy every complete token; `consumed` marks the last good token boundary.
        let mut r = Reader::new(&buf);
        let mut consumed = 0usize;
        // (`while let` rather than `for`: the boundary offset is needed after each token.)
        #[allow(clippy::while_let_on_iterator)]
        while let Some(t) = r.next() {
            consumed = r.offset() as usize;
            match t {
                Token::Image { id, w: iw, h: ih } => {
                    let real = id_map.iter().find(|m| m.0 == id).map(|m| m.1).unwrap_or(u16::MAX);
                    w.push(&Token::Image { id: real, w: iw, h: ih });
                }
                other => w.push(&other),
            }
            if w.len() >= FLUSH_BYTES {
                sink.chapter_bytes(w.as_bytes())?;
                w = Writer::new();
            }
        }
        if consumed == 0 {
            if next >= end {
                // Corrupt or truncated tail: stop cleanly, as `Reader` does.
                break;
            }
            // One token is longer than the buffer: grow and read more.
            want = (buf.len() * 2).max(READ_CHUNK);
            if want > SECTION_LIMIT {
                return Err(DocError::TooLarge("qbk token"));
            }
        } else {
            buf.drain(..consumed);
            want = READ_CHUNK;
        }
    }
    if !w.is_empty() {
        sink.chapter_bytes(w.as_bytes())?;
    }
    Ok(())
}

/// Ingest a QBK file in two passes: images and small sections first, then chapters
/// streamed in index order.
pub fn ingest<R: ReadAt>(file: &R, sink: &mut dyn Sink) -> Result<(), DocError> {
    let mut hdr = [0u8; 8];
    file.read_exact_at(0, &mut hdr)?;
    if &hdr[..4] != MAGIC {
        return Err(DocError::Malformed("not a qbk"));
    }
    let count = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
    let file_len = file.len();
    let mut pos = 8u64;
    let mut id_map: Vec<(u16, u16)> = Vec::new();
    let mut chapters: Vec<ChapLoc> = Vec::new();
    let mut have_meta = false;
    let mut toc: Vec<TocEntry> = Vec::new();

    // Pass 1: everything except chapter bodies.
    for i in 0..count {
        let mut sh = [0u8; 8];
        file.read_exact_at(pos, &mut sh)?;
        let tag = [sh[0], sh[1], sh[2], sh[3]];
        let len = u32::from_le_bytes([sh[4], sh[5], sh[6], sh[7]]) as u64;
        let payload = pos + 8;
        let end = payload + len;
        if end > file_len {
            return Err(DocError::Malformed("qbk section out of range"));
        }
        let whole = |what: &'static str| -> Result<Vec<u8>, DocError> {
            if len > SECTION_LIMIT as u64 {
                return Err(DocError::TooLarge(what));
            }
            Ok(file.read_range(payload, len as usize)?)
        };
        match &tag {
            b"META" => {
                let p = whole("qbk meta")?;
                let m: Metadata = postcard::from_bytes(&p).map_err(|_| DocError::Malformed("qbk meta"))?;
                sink.metadata(&m)?;
                have_meta = true;
            }
            b"TOC " => {
                let p = whole("qbk toc")?;
                toc = postcard::from_bytes(&p).map_err(|_| DocError::Malformed("qbk toc"))?;
            }
            b"IMG " => {
                if len < 6 {
                    return Err(DocError::Malformed("qbk img"));
                }
                let mut idb = [0u8; 2];
                file.read_exact_at(payload, &mut idb)?;
                let id = u16::from_le_bytes(idb);
                let (bm, _) = bitmap_at(file, payload + 2, end)?;
                let real = sink.image(&bm)?;
                id_map.push((id, real));
            }
            b"COVR" => {
                let (full, at) = bitmap_at(file, payload, end)?;
                let (thumb, _) = bitmap_at(file, at, end)?;
                sink.cover(&full, &thumb)?;
            }
            b"CHAP" => {
                if len < 8 {
                    return Err(DocError::Malformed("qbk chap"));
                }
                let mut h = [0u8; 8];
                file.read_exact_at(payload, &mut h)?;
                let index = u16::from_le_bytes([h[0], h[1]]);
                let chars = u32::from_le_bytes([h[2], h[3], h[4], h[5]]);
                let title_len = u16::from_le_bytes([h[6], h[7]]) as usize;
                if 8 + title_len as u64 > len {
                    return Err(DocError::Malformed("qbk chap title"));
                }
                chapters.push(ChapLoc {
                    index,
                    chars,
                    title_at: payload + 8,
                    title_len,
                    qtx_at: payload + 8 + title_len as u64,
                    qtx_end: end,
                });
            }
            _ => {}
        }
        pos = end;
        sink.progress(i + 1, count + chapters.len() as u32);
    }
    if !have_meta {
        sink.metadata(&Metadata { title: "Untitled".into(), ..Default::default() })?;
    }

    // Pass 2: chapters, streamed in index order.
    chapters.sort_by_key(|c| c.index);
    let total = count + chapters.len() as u32;
    for (j, c) in chapters.iter().enumerate() {
        let title = if c.title_len > 0 {
            let t = file.read_range(c.title_at, c.title_len)?;
            Some(String::from_utf8_lossy(&t).into_owned())
        } else {
            None
        };
        sink.begin_chapter(c.index, title.as_deref())?;
        stream_chapter(file, c.qtx_at, c.qtx_end, &id_map, sink)?;
        sink.end_chapter(c.chars)?;
        sink.progress(count + j as u32 + 1, total);
    }
    sink.toc(&toc)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memsink::MemSink;
    use quire_qtx::ParaKind;

    #[test]
    fn round_trip() {
        let mut w = Writer::new();
        w.para(ParaKind::Body);
        w.text("Hello from a converted book.");
        w.push(&Token::Image { id: 7, w: 100, h: 50 });
        let qtx = w.finish();
        let mut b = Builder::new();
        b.metadata(&Metadata { title: "Converted".into(), authors: alloc::vec!["A".into()], ..Default::default() })
            .toc(&[TocEntry { title: "One".into(), chapter: 0, anchor: None, depth: 0 }])
            .image(7, &Bitmap::new(100, 50))
            .cover(&Bitmap::new(528, 792), &Bitmap::new(152, 228))
            .chapter(0, Some("One"), 28, &qtx);
        let bytes = b.finish();
        let mut sink = MemSink::default();
        ingest(&bytes, &mut sink).expect("ingest");
        assert_eq!(sink.meta.title, "Converted");
        assert_eq!(sink.chapters.len(), 1);
        assert_eq!(sink.images.len(), 1);
        assert!(sink.cover.is_some());
        let toks: Vec<Token> = Reader::new(&sink.chapters[0].1).collect();
        assert!(toks.contains(&Token::Image { id: 0, w: 100, h: 50 }), "image id remapped: {toks:?}");
        assert!(ingest(&b"QBK1\xFF\xFF\xFF\xFF"[..].to_vec(), &mut MemSink::default()).is_err());
    }

    #[test]
    fn images_after_chapters_resolve_and_long_chapters_stream_intact() {
        // Chapter 1 (index 1) is written before chapter 0 and before every image; it is
        // long enough to span many 4 KB pieces and holds one text run far larger than a
        // read chunk, so the streaming copy must grow its buffer for that single token.
        let mut w = Writer::new();
        w.para(ParaKind::Heading(1));
        w.text("Second");
        for i in 0..400 {
            w.para(ParaKind::Body);
            w.text(&alloc::format!("Paragraph {i} of a long converted chapter with enough words to matter."));
            w.push(&Token::Image { id: (i % 3) as u16 + 10, w: 64, h: 32 });
        }
        w.para(ParaKind::Body);
        w.text(&"x".repeat(20_000));
        w.push(&Token::End);
        let long = w.finish();
        let mut w0 = Writer::new();
        w0.para(ParaKind::Body);
        w0.text("First");
        w0.push(&Token::Image { id: 12, w: 8, h: 8 });
        let first = w0.finish();

        let mut b = Builder::new();
        b.chapter(1, Some("Two"), 40_000, &long)
            .image(10, &Bitmap::new(64, 32))
            .chapter(0, Some("One"), 5, &first)
            .image(11, &Bitmap::new(64, 32))
            .metadata(&Metadata { title: "Late meta".into(), ..Default::default() })
            .image(12, &Bitmap::new(8, 8));
        let bytes = b.finish();
        let mut sink = MemSink::default();
        ingest(&bytes, &mut sink).expect("ingest");
        assert_eq!(sink.meta.title, "Late meta");
        assert_eq!(sink.images.len(), 3);
        assert_eq!(sink.chapters.len(), 2);
        assert_eq!(sink.chapters[0].0.as_deref(), Some("One"), "chapters arrive in index order");
        assert_eq!(sink.chapters[1].0.as_deref(), Some("Two"));
        assert_eq!(sink.chapters[1].2, 40_000);

        // The streamed copy equals a whole-buffer remap: ids 10/11/12 → 0/1/2.
        let remap = |qtx: &[u8]| -> Vec<Token> {
            Reader::new(qtx)
                .map(|t| match t {
                    Token::Image { id, w, h } => Token::Image { id: id - 10, w, h },
                    o => o,
                })
                .collect()
        };
        let got1: Vec<Token> = Reader::new(&sink.chapters[1].1).collect();
        assert_eq!(got1.len(), 1 + 1 + 400 * 3 + 3);
        assert_eq!(got1, remap(&long));
        let got0: Vec<Token> = Reader::new(&sink.chapters[0].1).collect();
        assert_eq!(got0, remap(&first));
        assert!(got0.contains(&Token::Image { id: 2, w: 8, h: 8 }), "image declared last still resolves: {got0:?}");
    }

    #[test]
    fn truncated_and_oversized_sections_are_errors() {
        let mut b = Builder::new();
        b.metadata(&Metadata { title: "T".into(), ..Default::default() }).chapter(0, None, 0, &[]);
        let mut bytes = b.finish();
        // A section that claims to run past the end of the file (the last CHAP's payload
        // is 8 bytes; its length field sits just before it).
        let n = bytes.len();
        bytes[n - 12..n - 8].copy_from_slice(&1000u32.to_le_bytes());
        assert!(matches!(ingest(&bytes, &mut MemSink::default()), Err(DocError::Malformed(_))));
        // An IMG whose bitmap needs more bits than the section holds.
        let mut b = Builder::new();
        b.image(1, &Bitmap::new(64, 64));
        let mut bytes = b.finish();
        bytes[8 + 8 + 4..8 + 8 + 6].copy_from_slice(&60_000u16.to_le_bytes());
        assert!(matches!(ingest(&bytes, &mut MemSink::default()), Err(DocError::Malformed(_))));
    }
}

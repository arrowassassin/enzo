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

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_gfx::Bitmap;
use quire_qtx::{Reader, Token, Writer};

use crate::{DocError, Metadata, Sink, TocEntry};

/// Magic.
pub const MAGIC: &[u8; 4] = b"QBK1";

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

fn bitmap_from(p: &[u8], at: &mut usize) -> Result<Bitmap, DocError> {
    if *at + 4 > p.len() {
        return Err(DocError::Malformed("qbk bitmap"));
    }
    let w = u16::from_le_bytes([p[*at], p[*at + 1]]) as u32;
    let h = u16::from_le_bytes([p[*at + 2], p[*at + 3]]) as u32;
    *at += 4;
    let n = (w as usize).div_ceil(8) * h as usize;
    if *at + n > p.len() {
        return Err(DocError::Malformed("qbk bitmap bits"));
    }
    let bits = p[*at..*at + n].to_vec();
    *at += n;
    Ok(Bitmap { w, h, bits })
}

/// Ingest a QBK file, streaming sections.
pub fn ingest<R: ReadAt>(file: &R, sink: &mut dyn Sink) -> Result<(), DocError> {
    let mut hdr = [0u8; 8];
    file.read_exact_at(0, &mut hdr)?;
    if &hdr[..4] != MAGIC {
        return Err(DocError::Malformed("not a qbk"));
    }
    let count = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
    let mut pos = 8u64;
    let mut id_map: Vec<(u16, u16)> = Vec::new();
    let mut pending_chapters: Vec<(u16, Option<String>, u32, Vec<u8>)> = Vec::new();
    let mut have_meta = false;
    let mut toc: Vec<TocEntry> = Vec::new();
    for i in 0..count {
        let mut sh = [0u8; 8];
        file.read_exact_at(pos, &mut sh)?;
        let tag = [sh[0], sh[1], sh[2], sh[3]];
        let len = u32::from_le_bytes([sh[4], sh[5], sh[6], sh[7]]) as usize;
        if len > 64 * 1024 * 1024 {
            return Err(DocError::TooLarge("qbk section"));
        }
        let p = file.read_range(pos + 8, len)?;
        pos += 8 + len as u64;
        match &tag {
            b"META" => {
                let m: Metadata = postcard::from_bytes(&p).map_err(|_| DocError::Malformed("qbk meta"))?;
                sink.metadata(&m)?;
                have_meta = true;
            }
            b"TOC " => toc = postcard::from_bytes(&p).map_err(|_| DocError::Malformed("qbk toc"))?,
            b"IMG " => {
                if p.len() < 6 {
                    return Err(DocError::Malformed("qbk img"));
                }
                let id = u16::from_le_bytes([p[0], p[1]]);
                let mut at = 2usize;
                let bm = bitmap_from(&p, &mut at)?;
                let real = sink.image(&bm)?;
                id_map.push((id, real));
            }
            b"COVR" => {
                let mut at = 0usize;
                let full = bitmap_from(&p, &mut at)?;
                let thumb = bitmap_from(&p, &mut at)?;
                sink.cover(&full, &thumb)?;
            }
            b"CHAP" => {
                if p.len() < 8 {
                    return Err(DocError::Malformed("qbk chap"));
                }
                let index = u16::from_le_bytes([p[0], p[1]]);
                let chars = u32::from_le_bytes([p[2], p[3], p[4], p[5]]);
                let tl = u16::from_le_bytes([p[6], p[7]]) as usize;
                if 8 + tl > p.len() {
                    return Err(DocError::Malformed("qbk chap title"));
                }
                let title = if tl > 0 { Some(String::from_utf8_lossy(&p[8..8 + tl]).into_owned()) } else { None };
                pending_chapters.push((index, title, chars, p[8 + tl..].to_vec()));
            }
            _ => {}
        }
        sink.progress(i + 1, count);
    }
    if !have_meta {
        sink.metadata(&Metadata { title: "Untitled".into(), ..Default::default() })?;
    }
    pending_chapters.sort_by_key(|c| c.0);
    for (index, title, chars, qtx) in pending_chapters {
        sink.begin_chapter(index, title.as_deref())?;
        // Remap image ids.
        let mut w = Writer::new();
        for t in Reader::new(&qtx) {
            match t {
                Token::Image { id, w: iw, h: ih } => {
                    let real = id_map.iter().find(|m| m.0 == id).map(|m| m.1).unwrap_or(u16::MAX);
                    w.push(&Token::Image { id: real, w: iw, h: ih });
                }
                other => w.push(&other),
            }
        }
        sink.chapter_bytes(w.as_bytes())?;
        sink.end_chapter(chars)?;
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
}

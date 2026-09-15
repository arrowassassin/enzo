//! An open book: its cache sections, TOC and images, plus position arithmetic.

use alloc::string::String;
use alloc::vec::Vec;
use quire_doc::{Metadata, TocEntry};
use quire_fs::Fs;
use quire_gfx::{Bitmap, BitmapRef};
use quire_layout::{ImageSource, Pos};
use quire_qtx::{Reader, Token};

use crate::cache::{self, anchor_hash, AnchorRec, SectionInfo};
use crate::index::Loc;
use crate::{book_dir, BookId, LibError, LibResult};

/// Largest section we will load (the cache never writes bigger ones, but a card can lie).
const SECTION_LIMIT: usize = 256 * 1024;
/// Images kept decoded at once.
const IMAGE_SLOTS: usize = 6;
/// Total bytes of decoded images kept.
const IMAGE_BYTES: usize = 160 * 1024;

/// An open book.
#[derive(Debug)]
pub struct Book {
    /// Id.
    pub id: BookId,
    /// Cache directory.
    pub dir: String,
    /// Metadata.
    pub meta: Metadata,
    /// Table of contents.
    pub toc: Vec<TocEntry>,
    /// Sections in reading order.
    pub sections: Vec<SectionInfo>,
    /// Characters before each section (len = sections + 1; last = total).
    cum: Vec<u32>,
    /// Anchor table, sorted by hash.
    anchors: Vec<AnchorRec>,
    /// First section of each ingest chapter (index = chapter).
    chapter_first: Vec<u16>,
    /// Every TOC entry resolved to a location (parallel to `toc`).
    pub toc_locs: Vec<Loc>,
}

impl Book {
    /// Open a book's cache.
    pub fn open<F: Fs>(fs: &F, id: BookId) -> LibResult<Book> {
        let dir = book_dir(id);
        let sections: Vec<SectionInfo> =
            postcard::from_bytes(&fs.read_to_vec(&quire_fs::join(&dir, "sections.bin"))?).map_err(|_| LibError::Corrupt("sections.bin"))?;
        let meta: Metadata =
            fs.read_to_vec(&quire_fs::join(&dir, "meta.bin")).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default();
        let toc: Vec<TocEntry> =
            fs.read_to_vec(&quire_fs::join(&dir, "toc.bin")).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default();
        let mut cum = Vec::with_capacity(sections.len() + 1);
        let mut n = 0u32;
        for s in &sections {
            cum.push(n);
            n = n.saturating_add(s.chars);
        }
        cum.push(n);
        let anchors: Vec<AnchorRec> =
            fs.read_to_vec(&quire_fs::join(&dir, "anchors.bin")).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default();
        let max_chapter = sections.iter().map(|s| s.source as usize + 1).max().unwrap_or(0);
        let mut chapter_first = alloc::vec![u16::MAX; max_chapter];
        for (i, s) in sections.iter().enumerate() {
            let slot = &mut chapter_first[s.source as usize];
            if *slot == u16::MAX || (s.part == 0 && sections[*slot as usize].part != 0) {
                *slot = i as u16;
            }
        }
        let mut book = Book { id, dir, meta, toc, sections, cum, anchors, chapter_first, toc_locs: Vec::new() };
        book.toc_locs = book.toc.iter().map(|e| book.resolve_toc_entry(e)).collect();
        Ok(book)
    }

    fn resolve_toc_entry(&self, entry: &TocEntry) -> Loc {
        let first = self.section_of_chapter(entry.chapter).unwrap_or(0);
        let fallback = Loc { section: first, pos: Pos::START, chars: self.chars_before_section(first) };
        match entry.anchor.as_deref() {
            Some(a) => self.resolve_anchor_in_chapter(Some(entry.chapter), a).unwrap_or(fallback),
            None => fallback,
        }
    }

    /// Resolve an anchor id, optionally within an ingest chapter, from the anchor table.
    fn resolve_anchor_in_chapter(&self, chapter: Option<u16>, id: &str) -> Option<Loc> {
        let h = anchor_hash(id);
        let start = self.anchors.partition_point(|a| a.hash < h);
        for rec in self.anchors[start..].iter().take_while(|a| a.hash == h) {
            let sec = self.sections.get(rec.section as usize)?;
            if chapter.map(|c| sec.source == c).unwrap_or(true) {
                return Some(Loc {
                    section: rec.section,
                    pos: Pos { para: rec.offset, word: 0, part: 0 },
                    chars: self.cum[rec.section as usize] + rec.chars,
                });
            }
        }
        None
    }

    /// Resolve a link or footnote target: `"12#note-3"`, `"12"` (a chapter) or `"note-3"`.
    pub fn resolve_anchor(&self, target: &str) -> Option<Loc> {
        let (chapter, id) = match target.split_once('#') {
            Some((c, id)) => (c.parse::<u16>().ok(), id),
            None => match target.parse::<u16>() {
                Ok(c) => {
                    let s = self.section_of_chapter(c)?;
                    return Some(Loc { section: s, pos: Pos::START, chars: self.chars_before_section(s) });
                }
                Err(_) => (None, target),
            },
        };
        if id.is_empty() {
            let s = self.section_of_chapter(chapter?)?;
            return Some(Loc { section: s, pos: Pos::START, chars: self.chars_before_section(s) });
        }
        self.resolve_anchor_in_chapter(chapter, id).or_else(|| self.resolve_anchor_in_chapter(None, id))
    }

    /// The text that follows an anchor (a footnote body): the first paragraph after it.
    pub fn note_text<F: Fs>(&self, fs: &F, target: &str) -> Option<String> {
        let loc = self.resolve_anchor(target)?;
        let data = self.section(fs, loc.section).ok()?;
        let mut r = Reader::at(&data, loc.pos.para);
        let mut out = String::new();
        let mut started = false;
        while let Some(tag) = r.peek_tag() {
            match tag {
                quire_qtx::Tag::Text => {
                    if let Some(b) = r.text_bytes() {
                        if let Ok(t) = core::str::from_utf8(b) {
                            if started && !out.is_empty() && !out.ends_with(' ') && !t.starts_with(' ') {
                                out.push(' ');
                            }
                            out.push_str(t);
                            started = true;
                        }
                    }
                }
                quire_qtx::Tag::Para | quire_qtx::Tag::End | quire_qtx::Tag::ChapterTitle => {
                    if started {
                        break;
                    }
                    r.skip_token()?;
                }
                _ => {
                    r.skip_token()?;
                }
            }
            if out.len() > 2000 {
                break;
            }
        }
        let t = out.trim();
        (!t.is_empty()).then(|| String::from(t))
    }

    /// The chapter (TOC entry) containing `chars`, with its character bounds: (from, to, toc index).
    /// Without a TOC, ingest chapters are the bounds.
    pub fn chapter_bounds(&self, chars: u32) -> (u32, u32, Option<usize>) {
        let total = self.total_chars();
        if let Some(i) = self.toc_index_at_chars(chars) {
            let from = self.toc_locs[i].chars;
            let to = self.toc_locs.iter().map(|l| l.chars).filter(|c| *c > from).min().unwrap_or(total);
            return (from, to.max(from), Some(i));
        }
        let (section, _) = self.section_at_chars(chars);
        let chapter = self.sections.get(section as usize).map(|s| s.source).unwrap_or(0);
        let first = self.section_of_chapter(chapter).unwrap_or(section);
        let mut last = first;
        while (last as usize + 1) < self.sections.len() && self.sections[last as usize + 1].source == chapter {
            last += 1;
        }
        (self.chars_before_section(first), self.chars_before_section(last + 1), None)
    }

    /// The TOC entry at a global character offset: the entry with the greatest start at or
    /// before it (later entries win ties).
    pub fn toc_index_at_chars(&self, chars: u32) -> Option<usize> {
        let mut best: Option<(usize, u32)> = None;
        for (i, l) in self.toc_locs.iter().enumerate() {
            if l.chars <= chars && best.map(|b| l.chars >= b.1).unwrap_or(true) {
                best = Some((i, l.chars));
            }
        }
        best.map(|b| b.0)
    }

    /// Total characters.
    pub fn total_chars(&self) -> u32 {
        *self.cum.last().unwrap_or(&0)
    }
    /// Characters before a section.
    pub fn chars_before_section(&self, section: u16) -> u32 {
        self.cum.get(section as usize).copied().unwrap_or(self.total_chars())
    }
    /// Number of sections.
    pub fn section_count(&self) -> u16 {
        self.sections.len() as u16
    }
    /// Read a section's QTX bytes.
    pub fn section<F: Fs>(&self, fs: &F, n: u16) -> LibResult<Vec<u8>> {
        if n as usize >= self.sections.len() {
            return Err(LibError::NotFound);
        }
        let f = fs.open(&cache::section_path(&self.dir, n as usize))?;
        let len = (quire_fs::ReadAt::len(&f) as usize).min(SECTION_LIMIT);
        Ok(quire_fs::ReadAt::read_range(&f, 0, len)?)
    }

    /// Title shown for a section: its own, or its chapter's first part's.
    pub fn section_title(&self, n: u16) -> Option<&str> {
        let s = self.sections.get(n as usize)?;
        if let Some(t) = s.title.as_deref() {
            return Some(t);
        }
        let first = self.section_of_chapter(s.source)?;
        self.sections.get(first as usize)?.title.as_deref()
    }

    /// The section holding a global character offset, and the offset within it.
    pub fn section_at_chars(&self, chars: u32) -> (u16, u32) {
        if self.sections.is_empty() {
            return (0, 0);
        }
        let i = self.cum.partition_point(|c| *c <= chars).saturating_sub(1).min(self.sections.len() - 1);
        (i as u16, chars - self.cum[i])
    }

    /// A location from a global character offset (used for percent jumps and sync).
    pub fn loc_at_chars<F: Fs>(&self, fs: &F, chars: u32) -> LibResult<Loc> {
        let chars = chars.min(self.total_chars());
        let (section, within) = self.section_at_chars(chars);
        let data = self.section(fs, section)?;
        let pos = quire_layout::pos_at_chars(&data, within);
        let exact = self.cum[section as usize] + quire_layout::chars_before(&data, pos);
        Ok(Loc { section, pos, chars: exact })
    }

    /// Global character offset of a position.
    pub fn chars_at(&self, section: u16, data: &[u8], pos: Pos) -> u32 {
        self.chars_before_section(section) + quire_layout::chars_before(data, pos)
    }

    /// First section of an ingest chapter.
    pub fn section_of_chapter(&self, chapter: u16) -> Option<u16> {
        self.chapter_first.get(chapter as usize).copied().filter(|s| *s != u16::MAX)
    }

    /// Resolve a TOC entry to a location (from the anchor table; no card access).
    pub fn toc_target(&self, index: usize) -> Option<Loc> {
        self.toc_locs.get(index).copied()
    }

    /// The TOC entry the reader is in (the last entry starting at or before `loc`).
    pub fn toc_index_at(&self, loc: &Loc) -> Option<usize> {
        self.toc_index_at_chars(loc.chars)
    }

    /// Load the cover thumbnail.
    pub fn thumb<F: Fs>(&self, fs: &F) -> Option<Bitmap> {
        cache::load_pbm(fs, &quire_fs::join(&self.dir, "thumb.pbm"))
    }
    /// Load the full-page cover.
    pub fn cover<F: Fs>(&self, fs: &F) -> Option<Bitmap> {
        cache::load_pbm(fs, &quire_fs::join(&self.dir, "cover.pbm"))
    }
    /// Stream the full-page cover into a frame without decoding it into RAM (see
    /// [`cache::load_pbm_into`]).
    pub fn cover_into<F: Fs>(&self, fs: &F, f: &mut quire_gfx::Frame, place: impl FnOnce(u32, u32) -> (i32, i32)) -> Option<(u32, u32)> {
        cache::load_pbm_into(fs, &quire_fs::join(&self.dir, "cover.pbm"), f, place, quire_gfx::BlitMode::Or)
    }
    /// Load one image.
    pub fn image<F: Fs>(&self, fs: &F, id: u16) -> Option<Bitmap> {
        cache::load_pbm(fs, &cache::image_path(&self.dir, id))
    }
}

/// Byte offset of an anchor token in a section.
pub fn find_anchor(data: &[u8], id: &str) -> Option<u32> {
    let mut r = Reader::new(data);
    loop {
        let off = r.offset();
        match r.next() {
            Some(Token::Anchor(a)) if a == id => return Some(off),
            Some(_) => {}
            None => return None,
        }
    }
}

/// A small decoded-image store for rendering: fill it with the ids a page needs.
#[derive(Default)]
pub struct ImageStore {
    images: Vec<(u16, Bitmap)>,
}

impl ImageStore {
    /// Empty.
    pub fn new() -> Self {
        Self::default()
    }
    /// Ensure the given ids are loaded (evicting the oldest beyond the budget).
    pub fn ensure<F: Fs>(&mut self, fs: &F, book: &Book, ids: &[u16]) {
        for &id in ids {
            if self.images.iter().any(|(i, _)| *i == id) {
                continue;
            }
            if let Some(bm) = book.image(fs, id) {
                self.images.push((id, bm));
            }
        }
        // Keep the most recently requested ids.
        while self.images.len() > IMAGE_SLOTS || self.images.iter().map(|(_, b)| b.bits.len()).sum::<usize>() > IMAGE_BYTES {
            let victim = self.images.iter().position(|(i, _)| !ids.contains(i)).unwrap_or(0);
            self.images.remove(victim);
            if self.images.is_empty() {
                break;
            }
        }
    }
    /// Drop everything.
    pub fn clear(&mut self) {
        self.images.clear();
    }
}

impl ImageSource for ImageStore {
    fn image(&self, id: u16, _w: u32, _h: u32) -> Option<BitmapRef<'_>> {
        self.images.iter().find(|(i, _)| *i == id).map(|(_, b)| b.as_ref())
    }
}

/// Image ids a laid-out page refers to.
pub fn page_image_ids(page: &quire_layout::Page) -> Vec<u16> {
    let mut ids = Vec::new();
    for item in &page.items {
        if let quire_layout::DrawItem::Image { id, .. } = item {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
    }
    ids
}

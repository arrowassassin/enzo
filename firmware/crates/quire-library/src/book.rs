//! An open book: its cache sections, TOC and images, plus position arithmetic.

use alloc::string::String;
use alloc::vec::Vec;
use quire_doc::{Metadata, TocEntry};
use quire_fs::Fs;
use quire_gfx::{Bitmap, BitmapRef};
use quire_layout::{ImageSource, Pos};
use quire_qtx::{Reader, Token};

use crate::cache::{self, SectionInfo};
use crate::index::Loc;
use crate::{book_dir, BookId, LibError, LibResult};

/// Largest section we will load (the cache never writes bigger ones, but a card can lie).
const SECTION_LIMIT: usize = 256 * 1024;
/// Images kept decoded at once.
const IMAGE_SLOTS: usize = 6;
/// Total bytes of decoded images kept.
const IMAGE_BYTES: usize = 160 * 1024;

/// An open book.
#[derive(Clone, Debug)]
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
        Ok(Book { id, dir, meta, toc, sections, cum })
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

    /// Title shown for a section (its own, or the chapter's for continuation parts).
    pub fn section_title(&self, n: u16) -> Option<&str> {
        self.sections.get(n as usize).and_then(|s| s.title.as_deref())
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
        self.sections.iter().position(|s| s.source == chapter && s.part == 0).map(|i| i as u16)
    }

    /// Resolve a TOC entry to a location, scanning the chapter's parts for its anchor.
    pub fn toc_target<F: Fs>(&self, fs: &F, entry: &TocEntry) -> LibResult<Loc> {
        let first = self.section_of_chapter(entry.chapter).ok_or(LibError::NotFound)?;
        let Some(anchor) = entry.anchor.as_deref() else {
            return Ok(Loc { section: first, pos: Pos::START, chars: self.chars_before_section(first) });
        };
        let mut n = first;
        while (n as usize) < self.sections.len() && self.sections[n as usize].source == entry.chapter {
            let data = self.section(fs, n)?;
            if let Some(off) = find_anchor(&data, anchor) {
                let pos = quire_layout::para::blocks(&data)
                    .find(|b| b.next > off)
                    .map(|b| Pos { para: b.off, word: 0, part: 0 })
                    .unwrap_or(Pos::START);
                let chars = self.chars_at(n, &data, pos);
                return Ok(Loc { section: n, pos, chars });
            }
            n += 1;
        }
        Ok(Loc { section: first, pos: Pos::START, chars: self.chars_before_section(first) })
    }

    /// The TOC entry the reader is in (the last entry at or before `loc`).
    pub fn toc_index_at(&self, loc: &Loc) -> Option<usize> {
        let mut best: Option<(usize, u16)> = None;
        for (i, e) in self.toc.iter().enumerate() {
            let Some(s) = self.section_of_chapter(e.chapter) else { continue };
            if s <= loc.section && best.map(|b| s >= b.1).unwrap_or(true) {
                best = Some((i, s));
            }
        }
        best.map(|b| b.0)
    }

    /// Load the cover thumbnail.
    pub fn thumb<F: Fs>(&self, fs: &F) -> Option<Bitmap> {
        cache::load_pbm(fs, &quire_fs::join(&self.dir, "thumb.pbm"))
    }
    /// Load the full-page cover.
    pub fn cover<F: Fs>(&self, fs: &F) -> Option<Bitmap> {
        cache::load_pbm(fs, &quire_fs::join(&self.dir, "cover.pbm"))
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

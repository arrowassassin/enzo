//! Bookmarks, highlights and notes, stored per book in `marks.bin`.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_layout::Pos;
use serde::{Deserialize, Serialize};

use crate::{LibError, LibResult};

/// Kind of mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkKind {
    /// A page bookmark.
    Bookmark,
    /// A highlighted passage.
    Highlight,
    /// A highlight with a note.
    Note,
}

/// One mark.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    /// Kind.
    pub kind: MarkKind,
    /// Section.
    pub section: u16,
    /// Position.
    pub pos: Pos,
    /// Global character offset (for ordering and percent).
    pub chars: u32,
    /// Highlighted text or the page's first words (≤ 240 bytes).
    pub excerpt: String,
    /// The note, if any.
    pub note: String,
    /// Created (local seconds).
    pub created: u32,
}

/// Bound on marks per book.
pub const MAX_MARKS: usize = 1000;
const EXCERPT_BYTES: usize = 240;

/// A book's marks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marks {
    /// Sorted by position.
    pub items: Vec<Mark>,
}

impl Marks {
    /// Load (empty when missing).
    pub fn load<F: Fs>(fs: &F, dir: &str) -> Marks {
        fs.read_to_vec(&quire_fs::join(dir, "marks.bin")).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default()
    }
    /// Save atomically.
    pub fn save<F: Fs>(&self, fs: &F, dir: &str) -> LibResult<()> {
        let enc = postcard::to_allocvec(self).map_err(|_| LibError::Corrupt("marks encode"))?;
        Ok(fs.write_atomic(&quire_fs::join(dir, "marks.bin"), &enc)?)
    }
    /// Add a mark (excerpt trimmed to the byte bound on a char boundary).
    pub fn add(&mut self, mut mark: Mark) -> bool {
        if self.items.len() >= MAX_MARKS {
            return false;
        }
        if mark.excerpt.len() > EXCERPT_BYTES {
            let mut cut = EXCERPT_BYTES;
            while !mark.excerpt.is_char_boundary(cut) {
                cut -= 1;
            }
            mark.excerpt.truncate(cut);
        }
        let i = self.items.partition_point(|m| (m.section, m.chars) <= (mark.section, mark.chars));
        self.items.insert(i, mark);
        true
    }
    /// Remove by index.
    pub fn remove(&mut self, i: usize) -> Option<Mark> {
        if i < self.items.len() {
            Some(self.items.remove(i))
        } else {
            None
        }
    }
    /// Toggle a bookmark at a position; returns true when added.
    pub fn toggle_bookmark(&mut self, section: u16, pos: Pos, chars: u32, excerpt: &str, now: u32) -> bool {
        if let Some(i) = self.items.iter().position(|m| m.kind == MarkKind::Bookmark && m.section == section && m.pos == pos) {
            self.items.remove(i);
            return false;
        }
        self.add(Mark { kind: MarkKind::Bookmark, section, pos, chars, excerpt: excerpt.into(), note: String::new(), created: now })
    }
    /// Whether a page (starting at `pos`) is bookmarked.
    pub fn has_bookmark(&self, section: u16, pos: Pos) -> bool {
        self.items.iter().any(|m| m.kind == MarkKind::Bookmark && m.section == section && m.pos == pos)
    }
    /// Marks in a section, in order.
    pub fn in_section(&self, section: u16) -> impl Iterator<Item = &Mark> {
        self.items.iter().filter(move |m| m.section == section)
    }
    /// Export as Markdown (for the Drop page and for sharing).
    pub fn to_markdown(&self, title: &str) -> String {
        let mut s = alloc::format!("# {title}\n\n");
        for m in &self.items {
            match m.kind {
                MarkKind::Bookmark => s.push_str(&alloc::format!("- Bookmark: {}\n", m.excerpt)),
                MarkKind::Highlight => s.push_str(&alloc::format!("> {}\n\n", m.excerpt)),
                MarkKind::Note => s.push_str(&alloc::format!("> {}\n\n{}\n\n", m.excerpt, m.note)),
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_stay_sorted_and_bounded() {
        let mut m = Marks::default();
        assert!(m.toggle_bookmark(2, Pos::START, 500, "later", 1));
        assert!(m.toggle_bookmark(1, Pos::START, 100, "earlier", 2));
        assert_eq!(m.items[0].excerpt, "earlier");
        assert!(!m.toggle_bookmark(1, Pos::START, 100, "earlier", 3));
        assert_eq!(m.items.len(), 1);
        let long = "é".repeat(200);
        assert!(m.add(Mark {
            kind: MarkKind::Highlight,
            section: 0,
            pos: Pos::START,
            chars: 0,
            excerpt: long,
            note: String::new(),
            created: 0
        }));
        assert!(m.items[0].excerpt.len() <= EXCERPT_BYTES);
        assert!(m.to_markdown("T").contains("> é"));
    }
}

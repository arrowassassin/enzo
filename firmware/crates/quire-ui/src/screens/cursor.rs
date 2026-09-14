//! 26 the word cursor: lands on the rarest word, moves by rarity or line, looks up,
//! grows a selection for a highlight.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{Frame, Ink, Rect, TextStyle};
use quire_library::marks::{Mark, MarkKind};

use crate::text::draw_right;
use crate::theme::*;
use crate::widgets::{rail, CONTENT_TOP};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

/// Word tables baked by `build.rs` from `data/common-words.txt` and
/// `data/wordle-words.txt`: hashed frequency ranks for the cursor and the dictionary
/// flow, packed five-letter words for Wordle. Nothing here is scanned at run time.
pub mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

/// How many words the cursor cycles through with Rarer / Next.
const CYCLE: usize = 6;

/// FNV-1a over the lowercase form of `word`, matching the build script's hashing.
fn hash_lower(word: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut buf = [0u8; 4];
    for c in word.chars() {
        for l in c.to_lowercase() {
            for b in l.encode_utf8(&mut buf).bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
    }
    h
}

/// The word without surrounding punctuation or a possessive ending, which is what the
/// frequency list and the dictionary know.
pub fn normalise(word: &str) -> &str {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric());
    let w = w.strip_suffix("'s").or_else(|| w.strip_suffix("’s")).unwrap_or(w);
    w.trim_end_matches(|c: char| !c.is_alphanumeric())
}

/// Rank of a word in the frequency list (0 = "the"); None when not in the list.
pub fn rank(word: &str) -> Option<u32> {
    let w = normalise(word);
    if w.is_empty() {
        return None;
    }
    let h = hash_lower(w);
    words::COMMON_HASHES.binary_search(&h).ok().map(|i| words::COMMON_RANKS[i] as u32)
}

/// Rarity score: higher is rarer. Unknown words are rarest (capitalised ones — names,
/// mostly — below unknown common nouns); short words and numbers are common.
pub fn rarity(word: &str) -> u32 {
    let w = normalise(word);
    let n = w.chars().count();
    if n < 4 || w.chars().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    match rank(w) {
        Some(r) => r + 1,
        None => {
            let mut chars = w.chars();
            let capitalised = chars.next().map(|c| c.is_uppercase()).unwrap_or(false) && chars.all(|c| !c.is_uppercase());
            if capitalised {
                50_000 + n as u32
            } else {
                100_000 + n as u32
            }
        }
    }
}

/// The rarest word on a page.
pub fn rarest_word(words: &[(String, Rect)]) -> Option<String> {
    words.iter().map(|(w, _)| w).max_by_key(|w| rarity(w)).cloned()
}

/// The word cursor screen.
pub struct WordCursor {
    words: Vec<(String, Rect)>,
    /// Words ordered by rarity (indexes into `words`).
    by_rarity: Vec<usize>,
    /// Current word index (into `words`).
    cur: usize,
    /// Position in the rarity order (may be past `CYCLE` after a line move).
    rare_pos: usize,
    /// Selection end (inclusive) when selecting.
    sel_end: Option<usize>,
}

impl WordCursor {
    /// New: lands on the rarest word of the page.
    pub fn new() -> Self {
        WordCursor { words: Vec::new(), by_rarity: Vec::new(), cur: 0, rare_pos: 0, sel_end: None }
    }
    fn ensure<E: Env>(&mut self, cx: &mut Ctx<E>) {
        if !self.words.is_empty() {
            return;
        }
        if let Some(r) = cx.reader.as_mut() {
            self.words = r.page_words();
            // Rarity once per word, then a sort over the scores (no work per comparison).
            let scores: Vec<u32> = self.words.iter().map(|(w, _)| rarity(w)).collect();
            let mut order: Vec<usize> = (0..self.words.len()).collect();
            order.sort_by(|a, b| scores[*b].cmp(&scores[*a]).then(a.cmp(b)));
            self.by_rarity = order;
            self.cur = self.by_rarity.first().copied().unwrap_or(0);
            self.rare_pos = 0;
        }
    }
    /// How many words Rarer / Next cycle through.
    fn cycle(&self) -> usize {
        self.by_rarity.len().clamp(1, CYCLE)
    }
    fn move_line(&mut self, down: bool) {
        let Some((_, r)) = self.words.get(self.cur) else { return };
        let (cx, cy) = (r.x + r.w as i32 / 2, r.y);
        let target_y = if down {
            self.words.iter().map(|(_, w)| w.y).filter(|y| *y > cy).min()
        } else {
            self.words.iter().map(|(_, w)| w.y).filter(|y| *y < cy).max()
        };
        let Some(ty) = target_y else { return };
        if let Some((i, _)) =
            self.words.iter().enumerate().filter(|(_, (_, w))| w.y == ty).min_by_key(|(_, (_, w))| (w.x + w.w as i32 / 2 - cx).abs())
        {
            self.cur = i;
            self.rare_pos = self.by_rarity.iter().position(|x| *x == i).unwrap_or(usize::MAX);
        }
    }
    fn selection_text(&self) -> String {
        let end = self.sel_end.unwrap_or(self.cur);
        let (a, b) = if end >= self.cur { (self.cur, end) } else { (end, self.cur) };
        self.words[a..=b.min(self.words.len().saturating_sub(1))].iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>().join(" ")
    }
}

impl Default for WordCursor {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for WordCursor {
    fn name(&self) -> &'static str {
        "26-cursor"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        self.ensure(cx);
        if self.words.is_empty() {
            rail(f, ["", "Cancel", "", ""], None);
            return Refresh::Du;
        }
        let end = self.sel_end.unwrap_or(self.cur);
        let (a, b) = if end >= self.cur { (self.cur, end) } else { (end, self.cur) };
        for (i, (_, r)) in self.words.iter().enumerate() {
            if i >= a && i <= b {
                if self.sel_end.is_some() {
                    f.invert_rect(Rect::new(r.x - 1, r.y, r.w + 2, r.h));
                } else {
                    f.fill_rect(Rect::new(r.x, r.bottom() + 1, r.w, 2), Ink::Black);
                }
            }
        }
        // Counter "1 / 6" (rarity position within the cycle, "—" off it), or "selection".
        let n = self.cycle();
        let label = if self.sel_end.is_some() {
            String::from("selection")
        } else if self.rare_pos < n {
            alloc::format!("{} / {}", self.rare_pos + 1, n)
        } else {
            String::from("—")
        };
        // The counter takes the chapter's place in the running head while the cursor is up,
        // in the small mono face.
        if let Some(r) = cx.reader.as_ref() {
            let w = f.width() as i32;
            let text = r.text_rect();
            let right = (w - MARGIN - SPINE_W - 6).max(text.right());
            f.fill_rect(Rect::new(0, 0, w as u32, (MARGIN + 26) as u32), Ink::White);
            crate::widgets::reading_head(f, &r.book.meta.title, "", text.x, right, MARGIN + 18);
            draw_right(f, quire_fonts::ui::mono(), right, MARGIN + 18, &label, TextStyle::INK);
        }
        // The Spine is not needed while the cursor is up: clear its strip so the side
        // labels are not drawn through it.
        let w = f.width() as i32;
        let h = f.height() as i32;
        let strip_top = cx.reader.as_ref().map(|r| r.spine_rect().y).unwrap_or(CONTENT_TOP).min(CONTENT_TOP);
        f.fill_rect(Rect::new(w - 40, strip_top, 40, (h - RAIL_H - strip_top).max(0) as u32), Ink::White);
        if self.sel_end.is_some() {
            crate::widgets::side_labels(f, Some("Less"), Some("More"), true);
            rail(f, ["Less", "Cancel", "Save", "More"], None);
        } else {
            crate::widgets::side_labels(f, Some("Line"), Some("Line"), true);
            rail(f, ["Rarer", "Cancel", "Define", "Next"], None);
        }
        Refresh::Du
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        self.ensure(cx);
        if self.words.is_empty() {
            return Action::Pop;
        }
        let n = self.words.len();
        let cycle = self.cycle();
        match (ev.key, ev.kind) {
            (Key::Back, KeyKind::Press) => {
                if self.sel_end.is_some() {
                    self.sel_end = None;
                    return Action::Redraw;
                }
                Action::Pop
            }
            (Key::Confirm, KeyKind::Long) => {
                self.sel_end = Some(self.cur);
                Action::Redraw
            }
            (Key::Confirm, KeyKind::Press) => {
                if self.sel_end.is_some() {
                    // Save the highlight.
                    let text = self.selection_text();
                    let fs = cx.env.fs();
                    let now = cx.env.now();
                    if let Some(r) = cx.reader.as_mut() {
                        let loc = r.loc();
                        r.marks.add(Mark {
                            kind: MarkKind::Highlight,
                            section: loc.section,
                            pos: loc.pos,
                            chars: loc.chars,
                            excerpt: text,
                            note: String::new(),
                            created: now,
                        });
                        let _ = r.marks.save(fs, &r.book.dir);
                    }
                    return Action::Pop;
                }
                let word = String::from(normalise(&self.words[self.cur].0));
                Action::Replace(Box::new(super::dictionary::Dictionary::new(word)))
            }
            (Key::Right, KeyKind::Press) | (Key::Right, KeyKind::Repeat) | (Key::Right, KeyKind::Long) => {
                if self.sel_end.is_some() {
                    self.sel_end = Some((self.sel_end.unwrap_or(self.cur) + 1).min(n - 1));
                } else {
                    self.rare_pos = if self.rare_pos >= cycle { 0 } else { (self.rare_pos + 1) % cycle };
                    self.cur = self.by_rarity[self.rare_pos];
                }
                Action::Redraw
            }
            (Key::Left, KeyKind::Press) | (Key::Left, KeyKind::Repeat) | (Key::Left, KeyKind::Long) => {
                if self.sel_end.is_some() {
                    let e = self.sel_end.unwrap_or(self.cur);
                    self.sel_end = Some(if e > self.cur { e - 1 } else { self.cur });
                } else {
                    self.rare_pos = if self.rare_pos == 0 || self.rare_pos >= cycle { cycle - 1 } else { self.rare_pos - 1 };
                    self.cur = self.by_rarity[self.rare_pos];
                }
                Action::Redraw
            }
            (Key::Up, KeyKind::Press) | (Key::Up, KeyKind::Repeat) => {
                if self.sel_end.is_some() {
                    let e = self.sel_end.unwrap_or(self.cur);
                    self.sel_end = Some(if e > self.cur { e - 1 } else { self.cur });
                } else {
                    self.move_line(false);
                }
                Action::Redraw
            }
            (Key::Down, KeyKind::Press) | (Key::Down, KeyKind::Repeat) => {
                if self.sel_end.is_some() {
                    self.sel_end = Some((self.sel_end.unwrap_or(self.cur) + 1).min(n - 1));
                } else {
                    self.move_line(true);
                }
                Action::Redraw
            }
            _ => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_come_from_the_table() {
        assert_eq!(rank("the"), Some(0));
        assert_eq!(rank("The"), Some(0));
        assert_eq!(rank("THE,"), Some(0));
        assert!(rank("zzzzqx").is_none());
        assert!(rank("").is_none());
        assert_eq!(words::COMMON_HASHES.len(), words::COMMON_LEN);
        assert!(words::COMMON_HASHES.windows(2).all(|p| p[0] < p[1]));
    }

    #[test]
    fn possessives_and_punctuation_do_not_make_a_word_rare() {
        assert_eq!(normalise("shepherd's,"), "shepherd");
        assert_eq!(normalise("“pedestrian”"), "pedestrian");
        assert_eq!(rarity("shepherd's"), rarity("shepherd"));
        assert!(rarity("pedestrian") > rarity("shepherd's"), "a rare real word beats a common possessive");
        assert!(rarity("Ishmael") > rarity("whale"), "unknown names are rarer than known words");
        assert!(rarity("circumambulate") > rarity("Ishmael"), "unknown common nouns rank above names");
        assert_eq!(rarity("1851"), 0);
        assert_eq!(rarity("and"), 0);
    }
}

//! 26 the word cursor: lands on the rarest word, moves by rarity or line, looks up,
//! grows a selection for a highlight.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use quire_library::marks::{Mark, MarkKind};

use crate::text::draw_right;
use crate::theme::*;
use crate::widgets::rail;
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

/// The 10 000 most common English words, most common first.
static COMMON: &str = include_str!("../../data/common-words.txt");

/// Rank of a word in the frequency list (0 = "the"); None when not in the list.
pub fn rank(word: &str) -> Option<u32> {
    let w = word.to_lowercase();
    COMMON.lines().position(|l| l == w).map(|p| p as u32)
}

/// Rarity score: higher is rarer. Unknown words are rarest; short words are common.
pub fn rarity(word: &str) -> u32 {
    let n = word.chars().count();
    if n < 4 || word.chars().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    match rank(word) {
        Some(r) => r + 1,
        None => 100_000 + n as u32,
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
    /// Position in the rarity order.
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
            let mut order: Vec<usize> = (0..self.words.len()).collect();
            order.sort_by(|a, b| rarity(&self.words[*b].0).cmp(&rarity(&self.words[*a].0)).then(a.cmp(b)));
            self.by_rarity = order;
            self.cur = self.by_rarity.first().copied().unwrap_or(0);
            self.rare_pos = 0;
        }
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
            self.rare_pos = self.by_rarity.iter().position(|x| *x == i).unwrap_or(0);
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
        // Counter "1/6" at the top right (rarity position), or "selection".
        let mono = quire_fonts::ui::mono();
        let label = if self.sel_end.is_some() {
            String::from("selection")
        } else {
            alloc::format!("{} / {}", self.rare_pos + 1, self.by_rarity.len().min(6))
        };
        let w = f.width() as i32;
        f.fill_rect(Rect::new(w - MARGIN - 90, MARGIN - 4, 90, 26), Ink::White);
        draw_right(f, mono, w - MARGIN - SPINE_W - 8, MARGIN + 14, &label, TextStyle::INK);
        if self.sel_end.is_some() {
            crate::widgets::side_labels(f, Some("Less"), Some("More"), true);
            rail(f, ["Less", "Cancel", "Save", "More"], None);
        } else {
            crate::widgets::side_labels(f, Some("Line"), Some("Line"), true);
            rail(f, ["Rarer", "Cancel", "Define", "Next"], None);
        }
        let _ = draw_text;
        Refresh::Du
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        self.ensure(cx);
        if self.words.is_empty() {
            return Action::Pop;
        }
        let n = self.words.len();
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
                let word = self.words[self.cur].0.clone();
                Action::Replace(Box::new(super::dictionary::Dictionary::new(word)))
            }
            (Key::Right, KeyKind::Press) | (Key::Right, KeyKind::Repeat) | (Key::Right, KeyKind::Long) => {
                if self.sel_end.is_some() {
                    self.sel_end = Some((self.sel_end.unwrap_or(self.cur) + 1).min(n - 1));
                } else {
                    self.rare_pos = (self.rare_pos + 1) % self.by_rarity.len().max(1);
                    self.cur = self.by_rarity[self.rare_pos];
                }
                Action::Redraw
            }
            (Key::Left, KeyKind::Press) | (Key::Left, KeyKind::Repeat) | (Key::Left, KeyKind::Long) => {
                if self.sel_end.is_some() {
                    let e = self.sel_end.unwrap_or(self.cur);
                    self.sel_end = Some(if e > self.cur { e - 1 } else { self.cur });
                } else {
                    self.rare_pos = if self.rare_pos == 0 { self.by_rarity.len().saturating_sub(1) } else { self.rare_pos - 1 };
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

//! 23 go to: the one screen that speaks percent; chapter and bookmark pickers; page.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use quire_library::marks::MarkKind;

use crate::text::{centered_baseline, draw_label, ellipsis, line_h};
use crate::theme::*;
use crate::widgets::{self, rail, running_head, setting_row, RowState, SettingValue};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

/// The Go-to screen.
pub struct GoTo {
    percent: u32,
    chapter: usize,
    bookmark: usize,
    focus: usize,
    init: bool,
}

impl GoTo {
    /// New.
    pub fn new() -> Self {
        GoTo { percent: 0, chapter: 0, bookmark: 0, focus: 0, init: false }
    }
}

impl Default for GoTo {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for GoTo {
    fn name(&self) -> &'static str {
        "23-goto"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let Some(r) = cx.reader.as_mut() else { return Refresh::Du };
        if !self.init {
            self.percent = r.info().permille / 10;
            self.chapter = r.toc_index().unwrap_or(0);
            self.init = true;
        }
        running_head(f, "Go to", None);
        let poster = quire_fonts::ui::poster();
        let fl = quire_fonts::ui::label();
        let mono = quire_fonts::ui::mono();
        let w = f.width() as i32;
        let mut y = widgets::CONTENT_TOP + 12;
        // Percent row with the poster numeral.
        let pr = Rect::new(0, y, w as u32, 96);
        let focused = self.focus == 0;
        if focused {
            f.fill_rect(pr, Ink::Black);
        }
        let style = TextStyle { inverted: focused, ..TextStyle::INK };
        draw_text(f, poster, widgets::INSET, y + 20 + poster.ascent(), &alloc::format!("{}%", self.percent), style);
        draw_text(
            f,
            fl,
            widgets::INSET,
            y + 20 + poster.ascent() + poster.descent() + 4 + fl.ascent(),
            "OF THE BOOK",
            TextStyle { tracking: 1, ..style },
        );
        let pages = alloc::format!("Page {} of {}", r.page_number(), r.total_pages());
        let pw = quire_gfx::measure_text(mono, &pages, style);
        draw_text(f, mono, w - widgets::INSET - pw, centered_baseline(mono, y, 96), &pages, style);
        draw_text(f, fl, w - widgets::INSET - quire_gfx::measure_text(fl, "‹ 1% ›", style), y + 24, "‹ 1% ›", style);
        y += 96;
        f.fill_rect(Rect::new(0, y, w as u32, 1), Ink::Black);
        y += 8;
        // Chapter stepper.
        let chapters: Vec<String> = r.book.toc.iter().map(|e| e.title.clone()).collect();
        let ch_text = if chapters.is_empty() { String::from("none") } else { alloc::format!("{}", self.chapter + 1) };
        setting_row(
            f,
            y,
            ROW_H,
            "Chapter",
            &SettingValue::Stepper(ch_text),
            if self.focus == 1 { RowState::Focused } else { RowState::Normal },
        );
        if let Some(t) = chapters.get(self.chapter) {
            let fs = quire_fonts::ui::label();
            draw_text(f, fs, ROW_PAD, y + ROW_H + fs.ascent() + 6, &ellipsis(fs, t, w - 2 * ROW_PAD), TextStyle::INK);
        }
        y += ROW_H + line_h(fl) + 12;
        // Bookmarks.
        let marks: Vec<&quire_library::marks::Mark> = r.marks.items.iter().filter(|m| m.kind == MarkKind::Bookmark).collect();
        let bm_text =
            if marks.is_empty() { String::from("none in this book") } else { alloc::format!("{} of {}", self.bookmark + 1, marks.len()) };
        setting_row(
            f,
            y,
            ROW_H,
            "Bookmark",
            &SettingValue::Stepper(bm_text),
            if self.focus == 2 { RowState::Focused } else { RowState::Normal },
        );
        if let Some(m) = marks.get(self.bookmark) {
            let fs = quire_fonts::ui::label();
            draw_text(f, fs, ROW_PAD, y + ROW_H + fs.ascent() + 6, &ellipsis(fs, &m.excerpt, w - 2 * ROW_PAD), TextStyle::INK);
        }
        y += ROW_H + line_h(fl) + 24;
        draw_label(f, widgets::INSET, y + fl.ascent(), "Hold Right on the page to skim instead.", false);
        rail(f, ["", "Back", "Go", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        let Some(r) = cx.reader.as_mut() else { return Action::Pop };
        let fs = cx.env.fs();
        let n_ch = r.book.toc.len();
        let n_bm = r.marks.items.iter().filter(|m| m.kind == MarkKind::Bookmark).count();
        match ev.key {
            Key::Back if ev.kind == KeyKind::Press => Action::Pop,
            Key::Up => {
                self.focus = (self.focus + 2) % 3;
                Action::Redraw
            }
            Key::Down => {
                self.focus = (self.focus + 1) % 3;
                Action::Redraw
            }
            Key::Left | Key::Right => {
                let step = if ev.kind == KeyKind::Press { 1 } else { 5 };
                let fwd = ev.key == Key::Right;
                match self.focus {
                    0 => self.percent = if fwd { (self.percent + step).min(100) } else { self.percent.saturating_sub(step) },
                    1 if n_ch > 0 => self.chapter = if fwd { (self.chapter + 1) % n_ch } else { (self.chapter + n_ch - 1) % n_ch },
                    2 if n_bm > 0 => self.bookmark = if fwd { (self.bookmark + 1) % n_bm } else { (self.bookmark + n_bm - 1) % n_bm },
                    _ => {}
                }
                Action::Redraw
            }
            Key::Confirm if ev.kind == KeyKind::Press => {
                match self.focus {
                    0 => {
                        let total = r.book.total_chars() as u64;
                        r.goto_chars(fs, (total * self.percent as u64 / 100) as u32);
                    }
                    1 => {
                        if let Some(loc) = r.book.toc_target(self.chapter) {
                            r.goto(fs, loc);
                        }
                    }
                    _ => {
                        let marks: Vec<quire_library::Loc> = r
                            .marks
                            .items
                            .iter()
                            .filter(|m| m.kind == MarkKind::Bookmark)
                            .map(|m| quire_library::Loc { section: m.section, pos: m.pos, chars: m.chars })
                            .collect();
                        if let Some(loc) = marks.get(self.bookmark) {
                            r.goto(fs, *loc);
                        }
                    }
                }
                r.force_gc();
                Action::ToReader
            }
            _ => Action::None,
        }
    }
}

//! 22 contents: nested list, current chapter marked, time-to-read per row, the Spine beside.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use quire_library::time::fmt_duration;

use crate::icons::{self, Icon};
use crate::spine;
use crate::text::{centered_baseline, ellipsis, page_indicator};
use crate::theme::*;
use crate::widgets::{self, rail, running_head, ListNav};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

/// The contents screen.
pub struct Contents {
    nav: ListNav,
    ready: bool,
}

impl Contents {
    /// New.
    pub fn new() -> Self {
        Contents { nav: ListNav::new(0, 1), ready: false }
    }
}

impl Default for Contents {
    fn default() -> Self {
        Self::new()
    }
}

/// Rows: (title, depth, seconds to read, is current, toc index).
fn rows<E: Env>(cx: &mut Ctx<E>) -> Vec<(String, u8, u32, bool, usize)> {
    let Some(r) = cx.reader.as_mut() else { return Vec::new() };
    let cur = r.toc_index();
    let entry = cx.lib.get(r.id);
    let total = r.book.total_chars();
    let mut out = Vec::new();
    for (i, e) in r.book.toc.iter().enumerate() {
        let from = r.book.toc_locs.get(i).map(|l| l.chars).unwrap_or(0);
        let to = r.book.toc_locs.iter().map(|l| l.chars).filter(|c| *c > from).min().unwrap_or(total);
        let secs = entry.map(|b| cx.stats.secs_for_chars(b, to.saturating_sub(from))).unwrap_or(0);
        out.push((e.title.clone(), e.depth, secs, cur == Some(i), i));
    }
    if out.is_empty() {
        // No TOC: one row per ingest chapter.
        let mut seen = Vec::new();
        for (si, s) in r.book.sections.iter().enumerate() {
            if s.part != 0 || seen.contains(&s.source) {
                continue;
            }
            seen.push(s.source);
            let title = s.title.clone().unwrap_or_else(|| alloc::format!("Section {}", s.source + 1));
            let (from, to, _) = r.book.chapter_bounds(r.book.chars_before_section(si as u16));
            let secs = entry.map(|b| cx.stats.secs_for_chars(b, to.saturating_sub(from))).unwrap_or(0);
            out.push((title, 0, secs, r.section == si as u16, si));
        }
    }
    out
}

impl<E: Env> Screen<E> for Contents {
    fn name(&self) -> &'static str {
        "22-contents"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let rows = rows(cx);
        let row_h = cx.settings.row_h();
        let top = widgets::CONTENT_TOP;
        let per = widgets::rows_between(top, f.height() as i32 - RAIL_H, row_h);
        if !self.ready {
            self.nav = ListNav::new(rows.len(), per);
            if let Some(i) = rows.iter().position(|r| r.3) {
                self.nav.focus = i;
            }
            self.ready = true;
        } else {
            self.nav.per_page = per;
            self.nav.set_n(rows.len());
        }
        running_head(f, "Contents", Some(&page_indicator(self.nav.page(), self.nav.pages())));
        let spine_w = SPINE_W + 12;
        let w = f.width() as i32 - spine_w - 8;
        let font = quire_fonts::ui::list_title();
        let mono = quire_fonts::ui::mono();
        let mut y = top;
        for i in self.nav.visible() {
            let (title, depth, secs, current, _) = &rows[i];
            let focused = i == self.nav.focus;
            let r = Rect::new(0, y, w as u32, row_h as u32);
            if focused {
                f.fill_rect(r, Ink::Black);
            }
            let style = TextStyle { inverted: focused, ..TextStyle::INK };
            let x = ROW_PAD + *depth as i32 * 24;
            let v = fmt_duration(*secs);
            let vw = quire_gfx::measure_text(mono, &v, style);
            draw_text(f, mono, w - ROW_PAD - vw, centered_baseline(mono, y, row_h), &v, style);
            let mut tx = x;
            if *current {
                icons::draw(f, Icon::BookmarkSet, x, y + (row_h - 24) / 2, if focused { Ink::White } else { Ink::Black });
                tx += 30;
            }
            draw_text(f, font, tx, centered_baseline(font, y, row_h), &ellipsis(font, title, w - ROW_PAD - vw - 16 - tx), style);
            f.fill_rect(Rect::new(0, y + row_h - 1, w as u32, 1), Ink::Black);
            y += row_h;
        }
        if let Some(reader) = cx.reader.as_mut() {
            let sr = Rect::new(
                f.width() as i32 - MARGIN + (MARGIN - SPINE_W) / 2,
                top,
                SPINE_W as u32,
                (f.height() as i32 - RAIL_H - top - 16) as u32,
            );
            spine::draw(f, sr, &reader.spine_model(), Ink::Black);
        }
        rail(f, ["", "Back", "Open", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        if ev.is(Key::Confirm) {
            let rows = rows(cx);
            let Some(row) = rows.get(self.nav.focus) else { return Action::Pop };
            let fs = cx.env.fs();
            let now = cx.env.now();
            if let Some(r) = cx.reader.as_mut() {
                if r.book.toc.is_empty() {
                    r.goto_section(fs, row.4 as u16);
                } else if let Some(loc) = r.book.toc_target(row.4) {
                    r.goto(fs, loc);
                }
                r.force_gc();
                let _ = now;
            }
            return Action::ToReader;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
}

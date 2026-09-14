//! 2A end of book: the poster page, rating, next in series.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use quire_library::time::fmt_duration;

use crate::icons::{self, Icon};
use crate::text::{centered_baseline, draw_label, ellipsis, line_h};
use crate::theme::*;
use crate::widgets::{self, poster_tiles, rail, running_head};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

/// The end-of-book poster.
pub struct EndOfBook {
    /// 0 = stars, 1 = next in series, 2 = library.
    focus: usize,
    stars: u8,
}

impl EndOfBook {
    /// New.
    pub fn new() -> Self {
        EndOfBook { focus: 0, stars: 0 }
    }
}

impl Default for EndOfBook {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for EndOfBook {
    fn name(&self) -> &'static str {
        "2A-endofbook"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let Some(id) = cx.reader.as_ref().map(|r| r.id) else { return Refresh::Du };
        let Some(e) = cx.lib.get(id).cloned() else { return Refresh::Du };
        if self.stars == 0 {
            self.stars = e.stats.rating;
        }
        running_head(f, "Finished", None);
        let w = f.width() as i32;
        let ft = quire_fonts::ui::title();
        let fb = quire_fonts::ui::body();
        let fl = quire_fonts::ui::label();
        let mut y = widgets::CONTENT_TOP;
        draw_text(f, ft, widgets::INSET, y + ft.ascent(), &ellipsis(ft, &e.title, w - 2 * widgets::INSET), TextStyle::INK);
        y += ft.ascent() + ft.below() + 4;
        draw_text(f, fb, widgets::INSET, y + fb.ascent(), &ellipsis(fb, &e.author_line(), w - 2 * widgets::INSET), TextStyle::INK);
        y += line_h(fb) + 16;
        let days = match (e.stats.started, e.stats.finished) {
            (Some(a), Some(b)) => alloc::format!("{} days", (b.saturating_sub(a) + 1).max(1)),
            _ => String::from("—"),
        };
        let pph = if e.stats.seconds >= 60 {
            alloc::format!("{}", e.stats.pages as u64 * 3600 / e.stats.seconds as u64)
        } else {
            String::from("—")
        };
        let tiles = alloc::vec![
            (fmt_duration(e.stats.seconds), String::from("Reading time")),
            (days, String::from("Start to finish")),
            (pph, String::from("Pages / hour")),
            (alloc::format!("{}", e.stats.sessions), String::from("Sessions")),
        ];
        y = poster_tiles(f, widgets::INSET, y, w - 2 * widgets::INSET, &tiles, 2) + 20;
        // Rate it.
        draw_label(f, widgets::INSET, y + fl.ascent(), "Rate it", false);
        y += line_h(fl) + 6;
        for i in 0..5u8 {
            let x = widgets::INSET + i as i32 * 40;
            let filled = i < self.stars;
            if self.focus == 0 && i + 1 == self.stars.max(1) {
                f.fill_rect(Rect::new(x - 6, y - 2, 36, 36), Ink::Black);
                icons::draw(f, if filled { Icon::StarFilled } else { Icon::Star }, x, y + 4, Ink::White);
            } else {
                icons::draw(f, if filled { Icon::StarFilled } else { Icon::Star }, x, y + 4, Ink::Black);
            }
        }
        y += 56;
        // Next in series.
        let next = cx.lib.next_in_series(id).map(|n| (n.id, n.title.clone(), cx.stats.time_left_secs(n)));
        let rows: Vec<(String, String)> = match &next {
            Some((_, t, secs)) => {
                alloc::vec![(alloc::format!("Next in series · {t}"), fmt_duration(*secs)), (String::from("Library"), String::new())]
            }
            None => alloc::vec![(String::from("Library"), String::new())],
        };
        for (i, (t, v)) in rows.iter().enumerate() {
            let focused = self.focus == i + 1;
            widgets::row(
                f,
                y,
                ROW_H,
                t,
                None,
                if v.is_empty() { None } else { Some(v) },
                if focused { widgets::RowState::Focused } else { widgets::RowState::Normal },
            );
            y += ROW_H;
        }
        let _ = centered_baseline(fb, 0, 1);
        rail(f, ["Back", "Library", "Next book", "Rate"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        let Some(id) = cx.reader.as_ref().map(|r| r.id) else { return Action::Pop };
        let has_next = cx.lib.next_in_series(id).map(|n| n.id);
        let rows = if has_next.is_some() { 2 } else { 1 };
        match ev.key {
            Key::Back => Action::Pop,
            Key::Left => Action::Replace(Box::new(super::library::LibraryScreen::new())),
            Key::Right => match has_next {
                Some(n) => Action::Open(n),
                None => Action::Replace(Box::new(super::library::LibraryScreen::new())),
            },
            Key::Up => {
                self.focus = (self.focus + rows) % (rows + 1);
                Action::Redraw
            }
            Key::Down => {
                self.focus = (self.focus + 1) % (rows + 1);
                Action::Redraw
            }
            Key::Confirm => match self.focus {
                0 => {
                    self.stars = if self.stars >= 5 { 1 } else { self.stars + 1 };
                    if let Some(e) = cx.lib.get_mut(id) {
                        e.stats.rating = self.stars;
                    }
                    cx.lib.touch();
                    Action::Redraw
                }
                1 if has_next.is_some() => Action::Open(has_next.unwrap_or(id)),
                _ => Action::Replace(Box::new(super::library::LibraryScreen::new())),
            },
            Key::Power => Action::None,
        }
    }
}

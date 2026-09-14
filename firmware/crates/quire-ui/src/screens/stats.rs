//! 60 analytics: overview, rhythm, calendar, books, goals; 61 year in review.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use quire_library::stats::Range;
use quire_library::time::{self, fmt_duration};
use quire_library::{cache, Status};

use crate::spine;
use crate::text::{centered_baseline, draw_centered, draw_label, draw_right, ellipsis, line_h, small_caps};
use crate::theme::*;
use crate::widgets::{self, empty_state, ink_line, poster_tiles, rail, running_head, setting_row, tabs, ListNav, RowState, SettingValue};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

const RANGES: [Range; 5] = [Range::Today, Range::Week, Range::Month, Range::Year, Range::All];
const RANGE_NAMES: [&str; 5] = ["Today", "Week", "Month", "Year", "All"];

// ---------------------------------------------------------------------------------------
// 60a overview

/// The overview.
pub struct Overview {
    range: usize,
}

impl Overview {
    /// New.
    pub fn new() -> Self {
        Overview { range: 0 }
    }
}

impl Default for Overview {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Overview {
    fn name(&self) -> &'static str {
        "60a-overview"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Reading", None);
        let w = f.width() as i32;
        let today = cx.today();
        let now = cx.env.now();
        let range = RANGES[self.range];
        let y = tabs(f, widgets::CONTENT_TOP - 8, &RANGE_NAMES, self.range, false, None);
        let mut t = cx.stats.totals(range, today);
        // Include the session in progress.
        if let Some(r) = cx.reader.as_ref() {
            t.secs += r.session_secs(now);
        }
        if t.secs == 0 && range == Range::Today {
            let msg = match cx.reader.as_mut() {
                Some(r) => {
                    let (ch, _) = r.time_left(cx.lib, cx.stats);
                    alloc::format!("{} is {} from the next chapter mark.", r.book.meta.title, fmt_duration(ch))
                }
                None => String::from("Open a book to start today's reading."),
            };
            empty_state(f, y + 120, "No reading yet today", &msg);
            widgets::side_labels(f, Some("Books"), Some("Goals"), true);
            rail(f, ["", "Back", "Continue", ""], None);
            return Refresh::Gc;
        }
        let (streak, _) = cx.stats.streaks(today);
        let (from, to) = quire_library::Stats::range_days(range, today);
        let finished = quire_library::Stats::books_finished_between(cx.lib, from, to);
        let left = cx
            .reader
            .as_ref()
            .and_then(|r| cx.lib.get(r.id))
            .map(|b| fmt_duration(cx.stats.time_left_secs(b)))
            .unwrap_or_else(|| String::from("—"));
        let period = match range {
            Range::Today => "Books today",
            Range::Week => "Books this week",
            Range::Month => "Books this month",
            Range::Year => "Books this year",
            Range::All => "Books finished",
        };
        let tiles = alloc::vec![
            (fmt_duration(t.secs), String::from("Read")),
            (alloc::format!("{}", t.pages), String::from("Pages")),
            (alloc::format!("{}", t.pages_per_hour()), String::from("Pages / hour")),
            (alloc::format!("{streak}"), String::from("Streak days")),
            (alloc::format!("{finished}"), String::from(period)),
            (left, String::from("Left in book")),
        ];
        let mut yy = poster_tiles(f, widgets::INSET, y + 8, w - 2 * widgets::INSET, &tiles, 2) + 16;
        // The ink line.
        let (values, labels, unit): (Vec<u32>, (&str, &str, &str), &str) = match range {
            Range::Today => {
                let curve = cx.stats.day_curve(cx.env.fs(), today);
                (curve.iter().map(|m| *m as u32).collect(), ("0 h", "12 h", "23 h"), " min")
            }
            Range::Week => (cx.stats.day_secs_between(from, to).iter().map(|s| s / 60).collect(), ("7 days ago", "", "today"), " min"),
            Range::Month => (cx.stats.day_secs_between(from, to).iter().map(|s| s / 60).collect(), ("1", "", &format_day(to)), " min"),
            Range::Year => {
                (cx.stats.month_totals(time::civil(today).0).iter().map(|t| t.secs / 3600).collect(), ("Jan", "Jul", "Dec"), " h")
            }
            Range::All => {
                let (y0, _, _) = time::civil(cx.stats.days.first().map(|d| d.day).unwrap_or(today));
                let (y1, _, _) = time::civil(today);
                let vals: Vec<u32> = (y0..=y1)
                    .map(|y| cx.stats.totals_between(time::from_civil(y, 1, 1), time::from_civil(y, 12, 31)).secs / 3600)
                    .collect();
                (vals, ("", "", ""), " h")
            }
        };
        let chart_h = (f.height() as i32 - RAIL_H - yy - 12).clamp(80, 150);
        ink_line(f, Rect::new(widgets::INSET, yy, (w - 2 * widgets::INSET) as u32, chart_h as u32), &values, labels, unit, None);
        yy += chart_h;
        let _ = yy;
        widgets::side_labels(f, Some("Books"), Some("Goals"), true);
        rail(f, ["Rhythm", "Back", "Calendar", "Books"], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Left if ev.kind == KeyKind::Long => Action::Push(Box::new(Rhythm::new())),
            Key::Left => {
                self.range = (self.range + 4) % 5;
                Action::Redraw
            }
            Key::Right if ev.kind == KeyKind::Long => Action::Push(Box::new(Books::new())),
            Key::Right => {
                self.range = (self.range + 1) % 5;
                Action::Redraw
            }
            Key::Confirm => Action::Push(Box::new(Calendar::new())),
            Key::Up => Action::Push(Box::new(Books::new())),
            Key::Down => Action::Push(Box::new(Goals::new())),
            Key::Power => Action::None,
        }
    }
}

fn format_day(day: u16) -> String {
    let (_, _, d) = time::civil(day);
    alloc::format!("{d}")
}

// ---------------------------------------------------------------------------------------
// 60b rhythm

/// When you read: histograms and the session list for a day.
pub struct Rhythm {
    day_offset: u16,
}

impl Rhythm {
    /// New.
    pub fn new() -> Self {
        Rhythm { day_offset: 0 }
    }
}

impl Default for Rhythm {
    fn default() -> Self {
        Self::new()
    }
}

/// A 1-bit histogram with the favourite column inverted and labels beneath in small caps.
fn histogram(f: &mut Frame, r: Rect, values: &[u32], labels: &[(usize, &str)]) {
    let n = values.len().max(1) as i32;
    let fl = quire_fonts::ui::label();
    let chart_h = r.h as i32 - line_h(fl) - 6;
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let gap = 2;
    let bw = ((r.w as i32 - gap * (n - 1)) / n).max(4);
    let base = r.y + chart_h;
    f.fill_rect(Rect::new(r.x, base, r.w, 2), Ink::Black);
    let fav = values.iter().enumerate().max_by_key(|(_, v)| **v).map(|(i, _)| i).unwrap_or(0);
    for (i, v) in values.iter().enumerate() {
        let h = ((*v as u64 * (chart_h as u64 - 2)) / max as u64) as i32;
        let x = r.x + i as i32 * (bw + gap);
        if h > 0 {
            f.fill_rect(Rect::new(x, base - h, bw as u32, h as u32), Ink::Black);
        }
        if i == fav && *v > 0 {
            f.invert_rect(Rect::new(x, r.y, bw as u32, chart_h as u32));
        }
    }
    for (i, l) in labels {
        let x = r.x + *i as i32 * (bw + gap) + bw / 2;
        draw_centered(f, fl, x, base + 6 + fl.ascent(), &small_caps(l), crate::text::label_style(false));
    }
}

impl<E: Env> Screen<E> for Rhythm {
    fn name(&self) -> &'static str {
        "60b-rhythm"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Rhythm", None);
        let w = f.width() as i32;
        let x = widgets::INSET;
        let cw = w - 2 * x;
        let fl = quire_fonts::ui::label();
        let mut y = widgets::CONTENT_TOP;
        draw_label(f, x, y + fl.ascent(), "Time of day · all time", false);
        y += line_h(fl) + 4;
        let hours: Vec<u32> = cx.stats.hours.iter().map(|s| s / 60).collect();
        histogram(f, Rect::new(x, y, cw as u32, 110), &hours, &[(0, "midnight"), (12, "noon"), (21, "9 pm")]);
        y += 120;
        draw_label(f, x, y + fl.ascent(), "Weekday", false);
        y += line_h(fl) + 4;
        let wd: Vec<u32> = cx.stats.weekdays.iter().map(|s| s / 60).collect();
        histogram(f, Rect::new(x, y, cw as u32, 90), &wd, &[(0, "M"), (1, "T"), (2, "W"), (3, "T"), (4, "F"), (5, "S"), (6, "S")]);
        y += 100;
        let tiles = alloc::vec![
            (cx.stats.favourite_hour().map(time::fmt_hour).unwrap_or_else(|| String::from("—")), String::from("Favourite hour")),
            (
                cx.stats.favourite_weekday().map(|d| String::from(time::weekday_name(d))).unwrap_or_else(|| String::from("—")),
                String::from("Favourite day")
            ),
            (fmt_duration(cx.stats.typical_session_secs()), String::from("Typical session")),
        ];
        y = poster_tiles(f, x, y, cw, &tiles, 3) + 12;
        // Session list for the selected day.
        let today = cx.today();
        let day = today.saturating_sub(self.day_offset);
        draw_label(f, x, y + fl.ascent(), &time::fmt_weekday_date(day), false);
        y += line_h(fl) + 4;
        let sessions = cx.stats.sessions_between(cx.env.fs(), day as u32 * time::DAY, (day as u32 + 1) * time::DAY, 6);
        let mono = quire_fonts::ui::mono();
        if sessions.is_empty() {
            draw_text(f, fl, x, y + fl.ascent(), "No sessions that day.", TextStyle::INK);
        }
        for s in sessions {
            let title = cx.lib.get(s.book).map(|b| b.title.clone()).unwrap_or_else(|| String::from("a book"));
            let line =
                alloc::format!("{} · {} · {}", time::fmt_clock(s.start, cx.settings.clock_24h), fmt_duration(s.active_secs()), title);
            draw_text(f, mono, x, y + mono.ascent(), &ellipsis(mono, &line, cw), TextStyle::INK);
            y += line_h(mono);
            if y > f.height() as i32 - RAIL_H - 8 {
                break;
            }
        }
        rail(f, ["Earlier", "Back", "Day", "Later"], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Left => {
                self.day_offset = self.day_offset.saturating_add(1);
                Action::Redraw
            }
            Key::Right => {
                self.day_offset = self.day_offset.saturating_sub(1);
                Action::Redraw
            }
            Key::Confirm => {
                self.day_offset = 0;
                Action::Redraw
            }
            _ => Action::None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// 60c calendar

/// The month heat map.
pub struct Calendar {
    /// Months back from today.
    back: u16,
}

impl Calendar {
    /// New.
    pub fn new() -> Self {
        Calendar { back: 0 }
    }
}

impl Default for Calendar {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Calendar {
    fn name(&self) -> &'static str {
        "60c-calendar"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let today = cx.today();
        let (mut y0, mut m0, _) = time::civil(today);
        for _ in 0..self.back {
            if m0 == 1 {
                m0 = 12;
                y0 -= 1;
            } else {
                m0 -= 1;
            }
        }
        let title = alloc::format!("{} ‹ {y0} ›", time::month_name_long(m0));
        running_head(f, &title, None);
        let w = f.width() as i32;
        let x0 = widgets::INSET;
        let cell = 56;
        let gap = 4;
        let fl = quire_fonts::ui::label();
        let mono = quire_fonts::ui::mono();
        let mut y = widgets::CONTENT_TOP;
        for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
            draw_centered(f, fl, x0 + i as i32 * (cell + gap) + cell / 2, y + fl.ascent(), d, crate::text::label_style(false));
        }
        y += line_h(fl) + 4;
        let first = time::from_civil(y0, m0, 1);
        let n = time::days_in_month(y0, m0) as u16;
        let start_col = time::weekday(first) as i32;
        let month = cx.stats.month(y0, m0);
        let mut streak_days: Vec<u16> = Vec::new();
        for i in 0..n {
            let day = first + i;
            let col = (start_col + i as i32) % 7;
            let row = (start_col + i as i32) / 7;
            let r = Rect::new(x0 + col * (cell + gap), y + row * (cell + gap), cell as u32, cell as u32);
            let frac = cx.stats.goal_fraction(day);
            widgets::heat_fill(f, r, frac);
            if month.get(i as usize).map(|m| m.1 >= quire_library::stats::STREAK_MIN_SECS).unwrap_or(false) {
                streak_days.push(day);
            }
            let inv = frac >= 88;
            draw_text(
                f,
                mono,
                r.x + 6,
                r.y + 6 + mono.ascent(),
                &alloc::format!("{}", i + 1),
                TextStyle { inverted: inv, ..TextStyle::INK },
            );
            if day == today {
                f.stroke_rect(r, 3, Ink::Black);
            }
        }
        // Streak rule through centres of consecutive streak days.
        for pair in streak_days.windows(2) {
            if pair[1] == pair[0] + 1 {
                let i0 = (pair[0] - first) as i32 + start_col;
                let i1 = (pair[1] - first) as i32 + start_col;
                if i0 / 7 == i1 / 7 {
                    let cy = y + (i0 / 7) * (cell + gap) + cell / 2;
                    let cx0 = x0 + (i0 % 7) * (cell + gap) + cell / 2;
                    let cx1 = x0 + (i1 % 7) * (cell + gap) + cell / 2;
                    f.invert_rect(Rect::new(cx0, cy - 1, (cx1 - cx0) as u32, 2));
                }
            }
        }
        let rows = (start_col + n as i32 + 6) / 7;
        y += rows * (cell + gap) + 12;
        let (cur, longest) = cx.stats.streaks(today);
        let days_so_far =
            if self.back == 0 { month.iter().filter(|(d, _)| (*d as u16) <= time::civil(today).2 as u16).count() } else { n as usize };
        let days_read = month.iter().filter(|(_, s)| *s >= quire_library::stats::STREAK_MIN_SECS).count();
        let tiles = alloc::vec![
            (alloc::format!("{cur}"), String::from("Current streak")),
            (alloc::format!("{longest}"), String::from("Longest")),
            (alloc::format!("{days_read} / {days_so_far}"), String::from("Days so far")),
        ];
        y = poster_tiles(f, x0, y, w - 2 * x0, &tiles, 3) + 8;
        draw_text(f, fl, x0, y + fl.ascent(), "empty · sparse · dense · solid = 0 / 25 / 50 / 100% of the daily goal", TextStyle::INK);
        rail(f, ["Earlier", "Back", "Today", "Later"], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Left => {
                self.back = self.back.saturating_add(1).min(120);
                Action::Redraw
            }
            Key::Right => {
                self.back = self.back.saturating_sub(1);
                Action::Redraw
            }
            Key::Confirm => {
                self.back = 0;
                Action::Redraw
            }
            _ => Action::None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// 60d books

/// A table of every book: time, pages, pages per hour, started, finished.
pub struct Books {
    nav: ListNav,
    sort: usize,
}

impl Books {
    /// New.
    pub fn new() -> Self {
        Books { nav: ListNav::new(0, 10), sort: 1 }
    }
}

impl Default for Books {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Books {
    fn name(&self) -> &'static str {
        "60d-books"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Books", Some(&crate::text::page_indicator(self.nav.page(), self.nav.pages())));
        let w = f.width() as i32;
        let mut books: Vec<&quire_library::BookEntry> =
            cx.lib.books.iter().filter(|b| b.stats.seconds > 0 || b.status == Status::Finished).collect();
        match self.sort {
            0 => books.sort_by_key(|b| quire_library::index::sort_title(&b.title)),
            1 => books.sort_by_key(|b| core::cmp::Reverse(b.stats.seconds)),
            2 => books.sort_by_key(|b| core::cmp::Reverse(b.stats.pages)),
            _ => books.sort_by_key(|b| core::cmp::Reverse(pph(b))),
        }
        let row_h = 40;
        let top = widgets::CONTENT_TOP + 32;
        self.nav.per_page = widgets::rows_between(top, f.height() as i32 - RAIL_H, row_h);
        self.nav.set_n(books.len());
        // Table head.
        let fl = quire_fonts::ui::label();
        let mono = quire_fonts::ui::mono();
        let cols = [(widgets::INSET, "Title"), (w - 250, "Time"), (w - 160, "Pages"), (w - 80, "P/h")];
        for (i, (x, t)) in cols.iter().enumerate() {
            let inv = i == self.sort;
            if inv {
                let tw = quire_gfx::measure_text(fl, &small_caps(t), crate::text::label_style(true));
                f.fill_rect(Rect::new(x - 4, widgets::CONTENT_TOP, (tw + 8) as u32, 26), Ink::Black);
            }
            draw_text(f, fl, *x, widgets::CONTENT_TOP + 18, &small_caps(t), crate::text::label_style(inv));
        }
        f.fill_rect(Rect::new(widgets::INSET, widgets::CONTENT_TOP + 28, (w - 2 * widgets::INSET) as u32, RULE), Ink::Black);
        if books.is_empty() {
            empty_state(f, top + 100, "No reading yet", "Books you read appear here with their time and pace.");
        }
        let mut y = top;
        for i in self.nav.visible() {
            let b = books[i];
            let focused = i == self.nav.focus;
            if focused {
                f.fill_rect(Rect::new(0, y, w as u32, row_h as u32), Ink::Black);
            }
            let style = TextStyle { inverted: focused, ..TextStyle::INK };
            draw_text(
                f,
                quire_fonts::ui::body(),
                widgets::INSET,
                centered_baseline(quire_fonts::ui::body(), y, row_h),
                &ellipsis(quire_fonts::ui::body(), &b.title, w - 250 - widgets::INSET - 12),
                style,
            );
            draw_text(f, mono, w - 250, centered_baseline(mono, y, row_h), &fmt_duration(b.stats.seconds), style);
            draw_text(f, mono, w - 160, centered_baseline(mono, y, row_h), &alloc::format!("{}", b.stats.pages), style);
            draw_right(f, mono, w - widgets::INSET, centered_baseline(mono, y, row_h), &alloc::format!("{}", pph(b)), style);
            f.fill_rect(Rect::new(widgets::INSET, y + row_h - 1, (w - 2 * widgets::INSET) as u32, 1), Ink::Black);
            y += row_h;
        }
        rail(f, ["", "Back", "Open", "Sort"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        match ev.key {
            Key::Back if ev.kind == KeyKind::Press => Action::Pop,
            Key::Right if ev.kind == KeyKind::Press => {
                self.sort = (self.sort + 1) % 4;
                Action::Redraw
            }
            Key::Confirm if ev.kind == KeyKind::Long => {
                self.sort = (self.sort + 1) % 4;
                Action::Redraw
            }
            Key::Confirm => {
                let mut books: Vec<&quire_library::BookEntry> =
                    cx.lib.books.iter().filter(|b| b.stats.seconds > 0 || b.status == Status::Finished).collect();
                match self.sort {
                    0 => books.sort_by_key(|b| quire_library::index::sort_title(&b.title)),
                    1 => books.sort_by_key(|b| core::cmp::Reverse(b.stats.seconds)),
                    2 => books.sort_by_key(|b| core::cmp::Reverse(b.stats.pages)),
                    _ => books.sort_by_key(|b| core::cmp::Reverse(pph(b))),
                }
                match books.get(self.nav.focus) {
                    Some(b) => Action::Push(Box::new(super::bookinfo::BookInfo::new(b.id))),
                    None => Action::None,
                }
            }
            _ => {
                if self.nav.key(ev) {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
        }
    }
}

fn pph(b: &quire_library::BookEntry) -> u32 {
    if b.stats.seconds < 60 {
        0
    } else {
        (b.stats.pages as u64 * 3600 / b.stats.seconds as u64) as u32
    }
}

// ---------------------------------------------------------------------------------------
// 60e goals

/// Goals and awards.
pub struct Goals {
    focus: usize,
}

impl Goals {
    /// New.
    pub fn new() -> Self {
        Goals { focus: 0 }
    }
}

impl Default for Goals {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Goals {
    fn name(&self) -> &'static str {
        "60e-goals"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Goals", None);
        let w = f.width() as i32;
        let today = cx.today();
        let (year, _, _) = time::civil(today);
        let mut y = widgets::CONTENT_TOP;
        let st = &*cx.stats;
        let daily = if st.goal_pages { alloc::format!("{} pages", st.goal_page_count) } else { alloc::format!("{} min", st.goal_minutes) };
        let rows: [(&str, SettingValue); 3] = [
            ("Daily", SettingValue::Stepper(daily)),
            ("Count", SettingValue::Choice(String::from(if st.goal_pages { "pages" } else { "minutes" }))),
            ("Yearly", SettingValue::Stepper(alloc::format!("{} books", st.goal_books))),
        ];
        for (i, (t, v)) in rows.iter().enumerate() {
            setting_row(f, y, ROW_H, t, v, if self.focus == i { RowState::Focused } else { RowState::Normal });
            y += ROW_H;
        }
        y += 16;
        // Goal spine.
        let done = quire_library::Stats::books_finished_in_year(cx.lib, year);
        let goal = st.goal_books as u32;
        let sr = Rect::new(widgets::INSET, y, 24, 200);
        spine::draw_goal(f, sr, done, goal, Ink::Black);
        let poster = quire_fonts::ui::poster();
        let fl = quire_fonts::ui::label();
        draw_text(f, poster, sr.right() + 24, y + 24 + poster.ascent(), &alloc::format!("{done} / {goal}"), TextStyle::INK);
        draw_label(f, sr.right() + 24, y + 24 + poster.ascent() + poster.descent() + 4 + fl.ascent(), "Books this year", false);
        // Awards.
        let ax = sr.right() + 24;
        let mut ay = y + 100;
        draw_label(f, ax, ay + fl.ascent(), "Awards", false);
        ay += line_h(fl) + 4;
        let awards = st.awards(cx.lib);
        if awards.is_empty() {
            draw_text(f, fl, ax, ay + fl.ascent(), "Read a book to the end for the first one.", TextStyle::INK);
        }
        for a in awards.iter().take(6) {
            let line = alloc::format!("{} · {}", a.name, time::fmt_date(a.day));
            draw_text(
                f,
                quire_fonts::ui::body(),
                ax,
                ay + quire_fonts::ui::body().ascent(),
                &ellipsis(quire_fonts::ui::body(), &line, w - ax - widgets::INSET),
                TextStyle::INK,
            );
            ay += line_h(quire_fonts::ui::body());
        }
        rail(f, ["", "Back", "Change", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        let st = &mut *cx.stats;
        match ev.key {
            Key::Back => Action::Pop,
            Key::Up => {
                self.focus = (self.focus + 2) % 3;
                Action::Redraw
            }
            Key::Down => {
                self.focus = (self.focus + 1) % 3;
                Action::Redraw
            }
            Key::Left | Key::Right | Key::Confirm => {
                let dir: i32 = if ev.key == Key::Left { -1 } else { 1 };
                match self.focus {
                    0 => {
                        if st.goal_pages {
                            st.goal_page_count = (st.goal_page_count as i32 + 5 * dir).clamp(5, 500) as u16;
                        } else {
                            st.goal_minutes = (st.goal_minutes as i32 + 5 * dir).clamp(5, 600) as u16;
                        }
                    }
                    1 => st.goal_pages = !st.goal_pages,
                    _ => st.goal_books = (st.goal_books as i32 + dir).clamp(1, 365) as u16,
                }
                let _ = st.save(cx.env.fs());
                Action::Redraw
            }
            Key::Power => Action::None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// 61 year in review

/// The year poster.
pub struct YearInReview {
    year: u16,
    thumbs: Vec<quire_gfx::Bitmap>,
}

impl YearInReview {
    /// New, for this year.
    pub fn new() -> Self {
        YearInReview { year: 0, thumbs: Vec::new() }
    }
}

impl Default for YearInReview {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for YearInReview {
    fn name(&self) -> &'static str {
        "61-yearinreview"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let today = cx.today();
        if self.year == 0 {
            self.year = time::civil(today).0;
        }
        let y0 = self.year;
        let (a, b) = (time::from_civil(y0, 1, 1), time::from_civil(y0, 12, 31));
        let w = f.width() as i32;
        let hero = quire_fonts::ui::hero();
        let fl = quire_fonts::ui::label();
        draw_text(f, quire_fonts::ui::title(), widgets::INSET, 60, &alloc::format!("Quire · {y0}"), TextStyle::INK);
        f.fill_rect(Rect::new(widgets::INSET, 72, (w - 2 * widgets::INSET) as u32, RULE_HEAVY), Ink::Black);
        let books = cx.lib.finished_between(a, b);
        let totals = cx.stats.totals_between(a, b);
        let mut y = 110;
        draw_text(f, hero, widgets::INSET, y + hero.ascent(), &alloc::format!("{}", books.len()), TextStyle::INK);
        draw_label(f, widgets::INSET, y + hero.ascent() + hero.descent() + 6 + fl.ascent(), "Books", false);
        draw_text(f, hero, w / 2 + 20, y + hero.ascent(), &alloc::format!("{} h", totals.secs / 3600), TextStyle::INK);
        draw_label(f, w / 2 + 20, y + hero.ascent() + hero.descent() + 6 + fl.ascent(), "Read", false);
        y += hero.ascent() + hero.descent() + 6 + line_h(fl) + 24;
        let tiles = alloc::vec![
            (cx.stats.favourite_hour().map(time::fmt_hour).unwrap_or_else(|| String::from("—")), String::from("Favourite hour")),
            (alloc::format!("{}", cx.stats.longest_streak_in_year(y0)), String::from("Longest streak")),
        ];
        y = poster_tiles(f, widgets::INSET, y, w - 2 * widgets::INSET, &tiles, 2) + 20;
        // Five covers.
        if self.thumbs.is_empty() {
            for bk in books.iter().take(5) {
                if let Some(t) = cache::load_thumb(cx.env.fs(), bk.id) {
                    self.thumbs.push(t);
                }
            }
        }
        let cw = 76;
        let ch = 114;
        let mut x = widgets::INSET;
        for t in self.thumbs.iter().take(5) {
            let mut small = quire_gfx::Bitmap::new(cw, ch);
            for yy in 0..ch {
                for xx in 0..cw {
                    if t.get(xx * t.w / cw, yy * t.h / ch) {
                        small.set(xx, yy, true);
                    }
                }
            }
            f.blit(x, y, small.as_ref(), quire_gfx::BlitMode::Or);
            f.stroke_rect(Rect::new(x, y, cw, ch), 1, Ink::Black);
            x += cw as i32 + 16;
        }
        if !self.thumbs.is_empty() {
            y += ch as i32 + 24;
        }
        let fb = quire_fonts::ui::body();
        let longest = cx.stats.longest;
        if longest.0 > 0 {
            let title = cx.lib.get(longest.1).map(|b| b.title.clone()).unwrap_or_default();
            draw_text(
                f,
                fb,
                widgets::INSET,
                y + fb.ascent(),
                &ellipsis(fb, &alloc::format!("Longest session · {} · {title}", fmt_duration(longest.0)), w - 2 * widgets::INSET),
                TextStyle::INK,
            );
            y += line_h(fb);
        }
        let ls = cx.stats.longest_streak_in_year(y0);
        draw_text(f, fb, widgets::INSET, y + fb.ascent(), &alloc::format!("Longest streak · {ls} days"), TextStyle::INK);
        let foot = f.height() as i32 - RAIL_H - 16;
        draw_text(f, fl, widgets::INSET, foot, "Power + Down saves it", TextStyle::INK);
        rail(f, ["Earlier", "Back", "", "Later"], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Left => {
                self.year = self.year.saturating_sub(1);
                self.thumbs.clear();
                Action::Redraw
            }
            Key::Right => {
                self.year = self.year.saturating_add(1);
                self.thumbs.clear();
                Action::Redraw
            }
            _ => Action::None,
        }
    }
}

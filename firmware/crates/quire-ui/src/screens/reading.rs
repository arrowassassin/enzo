//! 20 the reading page, 21 the compass (and More), 10 the Home layer, 24 skim, 29 footnote.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use quire_library::time::fmt_duration;
use quire_library::Status;

use crate::spine;
use crate::text::{centered_baseline, draw_centered, draw_label, ellipsis, line_h, wrap};
use crate::theme::*;
use crate::widgets::{self, rail, RowState, SIDE_DOWN_Y, SIDE_H, SIDE_UP_Y};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen, SysRequest};

use super::{finish_by_line, left_line};

/// The reading page.
pub struct ReadingScreen {
    /// Skim in progress: pages turned while Right is held.
    skimming: bool,
    /// Frames since the last GC.
    idle_ticks: u8,
}

impl ReadingScreen {
    /// New.
    pub fn new() -> Self {
        ReadingScreen { skimming: false, idle_ticks: 0 }
    }
}

impl Default for ReadingScreen {
    fn default() -> Self {
        Self::new()
    }
}

/// Draw the current page from the reader into the frame, or the empty home.
pub fn draw_page<E: Env>(cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
    let fs = cx.env.fs();
    match cx.reader.as_mut() {
        Some(r) => {
            let info = r.render(fs, f, cx.settings);
            let gc = r.take_gc(cx.settings.gc_every_pages) || info.as_ref().map(|i| i.chapter_start).unwrap_or(false);
            if let Some(i) = &info {
                if i.last {
                    // The last page: a small "···  Right — finish" foot line.
                    let font = quire_fonts::ui::label();
                    let y = f.height() as i32 - MARGIN - 4;
                    draw_centered(f, font, f.width() as i32 / 2 - 20, y, "···   Right — finish", TextStyle::INK);
                }
            }
            if gc {
                Refresh::Gc
            } else {
                Refresh::Du
            }
        }
        None => {
            super::jump::draw_empty_home(cx, f);
            Refresh::Gc
        }
    }
}

impl<E: Env> Screen<E> for ReadingScreen {
    fn name(&self) -> &'static str {
        "20-reading"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let r = draw_page(cx, f);
        if self.skimming {
            if let Some(reader) = cx.reader.as_mut() {
                let m = reader.spine_model();
                let (_, book_secs) = reader.time_left(cx.lib, cx.stats);
                let l1 = spine::chapter_percent_label(reader.chapter_number(), m.permille());
                let l2 = alloc::format!("{} left", fmt_duration(book_secs));
                spine::draw_skim_label(f, reader.spine_rect(), &m, &l1, &l2);
            }
            return Refresh::Du;
        }
        r
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        let now = cx.env.now();
        if cx.reader.is_none() {
            // Nothing open: the empty home layer is the page.
            return match ev.key {
                Key::Back if ev.kind == KeyKind::Press => Action::Push(Box::new(HomeLayer::new())),
                Key::Confirm if ev.kind == KeyKind::Press => Action::Push(Box::new(super::library::LibraryScreen::new())),
                Key::Right if ev.kind == KeyKind::Press => Action::Push(Box::new(super::bookshop::BookshopHome::new())),
                Key::Left if ev.kind == KeyKind::Press => Action::Push(Box::new(super::library::LibraryScreen::new())),
                Key::Up if ev.kind == KeyKind::Press => Action::Push(Box::new(super::drop::DropScreen::new())),
                Key::Down if ev.kind == KeyKind::Press => Action::Push(Box::new(super::stats::Overview::new())),
                _ => Action::None,
            };
        }
        let side_chapters = cx.settings.side_keys == crate::settings::SideKeys::Chapters;
        let (up, down) = if cx.settings.swap_side_keys { (Key::Down, Key::Up) } else { (Key::Up, Key::Down) };
        let fs = cx.env.fs();
        let reader = cx.reader.as_mut().expect("reader");
        match (ev.key, ev.kind) {
            (Key::Right, KeyKind::Press) => {
                if self.skimming {
                    return Action::None;
                }
                if reader.at_end() {
                    reader.reached_end(cx.lib, quire_library::time::day_of(now));
                    return Action::Push(Box::new(super::endofbook::EndOfBook::new()));
                }
                reader.next_page(fs, now);
                Action::Redraw
            }
            (Key::Left, KeyKind::Press) => {
                reader.prev_page(fs, now);
                Action::Redraw
            }
            (k, KeyKind::Press) if k == down => {
                if side_chapters {
                    reader.next_chapter(fs, now);
                } else if !reader.next_page(fs, now) && reader.at_end() {
                    return Action::None;
                }
                Action::Redraw
            }
            (k, KeyKind::Press) if k == up => {
                if side_chapters {
                    reader.prev_chapter(fs, now);
                } else {
                    reader.prev_page(fs, now);
                }
                Action::Redraw
            }
            (Key::Right, KeyKind::Long) | (Key::Right, KeyKind::Repeat) => {
                // Hold Right: skim at the repeat cadence.
                self.skimming = true;
                reader.next_page(fs, now);
                Action::Redraw
            }
            (Key::Right, KeyKind::Release) => {
                if self.skimming {
                    self.skimming = false;
                    reader.force_gc();
                    return Action::Redraw;
                }
                Action::None
            }
            (k, KeyKind::Long) | (k, KeyKind::Repeat) if k == down => {
                reader.next_chapter(fs, now);
                Action::Redraw
            }
            (k, KeyKind::Long) | (k, KeyKind::Repeat) if k == up => {
                reader.prev_chapter(fs, now);
                Action::Redraw
            }
            (Key::Back, KeyKind::Press) => {
                if reader.has_return() {
                    reader.pop_return(fs);
                    return Action::Redraw;
                }
                Action::Push(Box::new(HomeLayer::new()))
            }
            (Key::Confirm, KeyKind::Press) => Action::Push(Box::new(Compass::new(false))),
            (Key::Confirm, KeyKind::Long) => Action::Push(Box::new(super::cursor::WordCursor::new())),
            _ => Action::None,
        }
    }
    fn event(&mut self, cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::Tick => {
                // Idle work: pre-render the next page, then fill the page index.
                let fs = cx.env.fs();
                if let Some(r) = cx.reader.as_mut() {
                    r.prerender(fs, cx.settings);
                    self.idle_ticks = self.idle_ticks.wrapping_add(1);
                    if self.idle_ticks.is_multiple_of(2) {
                        r.index_step(fs, cx.lib);
                    }
                }
                Action::None
            }
            Event::BooksChanged => Action::None,
            _ => Action::None,
        }
    }
    fn resume(&mut self, cx: &mut Ctx<E>) {
        if let Some(r) = cx.reader.as_mut() {
            r.force_gc();
        }
    }
}

// ---------------------------------------------------------------------------------------
// 21 compass

/// The compass: choices at the key positions over the lower 40 % of the page.
pub struct Compass {
    more: bool,
}

impl Compass {
    /// New (the first page; `more` = the second).
    pub fn new(more: bool) -> Self {
        Compass { more }
    }
}

/// Card geometry of the compass: the lower 40 %.
pub fn compass_card(f: &Frame) -> Rect {
    let h = f.height() as i32;
    let top = h * 60 / 100;
    Rect::new(0, top, f.width(), (h - top) as u32)
}

/// Draw a compass layout: a context line, four choices at the rail, two on the side.
#[allow(clippy::too_many_arguments)]
pub fn draw_compass(
    f: &mut Frame,
    context: &str,
    hold_hint: &str,
    cells: [(&str, &str); 4],
    up: &str,
    down: &str,
    focused_cell: Option<usize>,
) {
    let card = compass_card(f);
    widgets::screen(f, Rect::new(0, 0, f.width(), card.y as u32));
    f.fill_rect(card, Ink::White);
    f.fill_rect(Rect::new(0, card.y, card.w, RULE), Ink::Black);
    let lf = quire_fonts::ui::label();
    draw_text(f, lf, 28, card.y + 20 + lf.ascent(), &ellipsis(lf, context, card.w as i32 - 120), TextStyle::INK);
    // Side choices.
    draw_side_choices(f, up, down);
    // Hold hint centred in the free space.
    let hint_y = (card.y + 20 + line_h(lf) + (card.bottom() - 104)) / 2;
    let hint = ellipsis(lf, hold_hint, card.w as i32 - 88);
    draw_centered(f, lf, (card.w as i32 - 48) / 2, hint_y + lf.ascent() / 2, &hint, TextStyle::INK);
    // Choice cells: 104 px tall above the bottom edge.
    let cy = card.bottom() - 104;
    f.fill_rect(Rect::new(0, cy, card.w, RULE_HEAVY), Ink::Black);
    let cell = card.w as i32 / 4;
    // 26 px labels, or 22 px for the whole row when one of them will not fit its cell.
    let mut tf = quire_fonts::ui::list_title();
    if cells.iter().any(|(l, _)| quire_gfx::measure_text(tf, l, TextStyle::INK) > cell - 8) {
        tf = quire_fonts::ui::body();
    }
    for (i, (label, ctx)) in cells.iter().enumerate() {
        let x = i as i32 * cell;
        let r = Rect::new(x, cy + RULE_HEAVY as i32, cell as u32, (104 - RULE_HEAVY as i32) as u32);
        let inv = focused_cell == Some(i);
        if inv {
            f.fill_rect(r, Ink::Black);
        }
        if i < 3 {
            f.fill_rect(Rect::new(x + cell - 1, r.y, 1, r.h), Ink::Black);
        }
        let style = TextStyle { inverted: inv, ..TextStyle::INK };
        let cx = x + cell / 2;
        let base = r.y + 40;
        draw_centered(f, tf, cx, base, &ellipsis(tf, label, cell - 8), style);
        draw_centered(f, lf, cx, base + 30, &ellipsis(lf, ctx, cell - 8), style);
    }
}

/// The two rotated side choices (Up above Down), body text reading upwards with a tick at
/// the edge, spaced so they never run into each other or the choice cells.
fn draw_side_choices(f: &mut Frame, up: &str, down: &str) {
    let font = quire_fonts::ui::body();
    let th = line_h(font) + 2;
    let w = f.width() as i32;
    let x = w - 12 - th;
    let floor = f.height() as i32 - 104 - RULE_HEAVY as i32 - 6;
    let mut next_top = floor;
    for (label, y) in [(down, SIDE_DOWN_Y), (up, SIDE_UP_Y)] {
        if label.is_empty() {
            continue;
        }
        let tw = quire_gfx::measure_text(font, label, TextStyle::INK) + 8;
        let mut tmp = Frame::new(tw as u32, th as u32);
        draw_text(&mut tmp, font, 4, 1 + font.ascent(), label, TextStyle::INK);
        let rot = tmp.rotated(quire_gfx::Rotation::Ccw90);
        let centre = y + SIDE_H / 2;
        let top = (centre - tw / 2).min(next_top - tw).max(0);
        let r = Rect::new(x, top, th as u32, tw as u32);
        f.fill_rect(r, Ink::White);
        f.blit(x, top, rot.as_bitmap(), quire_gfx::BlitMode::Or);
        f.fill_rect(Rect::new(w - 4, centre - 12, 2, 24), Ink::Black);
        next_top = top - 12;
    }
}

impl<E: Env> Screen<E> for Compass {
    fn name(&self) -> &'static str {
        if self.more {
            "21-compass-more"
        } else {
            "21-compass"
        }
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let Some(reader) = cx.reader.as_mut() else { return Refresh::Du };
        let title = reader.book.meta.title.clone();
        let chapter = reader.chapter_title();
        let (chapter_secs, _) = reader.time_left(cx.lib, cx.stats);
        let page = reader.page_number();
        let total = reader.total_pages();
        let permille = reader.info().permille;
        let bookmarked = reader.bookmarked();
        let ch_no = reader.chapter_number();
        let ch_count = reader.chapter_count();
        let lf = quire_fonts::ui::label();
        let ctx_w = f.width() as i32 - 120;
        let full = if chapter.is_empty() {
            alloc::format!("{title} · p {page} of {total}")
        } else {
            alloc::format!("{title} · {chapter} · p {page} of {total}")
        };
        // When the whole line will not fit, the chapter and page matter more than the title.
        let context = if quire_gfx::measure_text(lf, &full, TextStyle::INK) <= ctx_w || chapter.is_empty() {
            full
        } else {
            alloc::format!("{chapter} · p {page} of {total}")
        };
        if self.more {
            let words = reader.page_words();
            let rare = super::cursor::rarest_word(&words).unwrap_or_default();
            let hl = reader.marks.items.iter().filter(|m| m.kind != quire_library::marks::MarkKind::Bookmark).count();
            let today_secs = cx.stats.totals(quire_library::stats::Range::Today, quire_library::time::day_of(cx.env.now())).secs
                + reader.session_secs(cx.env.now());
            let hl_text = alloc::format!("{hl} in book");
            let stats_text = fmt_duration(today_secs);
            draw_compass(
                f,
                &context,
                "hold Confirm — bookmark this page",
                [("Dictionary", &rare), ("Close", "—"), ("Stats", &stats_text), ("Highlights", &hl_text)],
                "Layout",
                "Sleep",
                None,
            );
        } else {
            let contents_ctx = match (ch_no, ch_count) {
                (Some(n), c) if c > 0 => alloc::format!("Ch {n} of {c}"),
                _ => alloc::format!("{} left", fmt_duration(chapter_secs)),
            };
            let goto_ctx = alloc::format!("{}% · p {page}", permille / 10);
            let bm = if bookmarked { "Bookmarked" } else { "Bookmark" };
            let bm_ctx = if bookmarked { "set" } else { "not set" };
            draw_compass(
                f,
                &context,
                "hold Confirm — bookmark this page",
                [("Contents", &contents_ctx), ("Close", "—"), (bm, bm_ctx), ("Go to", &goto_ctx)],
                "Type",
                "More",
                None,
            );
        }
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        let now = cx.env.now();
        if ev.kind == KeyKind::Long && ev.key == Key::Confirm {
            if let Some(r) = cx.reader.as_mut() {
                r.toggle_bookmark(cx.env.fs(), now);
            }
            return Action::Redraw;
        }
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        if self.more {
            return match ev.key {
                Key::Back => Action::Pop,
                Key::Left => Action::Replace(Box::new(super::cursor::WordCursor::new())),
                Key::Right => Action::Replace(Box::new(super::highlights::Highlights::new())),
                Key::Confirm => Action::Replace(Box::new(super::stats::Overview::new())),
                Key::Up => Action::Replace(Box::new(super::typeset::LayoutScreen::new())),
                Key::Down => {
                    if let Some(r) = cx.reader.as_mut() {
                        r.force_gc();
                    }
                    Action::Replace(Box::new(super::sleep::SleepScreen::new()))
                }
                Key::Power => Action::None,
            };
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Left => Action::Replace(Box::new(super::contents::Contents::new())),
            Key::Right => Action::Replace(Box::new(super::goto::GoTo::new())),
            Key::Confirm => {
                if let Some(r) = cx.reader.as_mut() {
                    r.toggle_bookmark(cx.env.fs(), now);
                }
                Action::Pop
            }
            Key::Up => Action::Replace(Box::new(super::typeset::TypeScreen::new())),
            Key::Down => Action::Replace(Box::new(Compass::new(true))),
            Key::Power => Action::None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// 10 home layer

/// The Home layer over the page: the book, time left, three recent books, the rail.
pub struct HomeLayer {
    focus: usize,
}

impl HomeLayer {
    /// New.
    pub fn new() -> Self {
        HomeLayer { focus: 0 }
    }
}

impl Default for HomeLayer {
    fn default() -> Self {
        Self::new()
    }
}

/// The Home layer's card: the upper 60 %.
pub fn home_card(f: &Frame) -> Rect {
    let h = f.height() as i32 * 60 / 100;
    Rect::new(0, 0, f.width(), h as u32)
}

impl<E: Env> Screen<E> for HomeLayer {
    fn name(&self) -> &'static str {
        "10-home"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let card = home_card(f);
        widgets::screen(f, Rect::new(0, card.bottom(), f.width(), (f.height() as i32 - card.bottom() - RAIL_H) as u32));
        f.fill_rect(card, Ink::White);
        f.fill_rect(Rect::new(0, card.bottom() - RULE as i32, card.w, RULE), Ink::Black);
        let now = cx.env.now();
        let today = quire_library::time::day_of(now);
        let x = 32;
        let w = f.width() as i32 - 64;
        let mut y = 40;
        let ft = quire_fonts::ui::title();
        let fb = quire_fonts::ui::body();
        let fl = quire_fonts::ui::label();
        let mut recent: Vec<(String, String)> = Vec::new();
        match cx.reader.as_mut() {
            Some(r) => {
                draw_text(f, ft, x, y + ft.ascent(), &ellipsis(ft, &r.book.meta.title, w), TextStyle::INK);
                y += ft.ascent() + ft.below() + 4;
                let author = cx.lib.get(r.id).map(|e| e.author_line()).unwrap_or_default();
                draw_text(f, fb, x, y + fb.ascent(), &ellipsis(fb, &author, w), TextStyle::INK);
                y += line_h(fb) + 20;
                let (ch_secs, book_secs) = r.time_left(cx.lib, cx.stats);
                let clock = quire_library::time::fmt_clock(now, cx.settings.clock_24h);
                let l1 = alloc::format!("{} · {clock}", left_line(ch_secs, "this chapter"));
                draw_text(f, fb, x, y + fb.ascent(), &ellipsis(fb, &l1, w), TextStyle::INK);
                y += line_h(fb);
                let fin = cx.stats.finish_day(today, book_secs);
                draw_text(f, fb, x, y + fb.ascent(), &finish_by_line(today, fin), TextStyle::INK);
                y += line_h(fb);
                let cur = r.id;
                for b in cx.lib.shelf().into_iter().filter(|b| b.id != cur).take(3) {
                    let v = if b.status == Status::Finished { String::from("finished") } else { fmt_duration(cx.stats.time_left_secs(b)) };
                    recent.push((b.title.clone(), v));
                }
            }
            None => {
                draw_text(f, ft, x, y + ft.ascent(), "Nothing open yet", TextStyle::INK);
                y += ft.ascent() + ft.below() + 12;
                for b in cx.lib.shelf().into_iter().take(3) {
                    recent.push((b.title.clone(), fmt_duration(cx.stats.time_left_secs(b))));
                }
            }
        }
        y += 24;
        f.fill_rect(Rect::new(x, y, w as u32, 1), Ink::Black);
        y += 14;
        draw_label(f, x, y + fl.ascent(), "Recent", false);
        y += line_h(fl) + 4;
        for (i, (t, v)) in recent.iter().enumerate() {
            if i > 0 {
                f.fill_rect(Rect::new(x, y, w as u32, 1), Ink::Black);
            }
            let inv = self.focus == i + 1;
            if inv {
                f.fill_rect(Rect::new(x, y, w as u32, 48), Ink::Black);
            }
            let style = TextStyle { inverted: inv, ..TextStyle::INK };
            let vw = quire_gfx::measure_text(fl, v, style);
            draw_text(f, fb, x + 4, centered_baseline(fb, y, 48), &ellipsis(fb, t, w - vw - 24), style);
            draw_text(f, fl, x + w - vw - 4, centered_baseline(fl, y, 48), v, style);
            y += 48;
            if y + 48 > card.bottom() - 8 {
                break;
            }
        }
        widgets::side_labels(f, Some("Drop"), Some("Stats"), true);
        rail(f, ["Library", "Close", "Continue", "Bookshop"], None);
        Refresh::Du
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        let cur = cx.reader.as_ref().map(|r| r.id);
        let recent: Vec<quire_library::BookId> = cx.lib.shelf().into_iter().filter(|b| Some(b.id) != cur).take(3).map(|b| b.id).collect();
        match ev.key {
            Key::Back => Action::Pop,
            Key::Confirm => {
                if self.focus >= 1 {
                    if let Some(id) = recent.get(self.focus - 1) {
                        return Action::Open(*id);
                    }
                }
                Action::Pop
            }
            Key::Left => Action::Replace(Box::new(super::library::LibraryScreen::new())),
            Key::Right => Action::Replace(Box::new(super::bookshop::BookshopHome::new())),
            Key::Up => {
                // Up cycles focus through the recent rows; long Up is Drop (side label).
                if self.focus == 0 {
                    return Action::Replace(Box::new(super::drop::DropScreen::new()));
                }
                self.focus -= 1;
                Action::Redraw
            }
            Key::Down => {
                if self.focus < recent.len() {
                    self.focus += 1;
                    Action::Redraw
                } else {
                    Action::Replace(Box::new(super::stats::Overview::new()))
                }
            }
            Key::Power => Action::None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// 29 footnote card

/// A footnote card over the lower third.
pub struct FootnoteCard {
    label: String,
    text: String,
    target: String,
    page: usize,
}

impl FootnoteCard {
    /// Card for a note target.
    pub fn new<E: Env>(cx: &mut Ctx<E>, target: &str, label: &str) -> Self {
        let text = cx
            .reader
            .as_ref()
            .and_then(|r| r.book.note_text(cx.env.fs(), target))
            .unwrap_or_else(|| String::from("The note could not be found in this book."));
        FootnoteCard { label: label.into(), text, target: target.into(), page: 0 }
    }
}

impl<E: Env> Screen<E> for FootnoteCard {
    fn name(&self) -> &'static str {
        "29-footnote"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let h = f.height() as i32;
        let top = h * 2 / 3 - 40;
        let card = Rect::new(0, top, f.width(), (h - top) as u32);
        widgets::screen(f, Rect::new(0, 0, f.width(), top as u32));
        f.fill_rect(card, Ink::White);
        f.fill_rect(Rect::new(0, top, card.w, RULE), Ink::Black);
        let fl = quire_fonts::ui::label();
        let fb = quire_fonts::ui::body();
        draw_label(f, 28, top + 20 + fl.ascent(), &alloc::format!("Footnote {}", self.label), false);
        let lines = wrap(fb, &self.text, f.width() as i32 - 56);
        let per = ((card.h as i32 - 60 - RAIL_H) / line_h(fb)).max(1) as usize;
        let mut y = top + 20 + line_h(fl) + 10;
        for l in lines.iter().skip(self.page * per).take(per) {
            draw_text(f, fb, 28, y + fb.ascent(), l, TextStyle::INK);
            y += line_h(fb);
        }
        let more = lines.len() > (self.page + 1) * per;
        rail(f, ["", "Back to text", "Open note", if more { "More" } else { "" }], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Right => {
                self.page += 1;
                Action::Redraw
            }
            Key::Left => {
                self.page = self.page.saturating_sub(1);
                Action::Redraw
            }
            Key::Confirm => {
                // Jump to the note in the text; Back from there returns (the reader saves the origin).
                let fs = cx.env.fs();
                if let Some(r) = cx.reader.as_mut() {
                    if let Some(loc) = r.book.resolve_anchor(&self.target) {
                        r.push_return();
                        r.goto(fs, loc);
                        r.force_gc();
                    }
                }
                Action::Pop
            }
            _ => Action::None,
        }
    }
}

/// The word-cursor and other overlays share this: a row of small hints in the rail.
pub fn hint_rail(f: &mut Frame, labels: [&str; 4]) {
    rail(f, labels, None);
}

/// Draw a "Keys locked" strip (45) over the top of the page for one refresh.
pub fn draw_locked_strip(f: &mut Frame) {
    let font = quire_fonts::ui::label();
    let r = Rect::new(0, 0, f.width(), 40);
    f.fill_rect(r, Ink::Black);
    draw_centered(f, font, f.width() as i32 / 2, centered_baseline(font, 0, 40), "Keys locked · hold Power to unlock", TextStyle::PAPER);
}

/// Sleep request helper used by several screens.
pub fn go_to_sleep<E: Env>() -> Action<E> {
    Action::System(SysRequest::Sleep)
}

/// Row state helper for lists (re-exported for the screens).
pub fn row_state(focused: bool) -> RowState {
    if focused {
        RowState::Focused
    } else {
        RowState::Normal
    }
}

/// Result helper for pickers.
pub fn choice(i: usize) -> Result_ {
    Result_::Choice(i)
}

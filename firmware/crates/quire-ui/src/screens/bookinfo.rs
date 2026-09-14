//! 12 book info: cover, title, poster numerals (time left, finish by, percent), details,
//! the book's own analytics block.
//!
//! Everything read from the card (the thumbnail, the session log, the manifest for page
//! two) is loaded once into the screen and kept; a draw touches nothing but RAM.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, BlitMode, Frame, Ink, Rect, TextStyle};
use quire_library::time::{fmt_date, fmt_duration};
use quire_library::{cache, BookId, IngestState, Status};

use crate::text::{ellipsis, line_h, wrap};
use crate::widgets::{self, poster_tiles, rail, running_head};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Screen};

/// The cover thumbnail on the page.
const COVER_W: u32 = 104;
const COVER_H: u32 = 156;

/// The book info screen.
pub struct BookInfo {
    id: BookId,
    page: usize,
    /// The thumbnail scaled to the page's cover size, read once.
    thumb: Option<Option<quire_gfx::Bitmap>>,
    /// Minutes read per day over the last 30 days, from the session log, read once.
    days: Option<Vec<u32>>,
    /// Page two's description and subject line, read once when first shown.
    meta: Option<(String, String)>,
}

impl BookInfo {
    /// New.
    pub fn new(id: BookId) -> Self {
        BookInfo { id, page: 0, thumb: None, days: None, meta: None }
    }
    fn ensure_page1<E: Env>(&mut self, cx: &Ctx<E>, today: u16) {
        if self.thumb.is_none() {
            let scaled = cache::load_thumb(cx.env.fs(), self.id).map(|bm| {
                let mut tmp = quire_gfx::Bitmap::new(COVER_W, COVER_H);
                for yy in 0..COVER_H {
                    for xx in 0..COVER_W {
                        if bm.get(xx * bm.w / COVER_W, yy * bm.h / COVER_H) {
                            tmp.set(xx, yy, true);
                        }
                    }
                }
                tmp
            });
            self.thumb = Some(scaled);
        }
        if self.days.is_none() {
            let sessions = cx.stats.book_sessions(cx.env.fs(), self.id, today, 30, 200);
            let mut days = alloc::vec![0u32; 30];
            for s in &sessions {
                let d = quire_library::time::day_of(s.start);
                if d <= today && today - d < 30 {
                    days[(29 - (today - d)) as usize] += s.active / 60;
                }
            }
            self.days = Some(days);
        }
    }
    fn ensure_page2<E: Env>(&mut self, cx: &Ctx<E>) {
        if self.meta.is_none() {
            let meta = quire_library::Book::open(cx.env.fs(), self.id).ok().map(|bk| bk.meta);
            let desc = meta.as_ref().and_then(|m| m.description.clone()).unwrap_or_else(|| String::from("No description in this file."));
            let subjects = meta.map(|m| m.subjects.join(" · ")).unwrap_or_default();
            self.meta = Some((desc, subjects));
        }
    }
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        alloc::format!("{}.{} MB", bytes / (1024 * 1024), (bytes % (1024 * 1024)) * 10 / (1024 * 1024))
    } else {
        alloc::format!("{} KB", bytes / 1024)
    }
}

impl<E: Env> Screen<E> for BookInfo {
    fn name(&self) -> &'static str {
        "12-bookinfo"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let Some(b) = cx.lib.get(self.id).cloned() else {
            // The book left the library while this was open (deleted from its compass).
            running_head(f, "Book", None);
            widgets::empty_state(f, f.height() as i32 / 2 - 40, "This book is no longer on the card", "Back returns to the list.");
            rail(f, ["", "Back", "", ""], None);
            return Refresh::Gc;
        };
        running_head(f, "Book", None);
        let w = f.width() as i32;
        let ft = quire_fonts::ui::title();
        let fb = quire_fonts::ui::body();
        let fl = quire_fonts::ui::label();
        let today = cx.today();
        let mut y = widgets::CONTENT_TOP;
        if self.page == 0 {
            self.ensure_page1(cx, today);
            // Cover left, title block right.
            let cr = Rect::new(widgets::INSET, y, COVER_W, COVER_H);
            match self.thumb.as_ref().and_then(|t| t.as_ref()) {
                Some(bm) => f.blit(cr.x, cr.y, bm.as_ref(), BlitMode::Or),
                None => widgets::typographic_cover(f, cr, &b.title, &b.author_line()),
            }
            f.stroke_rect(cr, 2, Ink::Black);
            let tx = cr.right() + 20;
            let tw = w - tx - widgets::INSET;
            let mut ty = y;
            for l in wrap(ft, &b.title, tw).iter().take(3) {
                draw_text(f, ft, tx, ty + ft.ascent(), l, TextStyle::INK);
                ty += line_h(ft);
            }
            draw_text(f, fb, tx, ty + fb.ascent(), &ellipsis(fb, &b.author_line(), tw), TextStyle::INK);
            ty += line_h(fb);
            let series = match &b.series {
                Some((n, i)) if *i > 0 => alloc::format!(
                    "{n} · {}",
                    if i % 10 == 0 { alloc::format!("{}", i / 10) } else { alloc::format!("{}.{}", i / 10, i % 10) }
                ),
                Some((n, _)) => n.clone(),
                None => String::from("no series"),
            };
            let year = b.year.map(|y| alloc::format!("{y} · ")).unwrap_or_default();
            draw_text(f, fl, tx, ty + fl.ascent() + 4, &ellipsis(fl, &alloc::format!("{year}{series}"), tw), TextStyle::INK);
            y = cr.bottom() + 20;
            // Poster numerals.
            let left = cx.stats.time_left_secs(&b);
            let fin = cx.stats.finish_day(today, left);
            let tiles = if b.status == Status::Finished {
                alloc::vec![
                    (fmt_duration(b.stats.seconds), String::from("Reading time")),
                    (b.stats.finished.map(fmt_date).unwrap_or_default(), String::from("Finished")),
                    (String::from("100%"), String::from("Read"))
                ]
            } else if b.ingest != IngestState::Ready {
                alloc::vec![
                    (String::from("—"), String::from("Time left")),
                    (String::from("—"), String::from("Finish by")),
                    (String::from("0%"), String::from("Read"))
                ]
            } else {
                alloc::vec![
                    (fmt_duration(left), String::from("Time left")),
                    (super::finish_short(today, fin), String::from("Finish by")),
                    (alloc::format!("{}%", b.percent()), String::from("Read"))
                ]
            };
            y = poster_tiles(f, widgets::INSET, y, w - 2 * widgets::INSET, &tiles, 3) + 16;
            // Last 30 days ink line for this book.
            let days = self.days.clone().unwrap_or_default();
            if days.iter().any(|d| *d > 0) {
                crate::text::draw_label(f, widgets::INSET, y + fl.ascent(), "Last 30 days", false);
                y += line_h(fl) + 4;
                widgets::ink_line(
                    f,
                    Rect::new(widgets::INSET, y, (w - 2 * widgets::INSET) as u32, 96),
                    &days,
                    ("30 days ago", "", "today"),
                    " min",
                    None,
                );
                y += 100;
                draw_text(f, fl, widgets::INSET, y + fl.ascent(), "forecast from the last seven sessions' pace", TextStyle::INK);
                y += line_h(fl) + 8;
            }
            // Detail line.
            let fmt = alloc::format!("{:?}", b.format).to_uppercase();
            let mut parts: Vec<String> = Vec::new();
            parts.push(fmt);
            parts.push(size_text(b.size));
            parts.push(alloc::format!("added {}", fmt_date(quire_library::time::day_of(b.added))));
            if let Some(e) = &b.error {
                parts.push(alloc::format!("couldn't open: {e}"));
            }
            for l in wrap(fl, &parts.join(" · "), w - 2 * widgets::INSET).iter().take(3) {
                draw_text(f, fl, widgets::INSET, y + fl.ascent(), l, TextStyle::INK);
                y += line_h(fl);
            }
            let _ = y;
        } else {
            // Page 2: description and path.
            self.ensure_page2(cx);
            let (desc, subjects) = self.meta.clone().unwrap_or_default();
            for l in wrap(fb, &desc, w - 2 * widgets::INSET).iter().take(18) {
                draw_text(f, fb, widgets::INSET, y + fb.ascent(), l, TextStyle::INK);
                y += line_h(fb);
            }
            y += 12;
            if !subjects.is_empty() {
                draw_text(f, fl, widgets::INSET, y + fl.ascent(), &ellipsis(fl, &subjects, w - 2 * widgets::INSET), TextStyle::INK);
                y += line_h(fl);
            }
            draw_text(
                f,
                quire_fonts::ui::mono(),
                widgets::INSET,
                y + fl.ascent() + 4,
                &ellipsis(quire_fonts::ui::mono(), &b.path, w - 2 * widgets::INSET),
                TextStyle::INK,
            );
        }
        rail(f, ["", "Back", "Open", "More"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        if cx.lib.get(self.id).is_none() {
            return Action::Pop;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Confirm => {
                let ready = cx.lib.get(self.id).map(|b| b.ingest == IngestState::Ready).unwrap_or(false);
                if ready {
                    Action::Open(self.id)
                } else {
                    Action::None
                }
            }
            Key::Right | Key::Left => {
                self.page ^= 1;
                Action::Redraw
            }
            Key::Down => Action::Push(Box::new(super::library::BookCompass::new(self.id))),
            _ => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if matches!(ev, Event::Ingest { .. }) {
            // Ingest changes the cover and the manifest: read them again next draw.
            self.thumb = None;
            self.meta = None;
            Action::Redraw
        } else {
            Action::None
        }
    }
    fn resume(&mut self, _cx: &mut Ctx<E>) {
        // A session may have been recorded meanwhile (the compass opened the book).
        self.days = None;
    }
}

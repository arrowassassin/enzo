//! 12 book info: cover, title, poster numerals (time left, finish by, percent), details,
//! the book's own analytics block.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, BlitMode, Frame, Ink, Rect, TextStyle};
use quire_library::time::{fmt_date, fmt_duration};
use quire_library::{cache, BookId, IngestState, Status};

use crate::text::{ellipsis, line_h, wrap};
use crate::widgets::{self, poster_tiles, rail, running_head};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Screen};

/// The book info screen.
pub struct BookInfo {
    id: BookId,
    page: usize,
}

impl BookInfo {
    /// New.
    pub fn new(id: BookId) -> Self {
        BookInfo { id, page: 0 }
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
        let Some(b) = cx.lib.get(self.id).cloned() else { return Refresh::Du };
        running_head(f, "Book", None);
        let w = f.width() as i32;
        let ft = quire_fonts::ui::title();
        let fb = quire_fonts::ui::body();
        let fl = quire_fonts::ui::label();
        let today = cx.today();
        let mut y = widgets::CONTENT_TOP;
        if self.page == 0 {
            // Cover left, title block right.
            let cover = cache::load_thumb(cx.env.fs(), self.id);
            let cr = Rect::new(widgets::INSET, y, 104, 156);
            match cover {
                Some(bm) => {
                    let mut tmp = quire_gfx::Bitmap::new(104, 156);
                    for yy in 0..156u32 {
                        for xx in 0..104u32 {
                            if bm.get(xx * bm.w / 104, yy * bm.h / 156) {
                                tmp.set(xx, yy, true);
                            }
                        }
                    }
                    f.blit(cr.x, cr.y, tmp.as_ref(), BlitMode::Or);
                }
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
            let sessions = cx.stats.book_sessions(cx.env.fs(), self.id, today, 30, 200);
            let mut days = alloc::vec![0u32; 30];
            for s in &sessions {
                let d = quire_library::time::day_of(s.start);
                if d <= today && today - d < 30 {
                    days[(29 - (today - d)) as usize] += s.active / 60;
                }
            }
            let has = days.iter().any(|d| *d > 0);
            if has {
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
            let meta = quire_library::Book::open(cx.env.fs(), self.id).ok().map(|bk| bk.meta);
            let desc = meta.as_ref().and_then(|m| m.description.clone()).unwrap_or_else(|| String::from("No description in this file."));
            for l in wrap(fb, &desc, w - 2 * widgets::INSET).iter().take(18) {
                draw_text(f, fb, widgets::INSET, y + fb.ascent(), l, TextStyle::INK);
                y += line_h(fb);
            }
            y += 12;
            let subjects = meta.map(|m| m.subjects.join(" · ")).unwrap_or_default();
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
            Action::Redraw
        } else {
            Action::None
        }
    }
}

//! 01 boot: wordmark, version, one stepped bar.

use quire_gfx::{draw_text, Frame, Rect, TextStyle};

use crate::text::draw_centered;
use crate::widgets::stepped_bar;
use crate::{Action, Ctx, Env, Event, KeyEvent, Refresh, Screen};

/// The boot screen; the platform pops it when indexing is done.
pub struct Boot {
    /// Progress text.
    pub status: alloc::string::String,
    /// Progress in 1/1000.
    pub permille: u32,
}

impl Boot {
    /// New.
    pub fn new(status: &str) -> Self {
        Boot { status: status.into(), permille: 0 }
    }
}

impl<E: Env> Screen<E> for Boot {
    fn name(&self) -> &'static str {
        "01-boot"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let w = f.width() as i32;
        let h = f.height() as i32;
        draw_centered(f, quire_fonts::ui::hero(), w / 2, h / 2 - 40, "Quire", TextStyle::INK);
        let v = cx.env.device().version.clone();
        draw_centered(f, quire_fonts::ui::mono(), w / 2, h / 2 + 4, &v, TextStyle::INK);
        stepped_bar(f, Rect::new(w / 2 - 120, h / 2 + 60, 240, 16), self.permille);
        draw_text(f, quire_fonts::ui::label(), w / 2 - 120, h / 2 + 100, &self.status, TextStyle::INK);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, _ev: KeyEvent) -> Action<E> {
        Action::None
    }
    fn event(&mut self, cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::Ingest { done, total, .. } => {
                self.permille = if *total > 0 { (*done as u64 * 1000 / *total as u64) as u32 } else { 0 };
                self.status = alloc::format!("Indexing {done} of {total} books");
                Action::Redraw
            }
            Event::BooksChanged => {
                let _ = cx;
                Action::Pop
            }
            _ => Action::None,
        }
    }
}

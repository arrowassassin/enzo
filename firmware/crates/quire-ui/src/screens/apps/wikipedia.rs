//! 77 Wikipedia: search a term, read the summary, open the article on the page.

use alloc::string::String;
use quire_gfx::{draw_text, Frame, TextStyle};

use crate::keyboard::KeyboardScreen;
use crate::net::{FetchRequest, NetEvent};
use crate::text::{line_h, page_indicator, paginate};
use crate::theme::*;
use crate::widgets::{self, empty_state, rail, running_head};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen, SysRequest, WifiState};

/// The Wikipedia app.
pub struct Wikipedia {
    query: String,
    result: Option<(String, String)>,
    error: Option<String>,
    working: bool,
    page: usize,
}

impl Wikipedia {
    /// New.
    pub fn new() -> Self {
        Wikipedia { query: String::new(), result: None, error: None, working: false, page: 0 }
    }
}

impl Default for Wikipedia {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Wikipedia {
    fn name(&self) -> &'static str {
        "77-wikipedia"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let w = f.width() as i32;
        let fb = quire_fonts::ui::body();
        let per = ((f.height() as i32 - RAIL_H - widgets::CONTENT_TOP - 8) / line_h(fb)).max(4) as usize;
        match (&self.result, &self.error) {
            (Some((title, text)), _) => {
                let pages = paginate(fb, text, w - 2 * widgets::INSET, per);
                running_head(f, title, Some(&page_indicator(self.page, pages.len())));
                let mut y = widgets::CONTENT_TOP;
                for l in pages.get(self.page).into_iter().flatten() {
                    draw_text(f, fb, widgets::INSET, y + fb.ascent(), l, TextStyle::INK);
                    y += line_h(fb);
                }
                rail(f, ["", "Back", "Search", "Next"], None);
            }
            (None, Some(e)) => {
                running_head(f, "Wikipedia", None);
                empty_state(f, 240, "Couldn't reach Wikipedia", e);
                rail(f, ["", "Back", "Search", "Retry"], None);
            }
            (None, None) => {
                running_head(f, "Wikipedia", None);
                let wifi = matches!(cx.env.wifi(), WifiState::Connected { .. });
                if self.working {
                    widgets::working_card(f, "Looking up", &self.query, 0, "wikipedia.org");
                } else if wifi {
                    empty_state(f, 240, "Look anything up", "Confirm to type a term on your phone or the keyboard.");
                } else {
                    empty_state(f, 240, "Wikipedia needs Wi-Fi", "Confirm turns it on; then search a term.");
                }
                rail(f, ["", "Back", if wifi { "Search" } else { "Wi-Fi" }, ""], None);
            }
        }
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Confirm => {
                if self.result.is_none() && !matches!(cx.env.wifi(), WifiState::Connected { .. }) {
                    return Action::Push(alloc::boxed::Box::new(super::super::wifi::WifiScreen::new_with_hint("Wikipedia needs Wi-Fi")));
                }
                Action::Push(KeyboardScreen::t9("Wikipedia", &self.query, "Search").boxed())
            }
            Key::Right => {
                if self.result.is_some() {
                    self.page += 1;
                } else if !self.query.is_empty() {
                    self.error = None;
                    self.working = true;
                    cx.env.request(SysRequest::Fetch(FetchRequest::Wikipedia(self.query.clone())));
                }
                Action::Redraw
            }
            Key::Left => {
                self.page = self.page.saturating_sub(1);
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn event(&mut self, cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::Net(NetEvent::Wikipedia(r)) => {
                self.working = false;
                match r {
                    Ok((t, s)) => {
                        self.result = Some((t.clone(), s.clone()));
                        self.page = 0;
                    }
                    Err(e) => self.error = Some(e.clone()),
                }
                Action::Redraw
            }
            Event::PhoneText(t) => {
                cx.phone_text.take();
                self.query = t.clone();
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let Result_::Text(t) = r {
            let t: String = t.trim().into();
            if !t.is_empty() {
                self.query = t;
                self.result = None;
                self.error = None;
                if matches!(cx.env.wifi(), WifiState::Connected { .. }) {
                    self.working = true;
                    cx.env.request(SysRequest::Fetch(FetchRequest::Wikipedia(self.query.clone())));
                } else {
                    self.error = Some(String::from("Wi-Fi is off."));
                }
            }
        }
        Action::Redraw
    }
}

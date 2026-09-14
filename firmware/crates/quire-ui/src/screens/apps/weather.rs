//! 74 weather: the temperature numeral, condition in words, five days of numerals.

use alloc::string::String;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};

use crate::keyboard::KeyboardScreen;
use crate::net::{FetchRequest, NetEvent};
use crate::text::{draw_centered, draw_label, line_h, small_caps};
use crate::widgets::{self, empty_state, rail, running_head};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen, SysRequest, WifiState};

/// The weather screen.
pub struct Weather {
    requested: bool,
}

impl Weather {
    /// New.
    pub fn new() -> Self {
        Weather { requested: false }
    }
}

impl Default for Weather {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Weather {
    fn name(&self) -> &'static str {
        "74-weather"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Weather", None);
        let w = f.width() as i32;
        let place = cx.settings.weather_place.clone();
        if place.is_empty() {
            empty_state(f, 240, "Where are you?", "Confirm to type a place name; the forecast comes from Open-Meteo, no account needed.");
            rail(f, ["", "Back", "Place", ""], None);
            return Refresh::Gc;
        }
        if !self.requested && matches!(cx.env.wifi(), WifiState::Connected { .. }) {
            self.requested = true;
            cx.env.request(SysRequest::Fetch(FetchRequest::Weather));
        }
        let fl = quire_fonts::ui::label();
        let hero = quire_fonts::ui::hero();
        let fb = quire_fonts::ui::body();
        let mut y = widgets::CONTENT_TOP;
        draw_label(f, widgets::INSET, y + fl.ascent(), &place, false);
        y += line_h(fl) + 12;
        match cx.env.net().weather() {
            Some((temp, cond, days)) => {
                draw_text(f, hero, widgets::INSET, y + hero.ascent(), &alloc::format!("{temp}°"), TextStyle::INK);
                draw_text(f, fb, widgets::INSET + 150, y + hero.ascent() - 10, &cond, TextStyle::INK);
                y += hero.ascent() + hero.descent() + 30;
                f.fill_rect(Rect::new(widgets::INSET, y, (w - 2 * widgets::INSET) as u32, 2), Ink::Black);
                y += 16;
                let cw = (w - 2 * widgets::INSET) / 5;
                let poster = quire_fonts::ui::poster();
                for (i, (name, hi, lo)) in days.iter().take(5).enumerate() {
                    let cx_ = widgets::INSET + i as i32 * cw + cw / 2;
                    draw_centered(f, fl, cx_, y + fl.ascent(), &small_caps(name), crate::text::label_style(false));
                    draw_centered(f, poster, cx_, y + line_h(fl) + 8 + poster.ascent(), &alloc::format!("{hi}°"), TextStyle::INK);
                    draw_centered(
                        f,
                        fl,
                        cx_,
                        y + line_h(fl) + 8 + poster.ascent() + poster.descent() + 4 + fl.ascent(),
                        &alloc::format!("{lo}°"),
                        TextStyle::INK,
                    );
                }
            }
            None => {
                let msg = match cx.env.wifi() {
                    WifiState::Connected { .. } => "Fetching the forecast…",
                    _ => "Turn Wi-Fi on to fetch the forecast (Power menu → Wi-Fi on). Night jobs refresh it while you sleep.",
                };
                for l in crate::text::wrap(fb, msg, w - 2 * widgets::INSET) {
                    draw_text(f, fb, widgets::INSET, y + fb.ascent(), &l, TextStyle::INK);
                    y += line_h(fb);
                }
            }
        }
        rail(f, ["", "Back", "Place", "Refresh"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Confirm => Action::Push(KeyboardScreen::new("Place", &cx.settings.weather_place, "City or town").boxed()),
            Key::Right => {
                self.requested = false;
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if matches!(ev, Event::Net(NetEvent::Weather) | Event::Wifi(_)) {
            Action::Redraw
        } else {
            Action::None
        }
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let Result_::Text(t) = r {
            let t: String = t.trim().into();
            if !t.is_empty() {
                cx.settings.weather_place = t;
                cx.settings.weather_lat = 0;
                cx.settings.weather_lon = 0;
                self.requested = false;
            }
        }
        Action::Redraw
    }
}

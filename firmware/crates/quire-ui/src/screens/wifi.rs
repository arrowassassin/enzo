//! 31 Wi-Fi: saved networks, scan, password (phone first), status card.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};

use crate::icons::{self, Icon};
use crate::keyboard::KeyboardScreen;
use crate::text::{ellipsis, line_h};
use crate::theme::*;
use crate::widgets::{self, rail, row, running_head, ListNav, RowState};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen, SysRequest, WifiNetwork, WifiState};

/// The Wi-Fi screen.
pub struct WifiScreen {
    nav: ListNav,
    scanning: bool,
    scan: Vec<WifiNetwork>,
    hint: Option<String>,
    /// Network awaiting a password.
    pending: Option<String>,
}

impl WifiScreen {
    /// New.
    pub fn new() -> Self {
        WifiScreen { nav: ListNav::new(0, 8), scanning: false, scan: Vec::new(), hint: None, pending: None }
    }
    /// With a one-line reason ("Updates need Wi-Fi").
    pub fn new_with_hint(hint: &str) -> Self {
        WifiScreen { hint: Some(hint.into()), ..Self::new() }
    }
    fn rows<E: Env>(&self, cx: &Ctx<E>) -> Vec<(String, String, bool)> {
        // (name, value, saved)
        let saved = cx.env.saved_networks();
        let cur = match cx.env.wifi() {
            WifiState::Connected { ssid, .. } => Some(ssid),
            WifiState::Connecting(s) => Some(s),
            _ => None,
        };
        let mut out: Vec<(String, String, bool)> = saved
            .iter()
            .map(|s| {
                let v = if cur.as_deref() == Some(s.as_str()) {
                    match cx.env.wifi() {
                        WifiState::Connected { .. } => String::from("connected"),
                        _ => String::from("joining"),
                    }
                } else {
                    String::from("saved")
                };
                (s.clone(), v, true)
            })
            .collect();
        for n in &self.scan {
            if !saved.contains(&n.ssid) {
                out.push((n.ssid.clone(), signal_bars(n.signal), false));
            }
        }
        out.push((String::from("Add network"), if self.scanning { String::from("scanning…") } else { String::from("scan") }, false));
        out
    }
}

impl Default for WifiScreen {
    fn default() -> Self {
        Self::new()
    }
}

/// Signal strength as a word (the mono face has no bar glyphs).
fn signal_bars(s: u8) -> String {
    String::from(match s {
        0 => "faint",
        1 => "weak",
        2 => "fair",
        3 => "good",
        _ => "strong",
    })
}

impl<E: Env> Screen<E> for WifiScreen {
    fn name(&self) -> &'static str {
        "31-wifi"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Wi-Fi", None);
        let w = f.width() as i32;
        let fl = quire_fonts::ui::label();
        let fb = quire_fonts::ui::body();
        let mut y = widgets::CONTENT_TOP;
        if let Some(h) = &self.hint {
            draw_text(f, fb, widgets::INSET, y + fb.ascent(), &ellipsis(fb, h, w - 2 * widgets::INSET), TextStyle::INK);
            y += line_h(fb) + 8;
        }
        // Status card.
        let state = cx.env.wifi();
        let card = Rect::new(widgets::INSET, y, (w - 2 * widgets::INSET) as u32, 76);
        f.stroke_rect(card, 2, Ink::Black);
        let (l1, l2) = match &state {
            WifiState::Connected { ssid, ip, host, signal } => {
                (alloc::format!("{ssid} {}", signal_bars(*signal)), alloc::format!("{host}.local · {ip}"))
            }
            WifiState::Hotspot { ssid, password, ip } => (alloc::format!("Hotspot {ssid}"), alloc::format!("password {password} · {ip}")),
            WifiState::Connecting(s) => (alloc::format!("Joining {s}…"), String::new()),
            WifiState::Failed(s) => (alloc::format!("Couldn't join {s}."), String::from("Check the password.")),
            WifiState::Off => (String::from("Wi-Fi off"), String::from("Reading mode · Confirm on a network joins it.")),
        };
        icons::draw(f, if matches!(state, WifiState::Off) { Icon::WifiOff } else { Icon::Wifi }, card.x + 14, card.y + 12, Ink::Black);
        draw_text(f, fb, card.x + 50, card.y + 14 + fb.ascent(), &ellipsis(fb, &l1, card.w as i32 - 64), TextStyle::INK);
        draw_text(f, fl, card.x + 50, card.y + 14 + line_h(fb) + fl.ascent(), &ellipsis(fl, &l2, card.w as i32 - 64), TextStyle::INK);
        y = card.bottom() + 12;
        let rows = self.rows(cx);
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(y, f.height() as i32 - RAIL_H, row_h);
        self.nav.set_n(rows.len());
        for i in self.nav.visible() {
            let (n, v, _) = &rows[i];
            row(f, y, row_h, n, None, Some(v), if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        rail(f, ["", "Back", "Join", "Forget"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        let rows = self.rows(cx);
        if ev.is(Key::Confirm) {
            let Some((name, _, saved)) = rows.get(self.nav.focus).cloned() else { return Action::None };
            if self.nav.focus + 1 == rows.len() {
                self.scanning = true;
                cx.env.request(SysRequest::WifiScan);
                return Action::Redraw;
            }
            if saved {
                cx.env.request(SysRequest::WifiJoin { ssid: name, password: String::new() });
                return Action::Redraw;
            }
            let secured = self.scan.iter().find(|n| n.ssid == name).map(|n| n.secured).unwrap_or(true);
            if !secured {
                cx.env.request(SysRequest::WifiJoin { ssid: name, password: String::new() });
                return Action::Redraw;
            }
            self.pending = Some(name.clone());
            return Action::Push(KeyboardScreen::new(&alloc::format!("Password for {name}"), "", "Password").secret().boxed());
        }
        if ev.is(Key::Right) {
            if let Some((name, _, true)) = rows.get(self.nav.focus).cloned() {
                cx.env.request(SysRequest::WifiForget(name));
            }
            return Action::Redraw;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::WifiScan(list) => {
                self.scanning = false;
                self.scan = list.clone();
                self.scan.sort_by_key(|n| core::cmp::Reverse(n.signal));
                Action::Redraw
            }
            Event::Wifi(_) => Action::Redraw,
            _ => Action::None,
        }
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let (Result_::Text(pw), Some(ssid)) = (r, self.pending.take()) {
            cx.env.request(SysRequest::WifiJoin { ssid, password: pw });
        }
        Action::Redraw
    }
}

/// Boxed for Jump.
pub fn boxed() -> Box<WifiScreen> {
    Box::new(WifiScreen::new())
}

//! 30 the Drop page on the device: QR pair, URL, arriving files, Wi-Fi line.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};

use crate::net::DownloadState;
use crate::text::{draw_label, ellipsis, line_h};
use crate::theme::*;
use crate::widgets::{self, rail, running_head, stepped_bar};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Screen, SysRequest, WifiState};

/// The URL of the Drop page for the current network state.
pub fn drop_url<E: Env>(cx: &Ctx<E>) -> String {
    match cx.env.wifi() {
        WifiState::Connected { host, .. } => alloc::format!("http://{host}.local"),
        WifiState::Hotspot { ip, .. } => alloc::format!("http://{ip}"),
        _ => alloc::format!("http://{}.local", cx.settings.hostname),
    }
}

/// The Drop screen.
pub struct DropScreen {
    started: bool,
}

impl DropScreen {
    /// New: turns Wi-Fi on (or the hotspot) when shown.
    pub fn new() -> Self {
        DropScreen { started: false }
    }
}

impl Default for DropScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for DropScreen {
    fn name(&self) -> &'static str {
        "30-drop"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.started {
            self.started = true;
            if matches!(cx.env.wifi(), WifiState::Off) {
                cx.env.request(if cx.env.saved_networks().is_empty() { SysRequest::Hotspot } else { SysRequest::WifiOn });
            }
        }
        running_head(f, "Drop books here", None);
        let w = f.width() as i32;
        let x = widgets::INSET;
        let fl = quire_fonts::ui::label();
        let fb = quire_fonts::ui::body();
        let mono = quire_fonts::ui::list_title();
        let mut y = widgets::CONTENT_TOP;
        let wifi = cx.env.wifi();
        let free_ok = cx.env.device().card_free.map(|fr| fr > 4 * 1024 * 1024).unwrap_or(true);
        match &wifi {
            WifiState::Connected { .. } | WifiState::Hotspot { .. } => {
                let url = drop_url(cx);
                // The QR pair: the page, and (for the hotspot) the network itself.
                let qr = crate::qr::size(&url, 5).unwrap_or(0);
                crate::qr::draw(f, &url, x, y, 5);
                if let WifiState::Hotspot { ssid, password, .. } = &wifi {
                    let wifi_qr = alloc::format!("WIFI:T:WPA;S:{ssid};P:{password};;");
                    crate::qr::draw(f, &wifi_qr, x + qr + 24, y, 5);
                    draw_label(f, x + qr + 24, y + qr + 20, "1 · join", false);
                    draw_label(f, x, y + qr + 20, "2 · open", false);
                    y += qr + 44;
                    draw_text(f, fb, x, y + fb.ascent(), &alloc::format!("Network {ssid} · password {password}"), TextStyle::INK);
                    y += line_h(fb);
                } else {
                    let tx = x + qr + 24;
                    draw_text(f, fb, tx, y + 40, "Scan with your phone,", TextStyle::INK);
                    draw_text(f, fb, tx, y + 40 + line_h(fb), "then drag books onto", TextStyle::INK);
                    draw_text(f, fb, tx, y + 40 + 2 * line_h(fb), "the page.", TextStyle::INK);
                    y += qr + 20;
                }
                draw_text(f, mono, x, y + mono.ascent(), &url.replace("http://", ""), TextStyle::INK);
                y += line_h(mono) + 16;
            }
            WifiState::Connecting(s) => {
                draw_text(f, fb, x, y + fb.ascent(), &alloc::format!("Joining {s}…"), TextStyle::INK);
                y += line_h(fb) + 16;
            }
            WifiState::Failed(s) => {
                for l in crate::text::wrap(
                    fb,
                    &alloc::format!("Couldn't join {s}. Check the password, or start the hotspot with Right."),
                    w - 2 * x,
                ) {
                    draw_text(f, fb, x, y + fb.ascent(), &l, TextStyle::INK);
                    y += line_h(fb);
                }
                y += 16;
            }
            WifiState::Off => {
                draw_text(f, fb, x, y + fb.ascent(), "Not connected. Turning Wi-Fi on…", TextStyle::INK);
                y += line_h(fb) + 16;
            }
        }
        if !free_ok {
            f.fill_rect(Rect::new(x, y, (w - 2 * x) as u32, 2), Ink::Black);
            y += 10;
            draw_text(f, fb, x, y + fb.ascent(), "The card is full. Delete a book to make room.", TextStyle::INK);
            y += line_h(fb) + 10;
        }
        // Arriving files.
        f.fill_rect(Rect::new(x, y, (w - 2 * x) as u32, 1), Ink::Black);
        y += 12;
        draw_label(f, x, y + fl.ascent(), "Arriving", false);
        y += line_h(fl) + 6;
        let downloads: Vec<crate::net::Download> = cx.env.net().downloads().to_vec();
        let mut added = 0;
        if downloads.is_empty() {
            draw_text(f, fl, x, y + fl.ascent(), "Nothing yet.", TextStyle::INK);
            y += line_h(fl);
        }
        for d in downloads.iter().rev().take(5) {
            draw_text(f, fb, x, y + fb.ascent(), &ellipsis(fb, &d.title, w - 2 * x - 120), TextStyle::INK);
            let status = match &d.state {
                DownloadState::Queued => String::from("waiting"),
                DownloadState::Working => {
                    d.total.map(|t| alloc::format!("{}%", d.done * 100 / t.max(1))).unwrap_or_else(|| String::from("…"))
                }
                DownloadState::Done => {
                    added += 1;
                    String::from("added")
                }
                DownloadState::Failed(e) => ellipsis(fl, e, 110),
                DownloadState::Retrying(s) => alloc::format!("retry in {s} s"),
            };
            let sw = quire_gfx::measure_text(fl, &status, TextStyle::INK);
            draw_text(f, fl, w - x - sw, y + fb.ascent(), &status, TextStyle::INK);
            y += line_h(fb);
            if let (DownloadState::Working, Some(t)) = (&d.state, d.total) {
                stepped_bar(f, Rect::new(x, y, (w - 2 * x) as u32, 12), (d.done * 1000 / t.max(1)) as u32);
                y += 18;
            }
        }
        let foot = f.height() as i32 - RAIL_H - 16;
        let line = match &wifi {
            WifiState::Connected { ssid, .. } => alloc::format!("Wi-Fi: {ssid} · {added} books added"),
            WifiState::Hotspot { ssid, .. } => alloc::format!("Hotspot: {ssid} · {added} books added"),
            _ => String::from("Wi-Fi: off"),
        };
        draw_text(f, fl, x, foot, &ellipsis(fl, &line, w - 2 * x), TextStyle::INK);
        widgets::side_labels(f, Some("Wi-Fi"), Some("Hotspot"), true);
        rail(f, ["", "Back", "Wi-Fi", "Hotspot"], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Confirm | Key::Up => Action::Push(Box::new(super::wifi::WifiScreen::new())),
            Key::Right | Key::Down => Action::System(SysRequest::Hotspot),
            _ => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::Wifi(_) | Event::Net(_) | Event::BooksChanged | Event::Ingest { .. } => Action::Redraw,
            _ => Action::None,
        }
    }
}

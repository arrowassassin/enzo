//! 50 settings: groups as a list; Keys, Sleep and power, Battery, Night jobs, About;
//! 51 OTA; 90 developer.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Pattern, Rect, TextStyle};

use crate::keyboard::KeyboardScreen;
use crate::net::{FetchRequest, NetEvent, OtaInfo};
use crate::text::{draw_label, ellipsis, line_h, page_indicator, wrap};
use crate::theme::*;
use crate::widgets::{self, poster_tiles, rail, row, running_head, setting_row, ListNav, RowState, SettingValue};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen, SysRequest};

// ---------------------------------------------------------------------------------------
// 50 settings home

const GROUPS: [(&str, &str); 11] = [
    ("Reading", "type, layout"),
    ("Display", "inverted, large UI, refresh"),
    ("Keys", "side keys, orientation, remote"),
    ("Sleep and power", "sleep screen, timeouts, battery"),
    ("Wi-Fi and sync", "networks, catalogs, Calibre, sync"),
    ("Library", "folders, rescan"),
    ("Bookshop", "language"),
    ("Language and time", "clock, night jobs"),
    ("Apps and games", "simple mode"),
    ("About", "version, storage, update"),
    ("Developer", "diagnostics"),
];

/// The settings list.
pub struct SettingsHome {
    nav: ListNav,
}

impl SettingsHome {
    /// New.
    pub fn new() -> Self {
        SettingsHome { nav: ListNav::new(GROUPS.len(), 11) }
    }
}

impl Default for SettingsHome {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for SettingsHome {
    fn name(&self) -> &'static str {
        "50-settings"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        // 22 px title over an 18 px subtitle fits the normal row, like Apps and Games.
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, "Settings", (self.nav.pages() > 1).then(|| page_indicator(self.nav.page(), self.nav.pages())).as_deref());
        let mut y = widgets::CONTENT_TOP;
        for i in self.nav.visible() {
            let (t, sub) = GROUPS[i];
            row(f, y, row_h, t, Some(sub), None, if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        rail(f, ["", "Back", "Open", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        if ev.is(Key::Confirm) {
            return match self.nav.focus {
                0 => Action::Push(Box::new(super::typeset::TypeScreen::new())),
                1 => Action::Push(Box::new(super::typeset::LayoutScreen::new())),
                2 => Action::Push(Box::new(KeysScreen::new())),
                3 => Action::Push(Box::new(SleepPower::new())),
                4 => Action::Push(Box::new(super::wifi::WifiScreen::new())),
                5 => Action::Push(Box::new(LibrarySettings::new())),
                6 => Action::Push(Box::new(GenericSettings::bookshop())),
                7 => Action::Push(Box::new(GenericSettings::language_time())),
                8 => Action::Push(Box::new(GenericSettings::apps())),
                9 => Action::Push(Box::new(About::new())),
                _ => Action::Push(Box::new(Developer::new())),
            };
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
}

// ---------------------------------------------------------------------------------------
// Generic setting lists

/// A row of a generic settings page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    SideKeys,
    SwapSide,
    OrientationFollow,
    LeftHanded,
    Tilt,
    Shake,
    Tap,
    Remote,
    SleepAfter,
    PowerOffAfter,
    SleepScreen,
    LockWhenSleeping,
    PowerRefreshes,
    NightJobs,
    PanelOff,
    Battery,
    BookshopLanguage,
    Clock24,
    Language,
    SetTime,
    SimpleMode,
    Rescan,
    Sources,
    Hostname,
    WifiAtBoot,
}

/// A generic settings page driven by a row list.
pub struct GenericSettings {
    title: &'static str,
    name: &'static str,
    rows: Vec<Row>,
    nav: ListNav,
}

impl GenericSettings {
    fn make(title: &'static str, name: &'static str, rows: Vec<Row>) -> Self {
        let n = rows.len();
        GenericSettings { title, name, rows, nav: ListNav::new(n, 10) }
    }
    /// Bookshop settings.
    pub fn bookshop() -> Self {
        Self::make("Bookshop", "50-bookshop", alloc::vec![Row::BookshopLanguage])
    }
    /// Language and time.
    pub fn language_time() -> Self {
        Self::make("Language and time", "50-language", alloc::vec![Row::Language, Row::Clock24, Row::SetTime, Row::NightJobs])
    }
    /// Apps and games.
    pub fn apps() -> Self {
        Self::make("Apps and games", "50-apps", alloc::vec![Row::SimpleMode])
    }
    fn value<E: Env>(&self, cx: &mut Ctx<E>, r: Row) -> (&'static str, SettingValue) {
        let s = &*cx.settings;
        let on_off = |b: bool| SettingValue::Toggle(b);
        match r {
            Row::SideKeys => (
                "Side keys",
                SettingValue::Choice(String::from(if s.side_keys == crate::settings::SideKeys::Pages { "Pages" } else { "Chapters" })),
            ),
            Row::SwapSide => ("Swap Up and Down", on_off(s.swap_side_keys)),
            Row::OrientationFollow => ("Follow orientation", on_off(s.orientation_follow)),
            Row::LeftHanded => ("Left-handed", on_off(s.left_handed)),
            Row::Tilt => ("Tilt to turn", on_off(s.tilt_turn)),
            Row::Shake => ("Shake to refresh", on_off(s.shake_refresh)),
            Row::Tap => ("Tap to turn", on_off(s.tap_turn)),
            Row::Remote => ("Remote", SettingValue::Text(String::from("pair a page-turner"))),
            Row::SleepAfter => ("Sleep after", SettingValue::Stepper(alloc::format!("{} min", s.sleep_after_min))),
            Row::PowerOffAfter => ("Power off after", SettingValue::Stepper(alloc::format!("{} h", s.power_off_after_min / 60))),
            Row::SleepScreen => ("Sleep screen", SettingValue::Choice(String::from(s.sleep.name()))),
            Row::LockWhenSleeping => ("Lock keys when sleeping", on_off(s.lock_when_sleeping)),
            Row::PowerRefreshes => ("Power key refreshes", on_off(s.power_refreshes)),
            Row::NightJobs => (
                "Night jobs",
                SettingValue::Choice(s.night_jobs_hour.map(quire_library::time::fmt_hour).unwrap_or_else(|| String::from("Off"))),
            ),
            Row::PanelOff => ("Panel off when idle", on_off(s.panel_off)),
            Row::Battery => (
                "Battery",
                SettingValue::Text(cx.env.battery().days_left.map(|d| alloc::format!("{d} days")).unwrap_or_else(|| "—".into())),
            ),
            Row::BookshopLanguage => ("Language", SettingValue::Choice(s.bookshop_language.to_uppercase())),
            Row::Clock24 => ("24-hour clock", on_off(s.clock_24h)),
            Row::Language => ("Language", SettingValue::Choice(String::from("English"))),
            Row::SetTime => ("Time", SettingValue::Text(quire_library::time::fmt_clock(cx.env.now(), s.clock_24h))),
            Row::SimpleMode => ("Simple mode", on_off(s.simple_mode)),
            Row::Rescan => ("Rescan the card", SettingValue::Nav),
            Row::Sources => ("Folders scanned", SettingValue::Text(cx.lib.sources.join(", "))),
            Row::Hostname => ("Name on the network", SettingValue::Text(alloc::format!("{}.local", s.hostname))),
            Row::WifiAtBoot => ("Wi-Fi at boot", on_off(s.wifi_at_boot)),
        }
    }
    fn change<E: Env>(&mut self, cx: &mut Ctx<E>, r: Row, dir: i32) -> Action<E> {
        let s = &mut *cx.settings;
        match r {
            Row::SideKeys => {
                s.side_keys = if s.side_keys == crate::settings::SideKeys::Pages {
                    crate::settings::SideKeys::Chapters
                } else {
                    crate::settings::SideKeys::Pages
                }
            }
            Row::SwapSide => s.swap_side_keys = !s.swap_side_keys,
            Row::OrientationFollow => s.orientation_follow = !s.orientation_follow,
            Row::LeftHanded => {
                s.left_handed = !s.left_handed;
                let rot = if s.left_handed { quire_gfx::Rotation::Flip180 } else { quire_gfx::Rotation::Portrait };
                return Action::System(SysRequest::Orientation(rot));
            }
            Row::Tilt => s.tilt_turn = !s.tilt_turn,
            Row::Shake => s.shake_refresh = !s.shake_refresh,
            Row::Tap => s.tap_turn = !s.tap_turn,
            Row::Remote => {
                return Action::Push(
                    super::Dialog::new(
                        "Pair a remote",
                        "Bluetooth page-turners pair from the Drop page in Transfer mode. Open Drop, then tap Remote on your phone.",
                        "Close",
                        "Open Drop",
                    )
                    .with_result(Result_::Choice(7)),
                )
            }
            Row::SleepAfter => {
                let opts = [2u16, 5, 10, 15, 30, 60];
                let i = opts.iter().position(|o| *o == s.sleep_after_min).unwrap_or(2) as i32;
                s.sleep_after_min = opts[(i + dir).rem_euclid(opts.len() as i32) as usize];
            }
            Row::PowerOffAfter => {
                let opts = [6u16, 12, 24, 72, 168];
                let i = opts.iter().position(|o| *o * 60 == s.power_off_after_min).unwrap_or(3) as i32;
                s.power_off_after_min = opts[(i + dir).rem_euclid(opts.len() as i32) as usize] * 60;
            }
            Row::SleepScreen => return Action::Push(Box::new(super::sleep::Picker::new())),
            Row::LockWhenSleeping => s.lock_when_sleeping = !s.lock_when_sleeping,
            Row::PowerRefreshes => s.power_refreshes = !s.power_refreshes,
            Row::NightJobs => {
                let cur = s.night_jobs_hour.map(|h| h as i32 + 1).unwrap_or(0);
                let n = (cur + dir).rem_euclid(25);
                s.night_jobs_hour = if n == 0 { None } else { Some((n - 1) as u8) };
                return Action::System(SysRequest::NightJobs(s.night_jobs_hour));
            }
            Row::PanelOff => s.panel_off = !s.panel_off,
            Row::Battery => return Action::Push(Box::new(BatteryScreen::new())),
            Row::BookshopLanguage => {
                let opts = ["en", "de", "fr", "es", "it", "nl", "pt", "ru"];
                let i = opts.iter().position(|o| *o == s.bookshop_language).unwrap_or(0) as i32;
                s.bookshop_language = String::from(opts[(i + dir).rem_euclid(opts.len() as i32) as usize]);
            }
            Row::Clock24 => s.clock_24h = !s.clock_24h,
            Row::Language => {}
            Row::SetTime => return Action::Push(Box::new(super::firstrun::TimePicker::new())),
            Row::SimpleMode => s.simple_mode = !s.simple_mode,
            Row::Rescan => return Action::System(SysRequest::Rescan),
            Row::Sources => return Action::Push(KeyboardScreen::new("Folders scanned", &cx.lib.sources.join(", "), "/Books, /").boxed()),
            Row::Hostname => return Action::Push(KeyboardScreen::new("Name on the network", &s.hostname, "quire").boxed()),
            Row::WifiAtBoot => s.wifi_at_boot = !s.wifi_at_boot,
        }
        Action::Redraw
    }
}

impl<E: Env> Screen<E> for GenericSettings {
    fn name(&self) -> &'static str {
        self.name
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, self.title, Some(&page_indicator(self.nav.page(), self.nav.pages())));
        let mut y = widgets::CONTENT_TOP;
        let rows = self.rows.clone();
        for i in self.nav.visible() {
            let (t, v) = self.value(cx, rows[i]);
            setting_row(f, y, row_h, t, &v, if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        rail(f, ["", "Back", "Change", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        let r = self.rows[self.nav.focus];
        match ev.key {
            Key::Up | Key::Down => {
                self.nav.key(ev);
                Action::Redraw
            }
            Key::Left => self.change(cx, r, -1),
            Key::Right | Key::Confirm => self.change(cx, r, 1),
            _ => Action::None,
        }
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        match r {
            Result_::Text(t) => {
                let row = self.rows[self.nav.focus];
                match row {
                    Row::Sources => {
                        let mut v: Vec<String> = t.split(',').map(|s| s.trim()).filter(|s| s.starts_with('/')).map(String::from).collect();
                        if v.is_empty() {
                            v = alloc::vec![String::from("/Books"), String::from("/")];
                        }
                        cx.lib.sources = v;
                        cx.lib.touch();
                    }
                    Row::Hostname => {
                        let h: String =
                            t.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(24).collect::<String>().to_lowercase();
                        if !h.is_empty() {
                            cx.settings.hostname = h;
                        }
                    }
                    _ => {}
                }
                Action::Redraw
            }
            Result_::Choice(7) => Action::Replace(Box::new(super::drop::DropScreen::new())),
            _ => Action::Redraw,
        }
    }
}

/// Keys: a line drawing of the device with each key labelled, plus the key options.
pub struct KeysScreen {
    inner: GenericSettings,
}

impl KeysScreen {
    /// New.
    pub fn new() -> Self {
        KeysScreen {
            inner: GenericSettings::make(
                "Keys",
                "50-keys",
                alloc::vec![
                    Row::SideKeys,
                    Row::SwapSide,
                    Row::OrientationFollow,
                    Row::LeftHanded,
                    Row::Tilt,
                    Row::Shake,
                    Row::Tap,
                    Row::Remote
                ],
            ),
        }
    }
}

impl Default for KeysScreen {
    fn default() -> Self {
        Self::new()
    }
}

/// Height of the device diagram drawn by [`draw_device`].
pub const DEVICE_H: i32 = 250;

/// Draw the device outline with the seven keys labelled, centred on `cx` with its top at
/// `y`: the power key sits at the top-left corner of the body, the two side keys on the
/// right edge and the four front keys along the bottom, named in one line beneath.
pub fn draw_device(f: &mut Frame, cx: i32, y: i32, labels: [&str; 7]) {
    let (bw, bh) = (140, 200);
    let x = cx - bw / 2;
    let body = Rect::new(x, y + 8, bw as u32, bh as u32);
    f.stroke_rect(body, 2, Ink::Black);
    f.stroke_rect(Rect::new(x + 12, y + 20, 116, 150), 1, Ink::Black);
    f.pattern_rect(Rect::new(x + 13, y + 21, 114, 148), Pattern::Hatch { pitch: 6 });
    let mono = quire_fonts::ui::mono();
    // Power: top-left corner, label to its right on the same line.
    f.fill_rect(Rect::new(x + 10, y, 22, 8), Ink::Black);
    draw_text(f, mono, x + 40, y + 8, labels[6], TextStyle::INK);
    // Front keys along the bottom edge, evenly spaced.
    for i in 0..4 {
        let kx = x + 22 + i * 28;
        f.fill_rect(Rect::new(kx, y + bh - 10, 20, 10), Ink::Black);
    }
    let names = alloc::format!("{} · {} · {} · {}", labels[0], labels[1], labels[2], labels[3]);
    crate::text::draw_centered(f, mono, cx, y + bh + 34, &names, TextStyle::INK);
    // Side keys on the right edge, labels beside them.
    for (i, l) in [labels[4], labels[5]].iter().enumerate() {
        let ky = y + 56 + i as i32 * 40;
        f.fill_rect(Rect::new(x + bw - 2, ky, 8, 24), Ink::Black);
        draw_text(f, mono, x + bw + 16, ky + 18, l, TextStyle::INK);
    }
}

impl<E: Env> Screen<E> for KeysScreen {
    fn name(&self) -> &'static str {
        "50-keys"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Keys", None);
        let (up, down) =
            if cx.settings.side_keys == crate::settings::SideKeys::Pages { ("Prev page", "Next page") } else { ("Prev ch", "Next ch") };
        let (u, d) = if cx.settings.swap_side_keys { (down, up) } else { (up, down) };
        // Centre the drawing together with its side-key labels (140 px body + 12 px gap +
        // the wider label), so the group sits on the page's centre.
        let mono = quire_fonts::ui::mono();
        let label_w = quire_gfx::measure_text(mono, u, TextStyle::INK).max(quire_gfx::measure_text(mono, d, TextStyle::INK));
        // The body spans cx ± 70 and the labels run from cx + 86 to cx + 86 + label_w.
        let group_cx = f.width() as i32 / 2 - (16 + label_w) / 2;
        draw_device(f, group_cx, widgets::CONTENT_TOP + 12, ["Left", "Back", "OK", "Right", u, d, "Power"]);
        let row_h = cx.settings.row_h();
        let top = widgets::CONTENT_TOP + 12 + DEVICE_H;
        self.inner.nav.per_page = widgets::rows_between(top, f.height() as i32 - RAIL_H, row_h);
        let mut y = top;
        let rows = self.inner.rows.clone();
        for i in self.inner.nav.visible() {
            let (t, v) = self.inner.value(cx, rows[i]);
            setting_row(f, y, row_h, t, &v, if i == self.inner.nav.focus { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        rail(f, ["", "Back", "Change", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        Screen::<E>::key(&mut self.inner, cx, ev)
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        Screen::<E>::result(&mut self.inner, cx, r)
    }
}

/// Sleep and power.
pub struct SleepPower {
    inner: GenericSettings,
}

impl SleepPower {
    /// New.
    pub fn new() -> Self {
        SleepPower {
            inner: GenericSettings::make(
                "Sleep and power",
                "50-sleepandpower",
                alloc::vec![
                    Row::SleepScreen,
                    Row::SleepAfter,
                    Row::PowerOffAfter,
                    Row::LockWhenSleeping,
                    Row::PowerRefreshes,
                    Row::NightJobs,
                    Row::PanelOff,
                    Row::Battery
                ],
            ),
        }
    }
}

impl Default for SleepPower {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for SleepPower {
    fn name(&self) -> &'static str {
        "50-sleepandpower"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        Screen::<E>::draw(&mut self.inner, cx, f)
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        Screen::<E>::key(&mut self.inner, cx, ev)
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        Screen::<E>::result(&mut self.inner, cx, r)
    }
}

/// Library settings.
pub struct LibrarySettings {
    inner: GenericSettings,
}

impl LibrarySettings {
    /// New.
    pub fn new() -> Self {
        LibrarySettings {
            inner: GenericSettings::make("Library", "50-library", alloc::vec![Row::Sources, Row::Rescan, Row::Hostname, Row::WifiAtBoot]),
        }
    }
}

impl Default for LibrarySettings {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for LibrarySettings {
    fn name(&self) -> &'static str {
        "50-library"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        Screen::<E>::draw(&mut self.inner, cx, f)
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        Screen::<E>::key(&mut self.inner, cx, ev)
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        Screen::<E>::result(&mut self.inner, cx, r)
    }
}

// ---------------------------------------------------------------------------------------
// 50 battery

/// The battery page.
pub struct BatteryScreen;

impl BatteryScreen {
    /// New.
    pub fn new() -> Self {
        BatteryScreen
    }
}

impl Default for BatteryScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for BatteryScreen {
    fn name(&self) -> &'static str {
        "50-battery"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Battery", None);
        let b = cx.env.battery();
        let w = f.width() as i32;
        let tiles = alloc::vec![
            (b.days_left.map(|d| alloc::format!("{d} days")).unwrap_or_else(|| String::from("—")), String::from("Left")),
            (alloc::format!("{}%", b.percent), String::from(if b.charging { "Charging" } else { "Charge" })),
            (b.cycles.map(|c| alloc::format!("{c}")).unwrap_or_else(|| String::from("—")), String::from("Cycles")),
            (b.health.map(|h| alloc::format!("{h}%")).unwrap_or_else(|| String::from("—")), String::from("Health")),
        ];
        let y = poster_tiles(f, widgets::INSET, widgets::CONTENT_TOP, w - 2 * widgets::INSET, &tiles, 2) + 24;
        let fl = quire_fonts::ui::label();
        let lines = [
            String::from("From the battery gauge."),
            String::from("Days left assumes your reading of the last week."),
            String::from("Sleep after a shorter idle time and turn Wi-Fi off to last longer."),
        ];
        let mut yy = y;
        for l in lines {
            for ll in wrap(fl, &l, w - 2 * widgets::INSET) {
                draw_text(f, fl, widgets::INSET, yy + fl.ascent(), &ll, TextStyle::INK);
                yy += line_h(fl);
            }
            yy += 6;
        }
        rail(f, ["", "Back", "", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.is(Key::Back) {
            Action::Pop
        } else {
            Action::None
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if matches!(ev, Event::Battery(_)) {
            Action::Redraw
        } else {
            Action::None
        }
    }
}

// ---------------------------------------------------------------------------------------
// 50 about, 51 OTA

/// About: version, storage bars, check for update, licences.
pub struct About {
    focus: usize,
    checking: bool,
    update: Option<Result<Option<OtaInfo>, String>>,
}

impl About {
    /// New.
    pub fn new() -> Self {
        About { focus: 0, checking: false, update: None }
    }
}

impl Default for About {
    fn default() -> Self {
        Self::new()
    }
}

fn storage_bar(f: &mut Frame, x: i32, y: i32, w: i32, label: &str, used: u64, total: u64) -> i32 {
    let fl = quire_fonts::ui::label();
    let mono = quire_fonts::ui::mono();
    let t = alloc::format!("{} of {}", mb(used), mb(total));
    let tw = quire_gfx::measure_text(mono, &t, TextStyle::INK);
    draw_label(f, x, y + fl.ascent(), &ellipsis(fl, &crate::text::small_caps(label), w - tw - 16), false);
    draw_text(f, mono, x + w - tw, y + fl.ascent(), &t, TextStyle::INK);
    let by = y + line_h(fl) + 4;
    let bar = Rect::new(x, by, w as u32, 16);
    f.stroke_rect(bar, 2, Ink::Black);
    if let Some(frac) = (used.min(total) * (w as u64 - 8)).checked_div(total) {
        f.pattern_rect(Rect::new(x + 4, by + 4, frac as u32, 8), Pattern::Hatch { pitch: 2 });
    }
    by + 28
}

fn mb(b: u64) -> String {
    if b >= 1024 * 1024 * 1024 {
        alloc::format!("{}.{} GB", b / (1024 * 1024 * 1024), (b % (1024 * 1024 * 1024)) * 10 / (1024 * 1024 * 1024))
    } else if b < 1024 * 1024 {
        alloc::format!("{} KB", b / 1024)
    } else {
        alloc::format!("{} MB", b / (1024 * 1024))
    }
}

impl<E: Env> Screen<E> for About {
    fn name(&self) -> &'static str {
        "50-about"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "About", None);
        let d = cx.env.device();
        let w = f.width() as i32;
        let x = widgets::INSET;
        let mono = quire_fonts::ui::mono();
        let fb = quire_fonts::ui::body();
        let mut y = widgets::CONTENT_TOP;
        draw_text(f, quire_fonts::ui::title(), x, y + 26, "Quire", TextStyle::INK);
        draw_text(f, mono, x + 120, y + 26, &alloc::format!("{} · {}", d.version, d.build), TextStyle::INK);
        y += 48;
        draw_text(
            f,
            fb,
            x,
            y + fb.ascent(),
            &ellipsis(fb, &alloc::format!("Panel: {} · serial {}", d.panel, d.serial), w - 2 * x),
            TextStyle::INK,
        );
        y += line_h(fb) + 16;
        y = storage_bar(f, x, y, w - 2 * x, "Flash", (d.flash_bytes as u64).saturating_sub(d.largest_block as u64), d.flash_bytes as u64);
        if let (Some(t), Some(fr)) = (d.card_total, d.card_free) {
            y = storage_bar(f, x, y, w - 2 * x, "Card", t.saturating_sub(fr), t);
        } else {
            draw_label(f, x, y + 18, "No card", false);
            y += 32;
        }
        let books = cx.lib.books.iter().filter(|b| !b.missing).count();
        y = storage_bar(
            f,
            x,
            y,
            w - 2 * x,
            "Heap · largest free block",
            (d.largest_block as u64).min(d.free_heap as u64),
            d.free_heap as u64,
        );
        draw_text(f, fb, x, y + fb.ascent(), &alloc::format!("{books} books in the library"), TextStyle::INK);
        y += line_h(fb) + 12;
        let rows: [(&str, String); 2] = [
            (
                "Check for update",
                match &self.update {
                    None if self.checking => String::from("checking…"),
                    None => String::new(),
                    Some(Ok(None)) => String::from("up to date"),
                    Some(Ok(Some(i))) => alloc::format!("{} available", i.version),
                    Some(Err(e)) => ellipsis(quire_fonts::ui::label(), e, 200),
                },
            ),
            ("Licences", String::from("MIT or Apache-2.0")),
        ];
        for (i, (t, v)) in rows.iter().enumerate() {
            setting_row(f, y, ROW_H, t, &SettingValue::Text(v.clone()), if self.focus == i { RowState::Focused } else { RowState::Normal });
            y += ROW_H;
        }
        rail(f, ["", "Back", "Open", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Up | Key::Down => {
                self.focus ^= 1;
                Action::Redraw
            }
            Key::Confirm => {
                if self.focus == 0 {
                    if let Some(Ok(Some(info))) = &self.update {
                        return Action::Push(Box::new(OtaScreen::available(info.clone())));
                    }
                    if !matches!(cx.env.wifi(), crate::WifiState::Connected { .. }) {
                        return Action::Push(Box::new(super::wifi::WifiScreen::new_with_hint("Updates need Wi-Fi")));
                    }
                    self.checking = true;
                    self.update = None;
                    cx.env.request(SysRequest::Fetch(FetchRequest::OtaCheck));
                    Action::Redraw
                } else {
                    Action::Push(Box::new(Licences))
                }
            }
            _ => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if let Event::Net(NetEvent::Ota(r)) = ev {
            self.checking = false;
            self.update = Some(r.clone());
            return Action::Redraw;
        }
        Action::None
    }
}

/// Licences page.
pub struct Licences;

impl<E: Env> Screen<E> for Licences {
    fn name(&self) -> &'static str {
        "50-licences"
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Licences", None);
        let fb = quire_fonts::ui::body();
        let text = "Quire is free software under the MIT or Apache-2.0 licence, at your option.\n\nFonts: Literata (OFL), Atkinson Hyperlegible (OFL), JetBrains Mono (OFL).\n\nBuilt with esp-hal, embassy, miniz_oxide, hypher, unicode-linebreak, pulldown-cmark, postcard, qrcodegen and other Rust crates under MIT, Apache-2.0 or BSD licences. Full texts ship in the source repository.";
        let mut y = widgets::CONTENT_TOP;
        for l in wrap(fb, text, f.width() as i32 - 2 * widgets::INSET) {
            draw_text(f, fb, widgets::INSET, y + fb.ascent(), &l, TextStyle::INK);
            y += line_h(fb);
        }
        rail(f, ["", "Back", "", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.is(Key::Back) {
            Action::Pop
        } else {
            Action::None
        }
    }
}

/// 51 OTA: available (paginated notes, Install), working, restart dialog.
pub struct OtaScreen {
    info: Option<OtaInfo>,
    page: usize,
    working: bool,
    done: u64,
    total: u64,
    error: Option<String>,
    finished: bool,
}

impl OtaScreen {
    /// An available update.
    pub fn available(info: OtaInfo) -> Self {
        OtaScreen { info: Some(info), page: 0, working: false, done: 0, total: 0, error: None, finished: false }
    }
    /// An SD-card update (`/quire-update.bin`).
    pub fn from_card() -> Self {
        OtaScreen { info: None, page: 0, working: false, done: 0, total: 0, error: None, finished: false }
    }
}

impl<E: Env> Screen<E> for OtaScreen {
    fn name(&self) -> &'static str {
        if self.working {
            "51-ota-working"
        } else {
            "51-ota-available"
        }
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let w = f.width() as i32;
        if self.working {
            let permille = (self.done * 1000).checked_div(self.total).unwrap_or(0) as u32;
            let status =
                if self.total > 0 { alloc::format!("{} of {}", mb(self.done), mb(self.total)) } else { String::from("connecting") };
            running_head(f, "Updating", None);
            widgets::working_card(f, "Installing", "Never touch Power", permille, &status);
            let fl = quire_fonts::ui::label();
            draw_text(
                f,
                fl,
                widgets::INSET,
                f.height() as i32 - RAIL_H - 20,
                "The update is verified before it replaces anything.",
                TextStyle::INK,
            );
            rail(f, ["", "", "", ""], None);
            return Refresh::Du;
        }
        running_head(f, "Update", None);
        let fb = quire_fonts::ui::body();
        let ft = quire_fonts::ui::title();
        let mut y = widgets::CONTENT_TOP;
        let (title, notes) = match &self.info {
            Some(i) => (alloc::format!("Version {}", i.version), alloc::format!("{}\n\n{}", i.notes, mb(i.size))),
            None => (String::from("Update from the card"), String::from("A quire-update.bin file was found on the card. It is checked and verified before installing; the previous version stays as a fallback.")),
        };
        draw_text(f, ft, widgets::INSET, y + ft.ascent(), &title, TextStyle::INK);
        y += ft.ascent() + ft.below() + 12;
        if let Some(e) = &self.error {
            for l in wrap(fb, &alloc::format!("Couldn't update. {e}"), w - 2 * widgets::INSET) {
                draw_text(f, fb, widgets::INSET, y + fb.ascent(), &l, TextStyle::INK);
                y += line_h(fb);
            }
        } else {
            let per = ((f.height() as i32 - RAIL_H - y - 16) / line_h(fb)).max(4) as usize;
            let pages = crate::text::paginate(fb, &notes, w - 2 * widgets::INSET, per);
            for l in pages.get(self.page).into_iter().flatten() {
                draw_text(f, fb, widgets::INSET, y + fb.ascent(), l, TextStyle::INK);
                y += line_h(fb);
            }
            let _ = cx;
        }
        rail(f, ["", "Back", "Install", "More"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press || self.working {
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
                self.working = true;
                self.error = None;
                let url = self.info.as_ref().map(|i| i.url.clone()).unwrap_or_else(|| String::from("/quire-update.bin"));
                cx.env.request(SysRequest::Ota(url));
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if let Event::Net(NetEvent::OtaProgress { done, total, finished }) = ev {
            self.done = *done;
            self.total = *total;
            match finished {
                Some(Ok(())) => {
                    self.finished = true;
                    self.working = false;
                    return Action::Push(super::Dialog::new(
                        "Update installed",
                        "Restart to run the new version. Your page is saved.",
                        "Later",
                        "Restart",
                    ));
                }
                Some(Err(e)) => {
                    self.working = false;
                    self.error = Some(e.clone());
                }
                None => {}
            }
            return Action::Redraw;
        }
        Action::None
    }
    fn result(&mut self, _cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if r == Result_::Choice(1) {
            return Action::System(SysRequest::Restart);
        }
        Action::Redraw
    }
}

// ---------------------------------------------------------------------------------------
// 90 developer

/// Developer: mono diagnostics.
pub struct Developer {
    heap: Vec<u32>,
}

impl Developer {
    /// New.
    pub fn new() -> Self {
        Developer { heap: Vec::new() }
    }
}

impl Default for Developer {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Developer {
    fn name(&self) -> &'static str {
        "90-developer"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Developer", None);
        let d = cx.env.device();
        self.heap.push(d.free_heap / 1024);
        if self.heap.len() > 40 {
            self.heap.remove(0);
        }
        let mono = quire_fonts::ui::mono();
        let x = widgets::INSET;
        let mut y = widgets::CONTENT_TOP;
        let w = f.width() as i32;
        let lines = [
            alloc::format!("fw {} {}", d.version, d.build),
            alloc::format!("panel {}", d.panel),
            alloc::format!("heap free {} KB, largest {} KB", d.free_heap / 1024, d.largest_block / 1024),
            alloc::format!("wifi {:?}", cx.env.wifi()),
            alloc::format!("battery {}% {} mV", cx.env.battery().percent, cx.env.battery().millivolts),
            alloc::format!("books {} · ingesting {}", cx.lib.books.len(), cx.ingesting.len()),
            alloc::format!("stats sessions {} · log {} B", cx.stats.sessions, cx.stats.log_len),
            alloc::format!("time {} · day {}", cx.env.now(), cx.today()),
        ];
        for l in lines {
            draw_text(f, mono, x, y + mono.ascent(), &ellipsis(mono, &l, w - 2 * x), TextStyle::INK);
            y += line_h(mono);
        }
        y += 12;
        let fl = quire_fonts::ui::label();
        draw_label(f, x, y + 14, "Heap free, KB", false);
        y += 24;
        if self.heap.len() < 2 {
            // One sample is not a graph yet.
            draw_text(f, mono, x, y + mono.ascent(), "collecting…", TextStyle::INK);
        } else {
            // A 2 px stepped line, as the brief draws the heap.
            widgets::step_line(f, Rect::new(x, y, (w - 2 * x) as u32, 100), &self.heap);
            let ay = y + 100 + 6 + mono.ascent();
            draw_text(f, mono, x, ay, "40 s ago", TextStyle::INK);
            crate::text::draw_right(f, mono, w - x, ay, "now", TextStyle::INK);
        }
        y += 130;
        // Key ADC readings are shown live by the platform via the device info serial field.
        draw_text(f, fl, x, y + fl.ascent(), "Power + Down saves a screenshot to the card.", TextStyle::INK);
        rail(f, ["", "Back", "Refresh", "Screenshot"], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Confirm => Action::Redraw,
            Key::Right => Action::System(SysRequest::Screenshot),
            _ => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if matches!(ev, Event::Tick) {
            Action::Redraw
        } else {
            Action::None
        }
    }
}

/// 99 recovery: 32 px text, two actions.
pub struct Recovery {
    reason: String,
}

impl Recovery {
    /// New.
    pub fn new(reason: &str) -> Self {
        Recovery { reason: reason.into() }
    }
}

impl<E: Env> Screen<E> for Recovery {
    fn name(&self) -> &'static str {
        "99-recovery"
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let ft = quire_fonts::ui::title();
        let w = f.width() as i32;
        let mut y = 120;
        for l in wrap(ft, "Recovery", w - 2 * widgets::INSET) {
            draw_text(f, ft, widgets::INSET, y + ft.ascent(), &l, TextStyle::INK);
            y += line_h(ft);
        }
        y += 16;
        let fb = quire_fonts::ui::body();
        let text = alloc::format!("{}\n\nInstall quire-update.bin from the card, or restart the previous version.", self.reason);
        for l in wrap(fb, &text, w - 2 * widgets::INSET) {
            draw_text(f, fb, widgets::INSET, y + fb.ascent(), &l, TextStyle::INK);
            y += line_h(fb);
        }
        rail(f, ["", "Restart", "Install", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::System(SysRequest::Restart),
            Key::Confirm => Action::Push(Box::new(OtaScreen::from_card())),
            _ => Action::None,
        }
    }
}

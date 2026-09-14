//! 40 sleep screens (cover, poster, quote, quick resume, custom, charging, empty),
//! 44 the sleep-screen picker with live thumbnails.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::{draw_text, BlitMode, Frame, Ink, Pattern, Rect, TextStyle};

use crate::icons::{self, Icon};
use crate::settings::SleepVariant;
use crate::spine;
use crate::text::{centered_baseline, draw_centered, draw_label, ellipsis, line_h, wrap};
use crate::theme::*;
use crate::widgets::{self, rail, running_head, setting_row, RowState, SettingValue};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Screen, SysRequest};

use super::left_line;

/// Built-in quotes (author · work).
const QUOTES: &[(&str, &str)] = &[
    ("Rather than love, than money, than fame, give me truth.", "Thoreau · Walden"),
    (
        "It is a truth universally acknowledged, that a single man in possession of a good fortune, must be in want of a wife.",
        "Austen · Pride and Prejudice",
    ),
    ("Call me Ishmael.", "Melville · Moby-Dick"),
    ("All that we see or seem is but a dream within a dream.", "Poe · A Dream Within a Dream"),
    ("The world is a book, and those who do not travel read only one page.", "Augustine"),
    ("We read to know we are not alone.", "attributed to C. S. Lewis"),
    ("A reader lives a thousand lives before he dies. The man who never reads lives only one.", "Martin · A Dance with Dragons"),
    ("There is no friend as loyal as a book.", "Hemingway"),
    ("Books are a uniquely portable magic.", "King · On Writing"),
    ("Until I feared I would lose it, I never loved to read. One does not love breathing.", "Lee · To Kill a Mockingbird"),
    ("So many books, so little time.", "Zappa"),
    ("The reading of all good books is like a conversation with the finest minds of past centuries.", "Descartes"),
    ("I have always imagined that Paradise will be a kind of library.", "Borges"),
    ("You can never get a cup of tea large enough or a book long enough to suit me.", "C. S. Lewis"),
];

/// The sleep screen shown while the device sleeps.
pub struct SleepScreen {
    variant: Option<SleepVariant>,
    /// True when shown as a 3-second preview from the picker.
    preview: bool,
    /// True when drawn for the charging state.
    charging: bool,
}

impl SleepScreen {
    /// The configured sleep screen.
    pub fn new() -> Self {
        SleepScreen { variant: None, preview: false, charging: false }
    }
    /// A specific variant, as a preview.
    pub fn preview(v: SleepVariant) -> Self {
        SleepScreen { variant: Some(v), preview: true, charging: false }
    }
    /// The charging screen.
    pub fn charging() -> Self {
        SleepScreen { variant: None, preview: false, charging: true }
    }
}

impl Default for SleepScreen {
    fn default() -> Self {
        Self::new()
    }
}

/// Quote of the day: from `quotes.txt` on the card (one per line, `text | source`) or built in.
pub fn quote_of_day<E: Env>(cx: &Ctx<E>) -> (String, String) {
    let day = quire_library::time::day_of(cx.env.now()) as usize;
    if cx.settings.sleep_quotes_card {
        if let Ok(bytes) = cx.env.fs().read_to_vec("/quotes.txt") {
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
            if !lines.is_empty() {
                let l = lines[day % lines.len()];
                let (q, src) = l.split_once('|').unwrap_or((l, ""));
                return (String::from(q.trim()), String::from(src.trim()));
            }
        }
    }
    let (q, s) = QUOTES[day % QUOTES.len()];
    (String::from(q), String::from(s))
}

/// Pick the custom image file for this sleep.
pub fn custom_image<E: Env>(cx: &mut Ctx<E>) -> Option<quire_gfx::Bitmap> {
    let folder = cx.settings.sleep_folder.clone();
    let mut files: Vec<String> = cx
        .env
        .fs()
        .read_dir(&folder)
        .ok()?
        .into_iter()
        .filter(|e| !e.is_dir && e.name.to_ascii_lowercase().ends_with(".pbm"))
        .map(|e| e.name)
        .collect();
    files.sort();
    if files.is_empty() {
        return None;
    }
    let name = match cx.settings.sleep_rotation {
        crate::settings::ImageRotation::Fixed => {
            cx.settings.sleep_image.clone().filter(|n| files.contains(n)).unwrap_or_else(|| files[0].clone())
        }
        crate::settings::ImageRotation::Daily => files[(quire_library::time::day_of(cx.env.now()) as usize) % files.len()].clone(),
        crate::settings::ImageRotation::EachSleep => {
            let i = (cx.env.random() as usize) % files.len();
            files[i].clone()
        }
    };
    quire_library::cache::load_pbm(cx.env.fs(), &quire_fs::join(&folder, &name))
}

/// Draw a sleep variant into the frame at full size.
pub fn draw_variant<E: Env>(cx: &mut Ctx<E>, f: &mut Frame, v: SleepVariant, locked: bool) {
    f.clear(Ink::White);
    let w = f.width() as i32;
    let h = f.height() as i32;
    let now = cx.env.now();
    let today = quire_library::time::day_of(now);
    let (title, author, ch_secs, book_secs, model) = match cx.reader.as_mut() {
        Some(r) => {
            let (c, b) = r.time_left(cx.lib, cx.stats);
            let author = cx.lib.get(r.id).map(|e| e.author_line()).unwrap_or_default();
            (r.book.meta.title.clone(), author, c, b, Some(r.spine_model()))
        }
        None => (String::from("Quire"), String::new(), 0, 0, None),
    };
    match v {
        SleepVariant::Cover => {
            let cover = cx.reader.as_ref().and_then(|r| quire_library::cache::load_cover(cx.env.fs(), r.id));
            match cover {
                Some(bm) => {
                    let ox = (w - bm.w as i32) / 2;
                    let oy = (h - bm.h as i32) / 2;
                    f.blit(ox, oy, bm.as_ref(), BlitMode::Or);
                }
                None => {
                    widgets::typographic_cover(f, Rect::new(0, 0, w as u32, h as u32), &title, &author);
                }
            }
            if cx.settings.sleep_band && cx.reader.is_some() {
                let band_h = 96;
                let band = Rect::new(0, h - band_h, w as u32, band_h as u32);
                f.fill_rect(band, Ink::White);
                f.fill_rect(Rect::new(0, band.y, w as u32, RULE), Ink::Black);
                let ft = quire_fonts::ui::list_title();
                let fb = quire_fonts::ui::body();
                draw_text(f, ft, 32, band.y + 22 + ft.ascent(), &ellipsis(ft, &title, w - 120), TextStyle::INK);
                draw_text(
                    f,
                    fb,
                    32,
                    band.y + 22 + ft.ascent() + ft.descent() + 8 + fb.ascent(),
                    &ellipsis(fb, &left_line(ch_secs, "this chapter"), w - 120),
                    TextStyle::INK,
                );
                if let Some(m) = &model {
                    spine::draw_mini(f, Rect::new(w - 32 - 6, band.y + 16, 6, (band_h - 32) as u32), m, Ink::Black);
                }
                if locked {
                    icons::draw(f, Icon::Lock, w - 72, band.y + 36, Ink::Black);
                }
            }
        }
        SleepVariant::Poster => {
            let hero = quire_fonts::ui::hero();
            let fl = quire_fonts::ui::label();
            let fb = quire_fonts::ui::body();
            let ft = quire_fonts::ui::title();
            let cx_ = w / 2;
            let (big, small) = if cx.settings.sleep_streak {
                let (cur, _) = cx.stats.streaks(today);
                (alloc::format!("{cur}"), String::from("Streak days"))
            } else if cx.reader.is_some() {
                let fin = cx.stats.finish_day(today, book_secs);
                (super::finish_word(today, fin), String::from("Finish by"))
            } else {
                (
                    quire_library::time::fmt_clock(now, cx.settings.clock_24h),
                    String::from(quire_library::time::weekday_name_long(quire_library::time::weekday(today))),
                )
            };
            let big_font = if quire_gfx::measure_text(hero, &big, TextStyle::INK) > w - 64 { quire_fonts::ui::poster() } else { hero };
            draw_centered(f, fl, cx_, h / 2 - 80, &crate::text::small_caps(&small), crate::text::label_style(false));
            draw_centered(f, big_font, cx_, h / 2 - 80 + 20 + big_font.ascent(), &big, TextStyle::INK);
            draw_centered(f, ft, cx_, h / 2 + 80 + ft.ascent(), &ellipsis(ft, &title, w - 64), TextStyle::INK);
            draw_centered(
                f,
                fb,
                cx_,
                h / 2 + 80 + ft.ascent() + ft.descent() + 8 + fb.ascent(),
                &ellipsis(fb, &author, w - 64),
                TextStyle::INK,
            );
            if locked {
                icons::draw(f, Icon::Lock, w / 2 - 12, h - 80, Ink::Black);
            }
        }
        SleepVariant::Quote => {
            let (q, src) = quote_of_day(cx);
            let fq = quire_fonts::ui::title();
            let fs = quire_fonts::ui::label();
            let lines = wrap(fq, &q, w - 112);
            let total = lines.len() as i32 * line_h(fq) + 40 + line_h(fs);
            let mut y = (h - total) / 2;
            draw_text(f, quire_fonts::ui::hero(), 40, y - 10, "\u{201c}", TextStyle::INK);
            for l in &lines {
                draw_text(f, fq, 56, y + fq.ascent(), l, TextStyle::INK);
                y += line_h(fq);
            }
            y += 24;
            draw_label(f, 56, y + fs.ascent(), &src, false);
        }
        SleepVariant::QuickResume => {
            if let Some(r) = cx.reader.as_mut() {
                r.render(cx.env.fs(), f, cx.settings);
            }
            f.screen_rect(f.bounds(), Pattern::Dots50);
            let fl = quire_fonts::ui::label();
            let card = Rect::new(w / 2 - 90, h / 2 - 44, 180, 88);
            f.fill_rect(card, Ink::White);
            f.stroke_rect(card, 2, Ink::Black);
            if cx.settings.sleep_moon {
                icons::draw(f, Icon::Moon, w / 2 - 12, card.y + 14, Ink::Black);
                draw_centered(f, fl, w / 2, card.y + 66, "Press Power", TextStyle::INK);
            } else {
                draw_centered(f, fl, w / 2, centered_baseline(fl, card.y, 88), "Press Power", TextStyle::INK);
            }
        }
        SleepVariant::Custom => match custom_image(cx) {
            Some(bm) => {
                let ox = (w - bm.w as i32) / 2;
                let oy = (h - bm.h as i32) / 2;
                f.blit(ox, oy, bm.as_ref(), BlitMode::Or);
            }
            None => {
                widgets::empty_state(
                    f,
                    h / 2 - 60,
                    "No sleep images yet",
                    "Drop photos into the Drop page's Sleep images, or the /sleep folder on the card.",
                );
            }
        },
        SleepVariant::Blank => {}
    }
}

impl<E: Env> Screen<E> for SleepScreen {
    fn name(&self) -> &'static str {
        if self.charging {
            "40-sleep-charging"
        } else if self.preview {
            "44-picker-preview"
        } else {
            "40-sleep"
        }
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let battery = cx.env.battery();
        if self.charging || (battery.charging && !self.preview) {
            f.clear(Ink::White);
            let w = f.width() as i32;
            let h = f.height() as i32;
            let hero = quire_fonts::ui::hero();
            draw_centered(f, hero, w / 2, h / 2 - 20, &alloc::format!("{}%", battery.percent), TextStyle::INK);
            let line = match battery.days_left {
                Some(d) if battery.percent >= 100 => alloc::format!("Charged · {d} days"),
                Some(d) => alloc::format!("Charging · {d} days when full"),
                None => String::from("Charging"),
            };
            draw_centered(f, quire_fonts::ui::body(), w / 2, h / 2 + 40, &line, TextStyle::INK);
            return Refresh::Gc;
        }
        if battery.percent <= 2 && !battery.charging && !self.preview {
            f.clear(Ink::White);
            let w = f.width() as i32;
            let h = f.height() as i32;
            draw_centered(f, quire_fonts::ui::title(), w / 2, h / 2 - 20, "Battery empty", TextStyle::INK);
            let fb = quire_fonts::ui::body();
            for (i, l) in wrap(fb, "Charge with the magnetic cable. Your page is saved.", w - 96).iter().enumerate() {
                draw_centered(f, fb, w / 2, h / 2 + 24 + i as i32 * line_h(fb), l, TextStyle::INK);
            }
            return Refresh::Gc;
        }
        let v = self.variant.unwrap_or(cx.settings.sleep);
        let locked = cx.locked || cx.settings.lock_when_sleeping;
        draw_variant(cx, f, v, locked && !self.preview);
        if self.preview {
            let fl = quire_fonts::ui::mono();
            let w = f.width() as i32;
            f.fill_rect(Rect::new(w - 64, 8, 56, 28), Ink::White);
            draw_centered(f, fl, w - 36, 28, "3 s", TextStyle::INK);
        }
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if self.preview {
            return Action::Pop;
        }
        // Asleep: the platform handles Power; any other key is ignored.
        match ev.key {
            Key::Power if ev.kind == KeyKind::Press => Action::Pop,
            _ => {
                let _ = cx;
                Action::None
            }
        }
    }
    fn resume(&mut self, _cx: &mut Ctx<E>) {}
    fn event(&mut self, cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::Wake => Action::Pop,
            Event::Timer if self.preview => Action::Pop,
            Event::Battery(_) if !self.preview => Action::Redraw,
            _ => {
                let _ = cx;
                Action::None
            }
        }
    }
}

/// The platform asks for sleep once the sleep screen is drawn: a helper to push it and
/// request the sleep in one action.
pub fn sleep_now<E: Env>() -> Action<E> {
    Action::Push(Box::new(SleepScreen::new()))
}

/// The sleep-screen picker: six live thumbnails at 1:4 and options for the focused one.
pub struct Picker {
    focus: usize,
    opt_focus: usize,
    thumbs: Vec<(SleepVariant, quire_gfx::Bitmap)>,
    /// Focus is in the option rows rather than the grid.
    in_options: bool,
}

impl Picker {
    /// New, focused on the variant in use.
    pub fn new() -> Self {
        Picker { focus: 0, opt_focus: 0, thumbs: Vec::new(), in_options: false }
    }
    fn render_thumbs<E: Env>(&mut self, cx: &mut Ctx<E>) {
        self.thumbs.clear();
        for v in SleepVariant::ALL {
            let mut full = Frame::panel();
            draw_variant(cx, &mut full, v, false);
            // 1:4 by sampling: a pixel is ink if any of its 4×4 source pixels is ink for
            // text; for the cover use the majority so dithers stay grey-ish.
            let (tw, th) = (full.width() / 4, full.height() / 4);
            let mut bm = quire_gfx::Bitmap::new(tw, th);
            for y in 0..th {
                for x in 0..tw {
                    let mut n = 0;
                    for dy in 0..4 {
                        for dx in 0..4 {
                            if full.get((x * 4 + dx) as i32, (y * 4 + dy) as i32) {
                                n += 1;
                            }
                        }
                    }
                    if n >= 5 {
                        bm.set(x, y, true);
                    }
                }
            }
            self.thumbs.push((v, bm));
        }
    }
    fn options(&self, cx: &Ctx<impl Env>) -> Vec<(&'static str, SettingValue)> {
        let s = &*cx.settings;
        match SleepVariant::ALL[self.focus] {
            SleepVariant::Cover => alloc::vec![("Title band", SettingValue::Toggle(s.sleep_band))],
            SleepVariant::Poster => {
                alloc::vec![("Show", SettingValue::Choice(String::from(if s.sleep_streak { "Streak" } else { "Finish by" })))]
            }
            SleepVariant::Quote => alloc::vec![(
                "Quotes",
                SettingValue::Choice(String::from(if s.sleep_quotes_card { "quotes.txt on card" } else { "Built in" }))
            )],
            SleepVariant::Custom => alloc::vec![
                ("Folder", SettingValue::Text(s.sleep_folder.clone())),
                (
                    "Rotation",
                    SettingValue::Choice(String::from(match s.sleep_rotation {
                        crate::settings::ImageRotation::Fixed => "Fixed",
                        crate::settings::ImageRotation::EachSleep => "Each sleep",
                        crate::settings::ImageRotation::Daily => "Daily",
                    }))
                ),
            ],
            SleepVariant::QuickResume => alloc::vec![("Moon glyph", SettingValue::Toggle(s.sleep_moon))],
            SleepVariant::Blank => Vec::new(),
        }
    }
    fn change<E: Env>(&mut self, cx: &mut Ctx<E>) {
        let s = &mut *cx.settings;
        match (SleepVariant::ALL[self.focus], self.opt_focus) {
            (SleepVariant::Cover, _) => s.sleep_band = !s.sleep_band,
            (SleepVariant::Poster, _) => s.sleep_streak = !s.sleep_streak,
            (SleepVariant::Quote, _) => s.sleep_quotes_card = !s.sleep_quotes_card,
            (SleepVariant::Custom, 1) => {
                s.sleep_rotation = match s.sleep_rotation {
                    crate::settings::ImageRotation::Fixed => crate::settings::ImageRotation::EachSleep,
                    crate::settings::ImageRotation::EachSleep => crate::settings::ImageRotation::Daily,
                    crate::settings::ImageRotation::Daily => crate::settings::ImageRotation::Fixed,
                }
            }
            (SleepVariant::QuickResume, _) => s.sleep_moon = !s.sleep_moon,
            _ => {}
        }
        self.thumbs.clear();
    }
}

impl Default for Picker {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Picker {
    fn name(&self) -> &'static str {
        "44-picker"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if self.thumbs.is_empty() {
            self.render_thumbs(cx);
        }
        running_head(f, "Sleep screen", None);
        let w = f.width() as i32;
        let (tw, th) = (132i32, 198i32);
        let gap = (w - 2 * widgets::INSET - 3 * tw) / 2;
        let fl = quire_fonts::ui::label();
        for (i, (v, bm)) in self.thumbs.iter().enumerate() {
            let (c, r) = (i % 3, i / 3);
            let x = widgets::INSET + c as i32 * (tw + gap);
            let y = widgets::CONTENT_TOP + r as i32 * (th + 40);
            let focused = i == self.focus && !self.in_options;
            f.blit(x, y, bm.as_ref(), BlitMode::Or);
            f.stroke_rect(Rect::new(x - 1, y - 1, (tw + 2) as u32, (th + 2) as u32), if focused { 4 } else { 1 }, Ink::Black);
            let name = if cx.settings.sleep == *v { alloc::format!("{} ✓", v.name()) } else { String::from(v.name()) };
            draw_centered(f, fl, x + tw / 2, y + th + 8 + fl.ascent(), &name, TextStyle::INK);
        }
        let mut y = widgets::CONTENT_TOP + 2 * (th + 40) + 4;
        let opts = self.options(cx);
        let row_h = ROW_H;
        for (i, (t, v)) in opts.iter().enumerate() {
            setting_row(f, y, row_h, t, v, if self.in_options && i == self.opt_focus { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        let hint = "Options follow the focused thumbnail. Confirm applies it.";
        draw_text(f, fl, widgets::INSET, y + 12 + fl.ascent(), &ellipsis(fl, hint, w - 2 * widgets::INSET), TextStyle::INK);
        rail(f, ["", "Back", "Use", "Preview"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        let n_opts = self.options(cx).len();
        match ev.key {
            Key::Back => Action::Pop,
            Key::Right => {
                if self.in_options {
                    self.change(cx);
                    return Action::Redraw;
                }
                cx.env.request(SysRequest::Timer(3000));
                Action::Push(Box::new(SleepScreen::preview(SleepVariant::ALL[self.focus])))
            }
            Key::Confirm => {
                if self.in_options {
                    self.change(cx);
                } else {
                    cx.settings.sleep = SleepVariant::ALL[self.focus];
                }
                Action::Redraw
            }
            Key::Left => {
                if self.in_options {
                    self.change(cx);
                } else {
                    self.focus = (self.focus + 5) % 6;
                }
                Action::Redraw
            }
            Key::Up => {
                if self.in_options {
                    if self.opt_focus == 0 {
                        self.in_options = false;
                    } else {
                        self.opt_focus -= 1;
                    }
                } else if self.focus >= 3 {
                    self.focus -= 3;
                } else {
                    self.focus = (self.focus + 1) % 3;
                }
                Action::Redraw
            }
            Key::Down => {
                if self.in_options {
                    if self.opt_focus + 1 < n_opts {
                        self.opt_focus += 1;
                    }
                } else if self.focus < 3 {
                    self.focus += 3;
                } else if n_opts > 0 {
                    self.in_options = true;
                    self.opt_focus = 0;
                }
                Action::Redraw
            }
            Key::Power => Action::None,
        }
    }
}

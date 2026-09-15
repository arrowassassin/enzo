//! 40 sleep screens (cover, poster, quote, quick resume, images, charging, empty),
//! 44 the sleep-screen picker with live thumbnails.
//!
//! The Images variant shows a loose `.pbm` from the sleep folder or an image of an
//! installed pack (`sleeppack`), with the live time drawn in the image's clock slot (a
//! plate over a loose image). While the device sleeps the platform ticks once a minute
//! and the `Ui` calls [`Screen::minute_tick`] on the screen with the frame it last drew,
//! so only the slot is repainted: no image is read from the card again.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::{draw_text, measure_text, BlitMode, Frame, Ink, Pattern, Rect, TextStyle};

use crate::icons::{self, Icon};
use crate::settings::{ImageRotation, SleepVariant};
use crate::sleeppack::{self, Clock, ClockSlot, ClockStyle, PackInfo, Surface, ALL_PACKS};
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
    /// What the last draw put in the frame: what a minute tick has to repaint, and the
    /// image to keep on a redraw (a battery change must not rotate the picture).
    drawn: Option<Drawn>,
}

/// What a sleep screen's draw left in the frame.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Drawn {
    /// Nothing on it tells the time.
    Still,
    /// It shows the time: redraw every minute.
    Timed,
    /// It changes with the day (a finish-by poster, the quote of the day).
    Daily(u16),
    /// An image, with its clock if one is drawn.
    Image {
        /// The image shown.
        source: ImageSource,
        /// Where its time is; a tick repaints just this.
        clock: Option<Clock>,
    },
}

/// An image the Images sleep screen can show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageSource {
    /// A loose `.pbm` file at this path.
    Loose(String),
    /// Image `index` of the installed pack `id`.
    Pack {
        /// Pack folder name.
        id: String,
        /// Index into the pack's images.
        index: usize,
    },
}

impl SleepScreen {
    /// The configured sleep screen.
    pub fn new() -> Self {
        SleepScreen { variant: None, preview: false, charging: false, drawn: None }
    }
    /// A specific variant, as a preview.
    pub fn preview(v: SleepVariant) -> Self {
        SleepScreen { variant: Some(v), preview: true, charging: false, drawn: None }
    }
    /// The charging screen.
    pub fn charging() -> Self {
        SleepScreen { variant: None, preview: false, charging: true, drawn: None }
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

/// The loose `.pbm` files in the sleep folder, sorted by name.
fn loose_images<E: Env>(cx: &Ctx<E>) -> Vec<String> {
    let mut files: Vec<String> = cx
        .env
        .fs()
        .read_dir(&cx.settings.sleep_folder)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir && e.name.to_ascii_lowercase().ends_with(".pbm"))
        .map(|e| e.name)
        .collect();
    files.sort();
    files
}

/// The pick a rotation makes among `n` items: the fixed choice (`fixed`, when it is one
/// of them), the day's, or a random one (`random` is only drawn then).
fn rotate(rotation: ImageRotation, n: usize, fixed: Option<usize>, day: u16, random: impl FnOnce() -> u32) -> usize {
    match rotation {
        ImageRotation::Fixed => fixed.unwrap_or(0),
        ImageRotation::Daily => day as usize % n,
        ImageRotation::EachSleep => random() as usize % n,
    }
}

/// The images the source setting offers, with the fixed choice's place among them.
enum Pool {
    /// Every installed pack, flattened in pack order.
    Packs(Vec<PackInfo>),
    /// One pack.
    Pack(String, usize),
    /// The loose files of the sleep folder.
    Loose(Vec<String>),
}

impl Pool {
    fn len(&self) -> usize {
        match self {
            Pool::Packs(packs) => packs.iter().map(|p| p.images).sum(),
            Pool::Pack(_, n) => *n,
            Pool::Loose(files) => files.len(),
        }
    }
}

/// The pool the source setting names (an unusable pack falls back to the loose files) and
/// where the fixed choice sits in it.
fn image_pool<E: Env>(cx: &Ctx<E>) -> (Pool, Option<usize>) {
    let s = &*cx.settings;
    let fs = cx.env.fs();
    match s.sleep_pack.as_deref() {
        Some(ALL_PACKS) => {
            let packs = sleeppack::list_packs(fs, &s.sleep_folder);
            if packs.iter().any(|p| p.images > 0) {
                // A fixed choice is "<pack>/<file>"; its flat index is found in its pack.
                let fixed = s.sleep_image.as_deref().and_then(|f| {
                    let (id, file) = f.split_once('/')?;
                    let before: usize = packs.iter().take_while(|p| p.id != id).map(|p| p.images).sum();
                    let pack = sleeppack::load_pack(fs, &s.sleep_folder, id)?;
                    Some(before + pack.images.iter().position(|i| i.file == file)?)
                });
                return (Pool::Packs(packs), fixed);
            }
        }
        Some(id) => {
            if let Some(pack) = sleeppack::load_pack(fs, &s.sleep_folder, id) {
                let fixed = s.sleep_image.as_deref().and_then(|f| pack.images.iter().position(|i| i.file == f));
                return (Pool::Pack(String::from(id), pack.images.len()), fixed);
            }
        }
        None => {}
    }
    let files = loose_images(cx);
    let fixed = s.sleep_image.as_deref().and_then(|f| files.iter().position(|n| n == f));
    (Pool::Loose(files), fixed)
}

/// The image for this sleep, chosen by the source and rotation settings: an image of the
/// chosen pack (or of all packs), else a loose file in the sleep folder. `None` when the
/// card has nothing to show.
pub fn choose_image<E: Env>(cx: &mut Ctx<E>) -> Option<ImageSource> {
    let (pool, fixed) = image_pool(cx);
    let n = pool.len();
    if n == 0 {
        return None;
    }
    let (rotation, day) = (cx.settings.sleep_rotation, cx.today());
    let i = rotate(rotation, n, fixed, day, || cx.env.random());
    match pool {
        Pool::Packs(packs) => {
            let mut rest = i;
            for p in packs {
                if rest < p.images {
                    return Some(ImageSource::Pack { id: p.id, index: rest });
                }
                rest -= p.images;
            }
            None
        }
        Pool::Pack(id, _) => Some(ImageSource::Pack { id, index: i }),
        Pool::Loose(files) => Some(ImageSource::Loose(quire_fs::join(&cx.settings.sleep_folder, &files[i]))),
    }
}

/// The image after `src` in its pool (the next of the pack, the next loose file, round
/// again at the end), for when `src` cannot be read.
fn next_image<E: Env>(cx: &Ctx<E>, src: &ImageSource) -> Option<ImageSource> {
    match src {
        ImageSource::Pack { id, index } => {
            let pack = sleeppack::load_pack(cx.env.fs(), &cx.settings.sleep_folder, id)?;
            Some(ImageSource::Pack { id: id.clone(), index: (index + 1) % pack.images.len() })
        }
        ImageSource::Loose(path) => {
            let files = loose_images(cx);
            let i = files.iter().position(|n| quire_fs::join(&cx.settings.sleep_folder, n) == *path)?;
            Some(ImageSource::Loose(quire_fs::join(&cx.settings.sleep_folder, &files[(i + 1) % files.len()])))
        }
    }
}

/// The time as the sleep screens show it.
pub fn clock_text<E: Env>(cx: &Ctx<E>) -> String {
    quire_library::time::fmt_clock(cx.env.now(), cx.settings.clock_24h)
}

/// Padding inside the clock plate over a loose image.
const PLATE_PAD: i32 = 8;
/// The plate's distance from the top of the frame.
const PLATE_TOP: i32 = 24;

/// The clock plate over a loose image: a label-style slot, centred near the top, sized
/// for the widest time the clock setting can produce so it never changes between minutes.
pub fn plate_clock(frame_w: i32, h24: bool) -> Clock {
    let style = ClockStyle::Label;
    let text_style = TextStyle { tracking: style.tracking(), ..TextStyle::INK };
    let suffixes: &[&str] = if h24 { &[""] } else { &[" am", " pm"] };
    let mut widest = 0;
    for d in 0..10u32 {
        for suffix in suffixes {
            let t = alloc::format!("{d}{d}:{d}{d}{suffix}");
            widest = widest.max(measure_text(style.font(), &t, text_style));
        }
    }
    let (asc, desc) = sleeppack::digit_extent(style.font());
    let w = widest + 2 * PLATE_PAD;
    let h = asc + desc + 2 * PLATE_PAD;
    Clock { slot: ClockSlot { x: (frame_w - w) / 2, y: PLATE_TOP, w, h, style, on: Surface::Paper }, plate: true }
}

/// Paint the time into its slot (and the plate's rule around it).
pub fn paint_clock(f: &mut Frame, clock: &Clock, time: &str) {
    sleeppack::draw_clock(f, &clock.slot, time);
    if clock.plate {
        f.stroke_rect(clock.slot.rect(), HAIR, Ink::Black);
    }
}

/// Draw one image source into the frame with its clock when `with_clock`. `None` when it
/// cannot be read; otherwise the clock drawn, if any.
fn draw_image<E: Env>(cx: &Ctx<E>, f: &mut Frame, src: &ImageSource, with_clock: bool, time: &str) -> Option<Option<Clock>> {
    let fs = cx.env.fs();
    let (w, h) = (f.width() as i32, f.height() as i32);
    let mut clock = None;
    match src {
        ImageSource::Loose(path) => {
            f.clear(Ink::White);
            quire_library::cache::load_pbm_into(fs, path, f, centred(w, h), BlitMode::Or)?;
        }
        ImageSource::Pack { id, index } => {
            let pack = sleeppack::load_pack(fs, &cx.settings.sleep_folder, id)?;
            if !sleeppack::draw_image_into(fs, &pack, *index, f) {
                return None;
            }
            clock = pack.images.get(*index).and_then(|i| i.clock).map(|slot| Clock { slot, plate: false });
        }
    }
    if !with_clock {
        return Some(None);
    }
    let clock = clock.unwrap_or_else(|| plate_clock(w, cx.settings.clock_24h));
    paint_clock(f, &clock, time);
    Some(Some(clock))
}

/// The Images variant: the image `keep` (what an earlier draw showed) or the rotation's
/// choice, falling through its pool when a file cannot be read, else the empty state.
fn draw_images<E: Env>(cx: &mut Ctx<E>, f: &mut Frame, keep: Option<&ImageSource>) -> Drawn {
    let time = clock_text(cx);
    let with_clock = cx.settings.sleep_clock;
    let mut source = keep.cloned().or_else(|| choose_image(cx));
    let first = source.clone();
    let mut attempts = 0;
    while let Some(src) = source.take() {
        if let Some(clock) = draw_image(cx, f, &src, with_clock, &time) {
            return Drawn::Image { source: src, clock };
        }
        attempts += 1;
        if attempts < 8 {
            source = next_image(cx, &src).filter(|n| Some(n) != first.as_ref());
        }
    }
    f.clear(Ink::White);
    widgets::empty_state(
        f,
        f.height() as i32 / 2 - 60,
        "No sleep images yet",
        "Download a pack or drop photos from the Drop page, or copy them to the /sleep folder on the card.",
    );
    Drawn::Still
}

/// Where a full-page image of `bw × bh` sits, centred on a `w × h` frame.
fn centred(w: i32, h: i32) -> impl FnOnce(u32, u32) -> (i32, i32) {
    move |bw, bh| ((w - bw as i32) / 2, (h - bh as i32) / 2)
}

/// Draw a sleep variant into the frame at full size.
pub fn draw_variant<E: Env>(cx: &mut Ctx<E>, f: &mut Frame, v: SleepVariant, locked: bool) {
    let _ = draw_variant_keeping(cx, f, v, locked, None);
}

/// Draw a sleep variant, showing `keep` again for the Images variant; returns what the
/// frame now holds.
fn draw_variant_keeping<E: Env>(cx: &mut Ctx<E>, f: &mut Frame, v: SleepVariant, locked: bool, keep: Option<&ImageSource>) -> Drawn {
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
            // The cover streams from the card straight into the frame: no decoded copy.
            let fs = cx.env.fs();
            let placed = cx.reader.as_ref().and_then(|r| quire_library::cache::load_cover_into(fs, r.id, f, centred(w, h), BlitMode::Or));
            if placed.is_none() {
                widgets::typographic_cover(f, Rect::new(0, 0, w as u32, h as u32), &title, &author);
            }
            if cx.settings.sleep_band && cx.reader.is_some() {
                let band_h = 112;
                let band = Rect::new(0, h - band_h, w as u32, band_h as u32);
                f.fill_rect(band, Ink::White);
                f.fill_rect(Rect::new(0, band.y, w as u32, RULE), Ink::Black);
                let ft = quire_fonts::ui::list_title();
                let fb = quire_fonts::ui::body();
                draw_text(f, ft, 32, band.y + 16 + ft.ascent(), &ellipsis(ft, &title, w - 120), TextStyle::INK);
                draw_text(
                    f,
                    fb,
                    32,
                    band.y + 16 + ft.ascent() + ft.below() + 6 + fb.ascent(),
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
                h / 2 + 80 + ft.ascent() + ft.below() + 8 + fb.ascent(),
                &ellipsis(fb, &author, w - 64),
                TextStyle::INK,
            );
            if locked {
                icons::draw(f, Icon::Lock, w / 2 - 12, h - 80, Ink::Black);
            }
            // Without a book the poster is a clock; with one its wording follows the day.
            return if cx.reader.is_none() { Drawn::Timed } else { Drawn::Daily(today) };
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
            return Drawn::Daily(today);
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
        SleepVariant::Custom => return draw_images(cx, f, keep),
        SleepVariant::Blank => {}
    }
    Drawn::Still
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
            self.drawn = Some(Drawn::Still);
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
            self.drawn = Some(Drawn::Still);
            return Refresh::Gc;
        }
        let v = self.variant.unwrap_or(cx.settings.sleep);
        let locked = cx.locked || cx.settings.lock_when_sleeping;
        // A redraw (a battery change) keeps the image the first draw chose.
        let keep = match &self.drawn {
            Some(Drawn::Image { source, .. }) => Some(source.clone()),
            _ => None,
        };
        self.drawn = Some(draw_variant_keeping(cx, f, v, locked && !self.preview, keep.as_ref()));
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
    fn minute_tick(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        match &self.drawn {
            Some(Drawn::Image { clock: Some(clock), .. }) => {
                // The frame still holds the image: repaint the slot only.
                let clock = *clock;
                paint_clock(f, &clock, &clock_text(cx));
                Refresh::Du
            }
            Some(Drawn::Timed) => {
                <Self as Screen<E>>::draw(self, cx, f);
                Refresh::Du
            }
            Some(Drawn::Daily(day)) if *day != cx.today() => {
                <Self as Screen<E>>::draw(self, cx, f);
                Refresh::Du
            }
            _ => Refresh::None,
        }
    }
    fn minute_tick_needs_fs(&self) -> bool {
        // An image is already in the frame and only its clock slot is repainted; a still
        // screen has nothing to repaint at all. Everything else redraws, and a redraw
        // reads the card (the quote file, the pack, the library).
        !matches!(self.drawn, Some(Drawn::Image { .. }) | Some(Drawn::Still))
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
    /// One 1:4 thumbnail per variant (3.3 KB each), rendered on demand into the picker's
    /// own frame before the picker is drawn; an option change drops only its variant's.
    thumbs: [Option<quire_gfx::Bitmap>; 6],
    /// Focus is in the option rows rather than the grid.
    in_options: bool,
    /// The installed packs, listed from the card on first use.
    packs: Option<Vec<PackInfo>>,
}

/// Height of the picker's option rows: three of them fit under the two rows of
/// thumbnails with two lines of help above the rail.
const PICKER_ROW_H: i32 = 44;
/// Vertical pitch of the thumbnail rows: the 198 px thumbnail, its name, and air.
const THUMB_PITCH: i32 = 198 + 36;

/// Shrink a full frame 1:4 by counting ink in each 4 × 4 cell, a byte (two cells) at a
/// time. Text keeps its strokes with `n >= 5`; halftoned images (the cover, the screened
/// page of Quick resume) take the majority so they read as grey, not solid.
fn thumb_of(full: &Frame, majority: bool) -> quire_gfx::Bitmap {
    let (tw, th) = (full.width() / 4, full.height() / 4);
    let mut bm = quire_gfx::Bitmap::new(tw, th);
    let stride = full.stride();
    let bits = full.bits();
    let need = if majority { 9 } else { 5 };
    let mut counts = alloc::vec![0u8; tw as usize];
    for y in 0..th {
        counts.fill(0);
        for dy in 0..4 {
            let row = &bits[(y * 4 + dy) as usize * stride..];
            for (b, &v) in row.iter().enumerate().take(stride) {
                if v == 0 {
                    continue;
                }
                let tx = b * 2;
                if let Some(c) = counts.get_mut(tx) {
                    *c += (v >> 4).count_ones() as u8;
                }
                if let Some(c) = counts.get_mut(tx + 1) {
                    *c += (v & 0x0F).count_ones() as u8;
                }
            }
        }
        for (x, &n) in counts.iter().enumerate() {
            if n >= need {
                bm.set(x as u32, y, true);
            }
        }
    }
    bm
}

impl Picker {
    /// New, focused on the variant in use.
    pub fn new() -> Self {
        Picker { focus: 0, opt_focus: 0, thumbs: [None, None, None, None, None, None], in_options: false, packs: None }
    }
    /// The installed packs (listed once).
    fn packs<E: Env>(&mut self, cx: &Ctx<E>) -> &[PackInfo] {
        self.packs.get_or_insert_with(|| sleeppack::list_packs(cx.env.fs(), &cx.settings.sleep_folder))
    }
    /// Render the variants whose thumbnail is missing into `f` (the frame the picker is
    /// about to draw into, so no scratch frame is needed) and shrink them.
    fn render_thumbs<E: Env>(&mut self, cx: &mut Ctx<E>, f: &mut Frame) {
        let mut rendered = false;
        for (i, v) in SleepVariant::ALL.iter().enumerate() {
            if self.thumbs[i].is_some() {
                continue;
            }
            draw_variant(cx, f, *v, false);
            let halftone = matches!(v, SleepVariant::Cover | SleepVariant::QuickResume);
            self.thumbs[i] = Some(thumb_of(f, halftone));
            rendered = true;
        }
        if rendered {
            f.clear(Ink::White);
        }
    }
    fn options<E: Env>(&mut self, cx: &Ctx<E>) -> Vec<(&'static str, SettingValue)> {
        let focus = SleepVariant::ALL[self.focus];
        let packs = self.packs(cx);
        let s = &*cx.settings;
        match focus {
            SleepVariant::Cover => alloc::vec![("Title band", SettingValue::Toggle(s.sleep_band))],
            SleepVariant::Poster => {
                alloc::vec![("Show", SettingValue::Choice(String::from(if s.sleep_streak { "Streak" } else { "Finish by" })))]
            }
            SleepVariant::Quote => alloc::vec![(
                "Quotes",
                SettingValue::Choice(String::from(if s.sleep_quotes_card { "quotes.txt on card" } else { "Built in" }))
            )],
            SleepVariant::Custom => {
                let source = match s.sleep_pack.as_deref() {
                    None => String::from("Loose images"),
                    Some(ALL_PACKS) => String::from("All packs"),
                    Some(id) => packs.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_else(|| String::from(id)),
                };
                alloc::vec![
                    ("Source", SettingValue::Choice(source)),
                    (
                        "Rotation",
                        SettingValue::Choice(String::from(match s.sleep_rotation {
                            ImageRotation::Fixed => "Fixed",
                            ImageRotation::EachSleep => "Each sleep",
                            ImageRotation::Daily => "Daily",
                        }))
                    ),
                    ("Clock", SettingValue::Toggle(s.sleep_clock)),
                ]
            }
            SleepVariant::QuickResume => alloc::vec![("Moon glyph", SettingValue::Toggle(s.sleep_moon))],
            SleepVariant::Blank => Vec::new(),
        }
    }
    fn change<E: Env>(&mut self, cx: &mut Ctx<E>) {
        let ids: Vec<String> = self.packs(cx).iter().map(|p| p.id.clone()).collect();
        let s = &mut *cx.settings;
        match (SleepVariant::ALL[self.focus], self.opt_focus) {
            (SleepVariant::Cover, _) => s.sleep_band = !s.sleep_band,
            (SleepVariant::Poster, _) => s.sleep_streak = !s.sleep_streak,
            (SleepVariant::Quote, _) => s.sleep_quotes_card = !s.sleep_quotes_card,
            (SleepVariant::Custom, 0) => {
                // Loose images → each pack in turn → all packs → loose images.
                s.sleep_pack = match s.sleep_pack.as_deref() {
                    None => ids.first().cloned(),
                    Some(ALL_PACKS) => None,
                    Some(id) => match ids.iter().position(|i| i == id) {
                        Some(i) if i + 1 < ids.len() => Some(ids[i + 1].clone()),
                        Some(_) => Some(String::from(ALL_PACKS)),
                        None => None,
                    },
                };
            }
            (SleepVariant::Custom, 1) => {
                s.sleep_rotation = match s.sleep_rotation {
                    ImageRotation::Fixed => ImageRotation::EachSleep,
                    ImageRotation::EachSleep => ImageRotation::Daily,
                    ImageRotation::Daily => ImageRotation::Fixed,
                }
            }
            (SleepVariant::Custom, 2) => s.sleep_clock = !s.sleep_clock,
            (SleepVariant::QuickResume, _) => s.sleep_moon = !s.sleep_moon,
            _ => {}
        }
        // Only the focused variant's thumbnail changed.
        self.thumbs[self.focus] = None;
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
        self.render_thumbs(cx, f);
        running_head(f, "Sleep screen", None);
        let w = f.width() as i32;
        let (tw, th) = (132i32, 198i32);
        let gap = (w - 2 * widgets::INSET - 3 * tw) / 2;
        let fl = quire_fonts::ui::label();
        for (i, v) in SleepVariant::ALL.iter().enumerate() {
            let Some(bm) = &self.thumbs[i] else { continue };
            let (c, r) = (i % 3, i / 3);
            let x = widgets::INSET + c as i32 * (tw + gap);
            let y = widgets::CONTENT_TOP + r as i32 * THUMB_PITCH;
            let focused = i == self.focus && !self.in_options;
            f.blit(x, y, bm.as_ref(), BlitMode::Or);
            f.stroke_rect(Rect::new(x - 1, y - 1, (tw + 2) as u32, (th + 2) as u32), if focused { 4 } else { 1 }, Ink::Black);
            let name = v.name();
            let base = y + th + 8 + fl.ascent();
            let right = draw_centered(f, fl, x + tw / 2, base, name, TextStyle::INK);
            if cx.settings.sleep == *v {
                icons::draw(f, Icon::Check, right + 6, base - 20, Ink::Black);
            }
        }
        let mut y = widgets::CONTENT_TOP + 2 * THUMB_PITCH + 2;
        let opts = self.options(cx);
        let row_h = PICKER_ROW_H;
        for (i, (t, v)) in opts.iter().enumerate() {
            setting_row(f, y, row_h, t, v, if self.in_options && i == self.opt_focus { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        let hint = if SleepVariant::ALL[self.focus] == SleepVariant::Custom {
            alloc::format!("Download packs from the Drop page or copy them to {}/packs on the card", cx.settings.sleep_folder)
        } else {
            String::from("Options follow the focused thumbnail. Confirm applies it.")
        };
        for (i, l) in wrap(fl, &hint, w - 2 * widgets::INSET).iter().take(2).enumerate() {
            draw_text(f, fl, widgets::INSET, y + 6 + fl.ascent() + i as i32 * line_h(fl), l, TextStyle::INK);
        }
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

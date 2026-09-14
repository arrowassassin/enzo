//! Quire's screen system.
//!
//! Every screen is a static page drawn into a 1-bit [`Frame`] in response to key events.
//! The [`Ui`] owns the screen stack, the open book, the library and the settings; the
//! platform (device or simulator) feeds it [`Event`]s and pushes the frame it returns to
//! the panel with the [`Refresh`] the screen asked for. The key grammar (brief §2) and the
//! signature elements live in `widgets`, `spine` and the screens.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;
#[cfg(any(test, feature = "std"))]
extern crate std;

pub mod dict;
pub mod icons;
pub mod keyboard;
pub mod qr;
pub mod reader;
pub mod screens;
pub mod settings;
pub mod spine;
pub mod text;
pub mod theme;
pub mod widgets;
pub mod zmachine;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::Frame;
use quire_library::{BookId, Library, Stats};

pub use reader::Reader;
pub use settings::Settings;

/// The seven keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// Bottom, outer left.
    Left,
    /// Bottom, inner left.
    Back,
    /// Bottom, inner right.
    Confirm,
    /// Bottom, outer right.
    Right,
    /// Right edge, upper.
    Up,
    /// Right edge, lower.
    Down,
    /// Top.
    Power,
}

/// What happened to a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// Pressed and released before the long-press threshold.
    Press,
    /// Held past 500 ms (fires once).
    Long,
    /// Still held: fires 5 times a second after the long press.
    Repeat,
    /// Released after a Long (or Repeats).
    Release,
}

/// A key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// The key.
    pub key: Key,
    /// What happened.
    pub kind: KeyKind,
}

impl KeyEvent {
    /// A short press.
    pub const fn press(key: Key) -> KeyEvent {
        KeyEvent { key, kind: KeyKind::Press }
    }
    /// A long press.
    pub const fn long(key: Key) -> KeyEvent {
        KeyEvent { key, kind: KeyKind::Long }
    }
    /// True for a short press of `key`.
    pub fn is(&self, key: Key) -> bool {
        self.key == key && self.kind == KeyKind::Press
    }
    /// True for a long press of `key`.
    pub fn is_long(&self, key: Key) -> bool {
        self.key == key && self.kind == KeyKind::Long
    }
}

/// How the panel should refresh after a draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Refresh {
    /// Nothing changed.
    None,
    /// Partial (DU) refresh, ~380 ms, no flash.
    Du,
    /// Full (GC) refresh with a black-white flash.
    Gc,
}

/// Requests a screen makes of the platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SysRequest {
    /// Enter sleep (the sleep screen is already drawn).
    Sleep,
    /// Power off.
    PowerOff,
    /// Restart.
    Restart,
    /// Full refresh of the current frame.
    RefreshFull,
    /// Turn Wi-Fi on (station).
    WifiOn,
    /// Turn Wi-Fi off.
    WifiOff,
    /// Start the hotspot.
    Hotspot,
    /// Join a network.
    WifiJoin {
        /// SSID.
        ssid: String,
        /// Password.
        password: String,
    },
    /// Forget a saved network.
    WifiForget(String),
    /// Scan for networks.
    WifiScan,
    /// Lock or unlock the keys.
    LockKeys(bool),
    /// Save a screenshot of the current frame.
    Screenshot,
    /// Set the clock (local seconds).
    SetTime(u32),
    /// Start an OTA update from a URL or the SD path.
    Ota(String),
    /// Begin ingest of pending books now.
    IngestNow,
    /// Rescan the card.
    Rescan,
    /// Set the panel orientation.
    Orientation(quire_gfx::Rotation),
    /// Start a timer for `ms` milliseconds (delivered as `Event::Timer`).
    Timer(u32),
    /// The reader wants a night-jobs hour set.
    NightJobs(Option<u8>),
    /// Start or stop the Calibre wireless server.
    Calibre(bool),
    /// Trigger a sync now.
    SyncNow,
    /// Ask the network layer to fetch something (see `net`).
    Fetch(net::FetchRequest),
    /// Reboot into recovery.
    Recovery,
}

/// What a screen wants after handling an event.
pub enum Action<E: Env> {
    /// Nothing.
    None,
    /// Redraw this screen.
    Redraw,
    /// Push a screen.
    Push(Box<dyn Screen<E>>),
    /// Pop this screen.
    Pop,
    /// Pop this screen and push another.
    Replace(Box<dyn Screen<E>>),
    /// Pop everything down to the reading page (or the empty home).
    ToReader,
    /// Pop to the first screen with this name.
    PopTo(&'static str),
    /// Open a book (replaces the reader) and go to it.
    Open(BookId),
    /// Ask the platform for something, then redraw.
    System(SysRequest),
    /// Pop, then deliver a result to the screen beneath.
    PopWith(Result_),
}

/// A value handed to the screen beneath when a picker or keyboard pops.
#[derive(Clone, Debug, PartialEq)]
pub enum Result_ {
    /// Text entered.
    Text(String),
    /// A choice index.
    Choice(usize),
    /// Cancelled.
    Cancel,
    /// A book chosen.
    Book(BookId),
}

/// Battery state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Battery {
    /// Percent 0–100.
    pub percent: u8,
    /// Whether charging.
    pub charging: bool,
    /// Estimated days left, if known.
    pub days_left: Option<u16>,
    /// Cycle count, if known.
    pub cycles: Option<u16>,
    /// Health percent, if known.
    pub health: Option<u8>,
    /// Voltage in mV.
    pub millivolts: u16,
}

/// Wi-Fi state.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum WifiState {
    /// Radio off.
    #[default]
    Off,
    /// Trying to join.
    Connecting(String),
    /// Joined.
    Connected {
        /// Network name.
        ssid: String,
        /// IP address text.
        ip: String,
        /// mDNS host name (without .local).
        host: String,
        /// Signal 0–4.
        signal: u8,
    },
    /// Running the hotspot.
    Hotspot {
        /// Network name.
        ssid: String,
        /// Password.
        password: String,
        /// IP address text.
        ip: String,
    },
    /// Failed to join.
    Failed(String),
}

/// A visible network from a scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiNetwork {
    /// Name.
    pub ssid: String,
    /// Signal 0–4.
    pub signal: u8,
    /// Whether a password is needed.
    pub secured: bool,
    /// Whether we have it saved.
    pub saved: bool,
}

/// Static facts about the device.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DeviceInfo {
    /// Firmware version.
    pub version: String,
    /// Build date or commit.
    pub build: String,
    /// Panel controller name.
    pub panel: String,
    /// Free heap bytes.
    pub free_heap: u32,
    /// Largest free block.
    pub largest_block: u32,
    /// Flash size bytes.
    pub flash_bytes: u32,
    /// Card total bytes, if a card is present.
    pub card_total: Option<u64>,
    /// Card free bytes.
    pub card_free: Option<u64>,
    /// Serial or MAC text.
    pub serial: String,
}

/// Everything that arrives from the platform.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A key.
    Key(KeyEvent),
    /// A timer requested with `SysRequest::Timer` fired.
    Timer,
    /// Periodic tick (about once a second while awake).
    Tick,
    /// Woke from sleep.
    Wake,
    /// Ingest progress for a book.
    Ingest {
        /// Book.
        id: BookId,
        /// Done units.
        done: u32,
        /// Total units.
        total: u32,
        /// Finished (Ok or the error text).
        finished: Option<Result<(), String>>,
    },
    /// Wi-Fi state changed.
    Wifi(WifiState),
    /// A scan finished.
    WifiScan(Vec<WifiNetwork>),
    /// A network job progressed (see `net`).
    Net(net::NetEvent),
    /// Battery changed.
    Battery(Battery),
    /// Text typed on the phone (Drop page).
    PhoneText(String),
    /// A key sent from the phone.
    PhoneKey(KeyEvent),
    /// New books arrived (a scan is due).
    BooksChanged,
}

pub mod net;

/// The platform behind the UI: clock, battery, radio, system requests.
pub trait Env {
    /// The card filesystem.
    type Fs: Fs;
    /// Card access.
    fn fs(&self) -> &Self::Fs;
    /// Local time, seconds since 1970 (the RTC keeps local time).
    fn now(&self) -> u32;
    /// Milliseconds since boot (for timers and the skim cadence).
    fn millis(&self) -> u32;
    /// Battery.
    fn battery(&self) -> Battery;
    /// Wi-Fi.
    fn wifi(&self) -> WifiState;
    /// Saved networks.
    fn saved_networks(&self) -> Vec<String>;
    /// Device facts.
    fn device(&self) -> DeviceInfo;
    /// Ask the platform for something.
    fn request(&mut self, req: SysRequest);
    /// Pseudo-random 32 bits.
    fn random(&mut self) -> u32;
    /// The network job runner's state (downloads, catalogs, sync). See `net`.
    fn net(&mut self) -> &mut dyn net::NetState;
}

/// Shared state every screen can reach.
pub struct Ctx<'a, E: Env> {
    /// The platform.
    pub env: &'a mut E,
    /// The library index.
    pub lib: &'a mut Library,
    /// Reading statistics.
    pub stats: &'a mut Stats,
    /// Settings.
    pub settings: &'a mut Settings,
    /// The open book, if any.
    pub reader: &'a mut Option<Reader>,
    /// Books being ingested: (id, done, total).
    pub ingesting: &'a Vec<(BookId, u32, u32)>,
    /// Text last typed from the phone, taken by the focused text field.
    pub phone_text: &'a mut Option<String>,
    /// Whether the keys are locked.
    pub locked: bool,
}

impl<E: Env> Ctx<'_, E> {
    /// Local day number today.
    pub fn today(&self) -> u16 {
        quire_library::time::day_of(self.env.now())
    }
    /// Save the library and settings if they changed.
    pub fn persist(&mut self) {
        let _ = self.lib.save(self.env.fs());
        let _ = self.settings.save(self.env.fs());
    }
}

/// A screen.
pub trait Screen<E: Env> {
    /// Stable name (matches the design artboard prefix, e.g. "22-contents").
    fn name(&self) -> &'static str;
    /// Draw into the frame (the frame arrives cleared, or holding the page beneath for overlays).
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh;
    /// Handle a key.
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E>;
    /// Handle a non-key event; default ignores.
    fn event(&mut self, _cx: &mut Ctx<E>, _ev: &Event) -> Action<E> {
        Action::None
    }
    /// A child screen popped with a result.
    fn result(&mut self, _cx: &mut Ctx<E>, _r: Result_) -> Action<E> {
        Action::Redraw
    }
    /// Whether this screen draws over the one beneath it.
    fn overlay(&self) -> bool {
        false
    }
    /// Called when the screen becomes the top again.
    fn resume(&mut self, _cx: &mut Ctx<E>) {}
}

/// The UI: screen stack plus the state the screens share.
pub struct Ui<E: Env> {
    /// Screens, bottom first. The bottom is always the reader (or the empty home).
    screens: Vec<Box<dyn Screen<E>>>,
    /// Library index.
    pub lib: Library,
    /// Stats.
    pub stats: Stats,
    /// Settings.
    pub settings: Settings,
    /// The open book.
    pub reader: Option<Reader>,
    /// Ingest progress.
    pub ingesting: Vec<(BookId, u32, u32)>,
    phone_text: Option<String>,
    /// Whether the keys are locked.
    pub locked: bool,
    frame: Frame,
    /// Set while the platform is asleep (sleep screen shown).
    pub asleep: bool,
    last_saved: u32,
}

impl<E: Env> Ui<E> {
    /// Build the UI: load the library, stats and settings and open the current book.
    pub fn new(env: &mut E) -> Self {
        let fs = env.fs();
        let lib = Library::load(fs);
        let stats = Stats::load(fs);
        let settings = Settings::load(fs);
        let mut ui = Ui {
            screens: Vec::new(),
            lib,
            stats,
            settings,
            reader: None,
            ingesting: Vec::new(),
            phone_text: None,
            locked: false,
            frame: Frame::panel(),
            asleep: false,
            last_saved: 0,
        };
        if !ui.settings.first_run_done {
            ui.screens.push(Box::new(screens::firstrun::FirstRun::new()));
        } else {
            ui.open_current(env);
            ui.screens.push(Box::new(screens::reading::ReadingScreen::new()));
        }
        ui
    }

    /// Open the library's current book into the reader.
    pub fn open_current(&mut self, env: &mut E) {
        if let Some(id) = self.lib.current {
            self.open_book(env, id);
        }
    }

    /// Open a book by id into the reader.
    pub fn open_book(&mut self, env: &mut E, id: BookId) -> bool {
        self.close_book(env);
        match Reader::open(env.fs(), &self.lib, &self.settings, id, env.now()) {
            Ok(r) => {
                self.reader = Some(r);
                self.lib.opened(id, env.now());
                true
            }
            Err(_) => false,
        }
    }

    /// Close the open book, recording the session.
    pub fn close_book(&mut self, env: &mut E) {
        if let Some(mut r) = self.reader.take() {
            r.close(env.fs(), &mut self.lib, &mut self.stats, env.now());
        }
        let _ = self.lib.save(env.fs());
    }

    /// The current frame.
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// Name of the top screen.
    pub fn top_name(&self) -> &'static str {
        self.screens.last().map(|s| s.name()).unwrap_or("")
    }

    /// Names of all screens, bottom first.
    pub fn stack_names(&self) -> Vec<&'static str> {
        self.screens.iter().map(|s| s.name()).collect()
    }

    fn ctx<'a>(&'a mut self, env: &'a mut E) -> (Ctx<'a, E>, &'a mut Vec<Box<dyn Screen<E>>>) {
        let cx = Ctx {
            env,
            lib: &mut self.lib,
            stats: &mut self.stats,
            settings: &mut self.settings,
            reader: &mut self.reader,
            ingesting: &self.ingesting,
            phone_text: &mut self.phone_text,
            locked: self.locked,
        };
        (cx, &mut self.screens)
    }

    /// Handle an event; returns the refresh to perform with [`Ui::frame`].
    pub fn handle(&mut self, env: &mut E, ev: Event) -> Refresh {
        // Bookkeeping events first.
        match &ev {
            Event::Ingest { id, done, total, finished } => {
                self.ingesting.retain(|(i, _, _)| i != id);
                match finished {
                    None => self.ingesting.push((*id, *done, *total)),
                    Some(res) => {
                        if let Some(e) = self.lib.get_mut(*id) {
                            match res {
                                Ok(()) => {
                                    // The platform's ingest driver already updated the entry
                                    // through the shared library; reload to be safe.
                                }
                                Err(msg) => {
                                    e.ingest = quire_library::IngestState::Failed;
                                    e.error = Some(msg.clone());
                                }
                            }
                        }
                    }
                }
            }
            Event::PhoneText(t) => self.phone_text = Some(t.clone()),
            Event::Wake => self.asleep = false,
            _ => {}
        }
        let action = {
            let locked = self.locked;
            let (mut cx, screens) = self.ctx(env);
            let Some(top) = screens.last_mut() else { return Refresh::None };
            match &ev {
                Event::Key(k) if locked => {
                    // Locked: only holding Power unlocks; anything else shows the strip.
                    if k.key == Key::Power && k.kind == KeyKind::Long {
                        cx.env.request(SysRequest::LockKeys(false));
                        Action::System(SysRequest::LockKeys(false))
                    } else {
                        Action::Push(Box::new(screens::locked::LockedStrip::new()))
                    }
                }
                Event::Key(k) => {
                    // Universal grammar: long Back opens Jump from anywhere except Jump itself.
                    if k.is_long(Key::Back) && top.name() != "42-jump" && top.name() != "01-boot" && top.name() != "99-recovery" {
                        Action::Push(Box::new(screens::jump::Jump::new()))
                    } else if k.is_long(Key::Power) && top.name() != "41-power" {
                        Action::Push(Box::new(screens::power::PowerMenu::new()))
                    } else if k.is(Key::Power) && top.name() != "41-power" {
                        Action::Push(Box::new(screens::sleep::SleepScreen::new()))
                    } else {
                        top.key(&mut cx, *k)
                    }
                }
                Event::PhoneKey(k) => top.key(&mut cx, *k),
                other => top.event(&mut cx, other),
            }
        };
        let refresh = self.apply(env, action);
        // A sleep screen on top means the device goes to sleep once this frame is on the
        // panel: persist everything and ask the platform, whichever screen put it there.
        let top = self.top_name();
        if (top == "40-sleep" || top == "40-sleep-charging") && !self.asleep {
            self.asleep = true;
            self.flush(env);
            env.request(SysRequest::Sleep);
        }
        // Periodic persistence of positions and stats.
        let now = env.now();
        if now.saturating_sub(self.last_saved) >= 30 {
            self.last_saved = now;
            if let Some(r) = self.reader.as_mut() {
                r.save_position(&mut self.lib);
            }
            let _ = self.lib.save(env.fs());
            let _ = self.settings.save(env.fs());
        }
        refresh
    }

    /// Apply an action as if the top screen returned it (the platform and tests use it to
    /// open Jump targets directly).
    pub fn apply(&mut self, env: &mut E, action: Action<E>) -> Refresh {
        match action {
            Action::None => Refresh::None,
            Action::Redraw => self.draw(env),
            Action::Push(s) => {
                self.screens.push(s);
                self.draw(env)
            }
            Action::Pop => {
                if self.screens.len() > 1 {
                    self.screens.pop();
                }
                self.resume_top(env);
                self.draw(env)
            }
            Action::Replace(s) => {
                if self.screens.len() > 1 {
                    self.screens.pop();
                }
                self.screens.push(s);
                self.draw(env)
            }
            Action::ToReader => {
                self.screens.truncate(1);
                if self.screens.is_empty() || self.screens[0].name() != "20-reading" {
                    self.screens.clear();
                    self.screens.push(Box::new(screens::reading::ReadingScreen::new()));
                }
                self.resume_top(env);
                self.draw(env)
            }
            Action::PopTo(name) => {
                while self.screens.len() > 1 && self.screens.last().map(|s| s.name()) != Some(name) {
                    self.screens.pop();
                }
                self.resume_top(env);
                self.draw(env)
            }
            Action::Open(id) => {
                self.open_book(env, id);
                self.screens.clear();
                self.screens.push(Box::new(screens::reading::ReadingScreen::new()));
                self.draw(env)
            }
            Action::System(req) => {
                match &req {
                    SysRequest::Sleep => self.asleep = true,
                    SysRequest::LockKeys(l) => self.locked = *l,
                    _ => {}
                }
                env.request(req);
                self.draw(env)
            }
            Action::PopWith(r) => {
                if self.screens.len() > 1 {
                    self.screens.pop();
                }
                let next = {
                    let (mut cx, screens) = self.ctx(env);
                    match screens.last_mut() {
                        Some(top) => top.result(&mut cx, r),
                        None => Action::None,
                    }
                };
                match next {
                    Action::None => self.draw(env),
                    other => self.apply(env, other),
                }
            }
        }
    }

    fn resume_top(&mut self, env: &mut E) {
        let (mut cx, screens) = self.ctx(env);
        if let Some(top) = screens.last_mut() {
            top.resume(&mut cx);
        }
    }

    /// Draw the stack into the frame: the topmost non-overlay screen, then overlays above it.
    pub fn draw(&mut self, env: &mut E) -> Refresh {
        let mut frame = core::mem::replace(&mut self.frame, Frame::new(1, 1));
        let inverted = self.settings.inverted;
        let refresh = {
            let (mut cx, screens) = self.ctx(env);
            let mut start = screens.len().saturating_sub(1);
            while start > 0 && screens[start].overlay() {
                start -= 1;
            }
            frame.clear(quire_gfx::Ink::White);
            let mut refresh = Refresh::Du;
            for s in screens[start..].iter_mut() {
                let r = s.draw(&mut cx, &mut frame);
                refresh = refresh.max(r);
            }
            refresh
        };
        if inverted {
            frame.invert_rect(frame.bounds());
        }
        self.frame = frame;
        refresh
    }

    /// Push a screen from outside (the platform opening Drop on a hotspot, tests).
    pub fn push(&mut self, env: &mut E, s: Box<dyn Screen<E>>) -> Refresh {
        self.apply(env, Action::Push(s))
    }

    /// Persist everything now (before sleep or power off).
    pub fn flush(&mut self, env: &mut E) {
        if let Some(r) = self.reader.as_mut() {
            r.save_position(&mut self.lib);
            r.flush_session(env.fs(), &mut self.lib, &mut self.stats, env.now());
        }
        let _ = self.lib.save(env.fs());
        let _ = self.settings.save(env.fs());
    }
}

/// Root of Quire's files on the card.
pub const ROOT: &str = quire_library::ROOT;

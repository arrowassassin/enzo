//! A host platform for the real UI: a card in a temp directory, a fixed clock, scripted
//! keys, and PNG snapshots. `quire-sim` (the binary) and the snapshot tests use it.

use quire_fs::host::HostFs;
use quire_gfx::Frame;
use quire_library::stats::Session;
use quire_library::{ingest_book, scan, Library, Stats};
use quire_ui::net::{NetState, NoNet};
use quire_ui::screens::jump::{self, Target};
use quire_ui::{Action, Battery, DeviceInfo, Env, Event, Key, KeyEvent, Refresh, Settings, SysRequest, Ui, WifiState};
use std::path::{Path, PathBuf};

/// The simulator's clock: Monday 14 September 2026, 21:47 local.
pub const NOW: u32 = 1_789_422_420;

/// The host platform.
pub struct HostEnv {
    fs: HostFs,
    /// Local time; advance it to simulate reading.
    pub now: u32,
    /// Milliseconds since boot.
    pub millis: u32,
    /// Battery reported to the UI.
    pub battery: Battery,
    /// Wi-Fi state reported to the UI.
    pub wifi: WifiState,
    /// Every system request the UI made, in order.
    pub requests: Vec<SysRequest>,
    net: NoNet,
    seed: u32,
}

impl HostEnv {
    /// A platform over the card at `root`.
    pub fn new(root: &Path) -> Self {
        HostEnv {
            fs: HostFs::new(root),
            now: NOW,
            millis: 1000,
            battery: Battery { percent: 62, charging: false, days_left: Some(19), cycles: Some(41), health: Some(97), millivolts: 3812 },
            wifi: WifiState::Off,
            requests: Vec::new(),
            net: NoNet::default(),
            seed: 0x2545_F491,
        }
    }
}

impl Env for HostEnv {
    type Fs = HostFs;
    fn fs(&self) -> &HostFs {
        &self.fs
    }
    fn now(&self) -> u32 {
        self.now
    }
    fn millis(&self) -> u32 {
        self.millis
    }
    fn battery(&self) -> Battery {
        self.battery
    }
    fn wifi(&self) -> WifiState {
        self.wifi.clone()
    }
    fn saved_networks(&self) -> Vec<String> {
        vec![String::from("HomeNet")]
    }
    fn device(&self) -> DeviceInfo {
        DeviceInfo {
            version: String::from("0.1.0"),
            build: String::from("sim"),
            panel: String::from("simulator"),
            free_heap: 118_000,
            largest_block: 64_000,
            flash_bytes: 16 * 1024 * 1024,
            card_total: Some(31_914_983_424),
            card_free: Some(29_100_000_000),
            serial: String::from("sim-0001"),
        }
    }
    fn request(&mut self, req: SysRequest) {
        self.requests.push(req);
    }
    fn random(&mut self) -> u32 {
        // xorshift32: deterministic so snapshots are stable.
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x
    }
    fn net(&mut self) -> &mut dyn NetState {
        &mut self.net
    }
}

/// Where the shared fixtures live.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/quire-doc/fixtures")
}

/// Build a card with real books ingested, thirty days of reading history, first run
/// done and Moby-Dick open at chapter 1. Returns the card root.
pub fn fixture_card(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("quire-sim-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Books/classics")).unwrap();
    let fx = fixtures();
    std::fs::copy(fx.join("moby-dick.epub"), dir.join("Books/classics/moby-dick.epub")).unwrap();
    std::fs::copy(fx.join("tracemonkey.pdf"), dir.join("Books/tracemonkey.pdf")).unwrap();
    std::fs::copy(fx.join("middlemarch.txt"), dir.join("Books/middlemarch.txt")).unwrap();
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::write(dir.join("notes/reading-list.md"), "# Reading list\n\n- *Bleak House*\n- *The Odyssey*\n").unwrap();
    std::fs::create_dir_all(dir.join("flashcards")).unwrap();
    std::fs::write(dir.join("flashcards/french.txt"), "bonjour\thello\nmerci\tthank you\nlivre\tbook\n").unwrap();

    let fs = HostFs::new(&dir);
    let mut lib = Library::load(&fs);
    scan(&fs, &mut lib, NOW - 86_400 * 40).expect("scan");
    for id in lib.pending() {
        ingest_book(&fs, &mut lib, id, &mut |_, _| {}).unwrap_or_else(|e| panic!("ingest {id}: {e}"));
    }
    let moby = lib.by_path("/Books/classics/moby-dick.epub").expect("moby").id;
    let middlemarch = lib.by_path("/Books/middlemarch.txt").expect("middlemarch").id;
    let pdf = lib.by_path("/Books/tracemonkey.pdf").expect("pdf").id;

    // Thirty days of evenings: Middlemarch finished, then Moby-Dick under way.
    let mut stats = Stats::load(&fs);
    for d in 0..30u32 {
        let day_start = NOW - (30 - d) * 86_400;
        let evening = day_start - (day_start % 86_400) + 20 * 3600 + (d * 7 % 40) * 60;
        let (book, secs) = if d < 18 { (middlemarch, 1500 + (d * 137) % 1900) } else { (moby, 1200 + (d * 211) % 2400) };
        let chars = secs * 22;
        let s = Session {
            book,
            start: evening,
            end: evening + secs + 40,
            active: secs,
            pages: (chars / 1100) as u16,
            chars,
            pace_chars: chars,
            pace_secs: secs,
            flags: 0,
        };
        stats.record(&fs, &mut lib, &s).expect("record");
        if d == 17 {
            lib.set_finished(middlemarch, true, quire_library::time::day_of(evening + secs));
        }
    }
    stats.save(&fs).expect("stats");
    // Positions: Moby-Dick a little way in, the PDF opened once.
    lib.opened(pdf, NOW - 86_400 * 3);
    lib.opened(moby, NOW - 3600);
    lib.save(&fs).expect("lib");
    let settings = Settings { first_run_done: true, ..Default::default() };
    settings.save(&fs).expect("settings");
    dir
}

/// The UI plus its platform, driven by keys.
pub struct Sim {
    /// The platform.
    pub env: HostEnv,
    /// The UI.
    pub ui: Ui<HostEnv>,
    /// Refresh the last event asked for.
    pub last_refresh: Refresh,
    /// Milliseconds the last event took to handle and draw (host time).
    pub last_ms: u128,
}

impl Sim {
    /// Boot on the card at `root`.
    pub fn boot(root: &Path) -> Self {
        let mut env = HostEnv::new(root);
        let ui = Ui::new(&mut env);
        let mut sim = Sim { env, ui, last_refresh: Refresh::None, last_ms: 0 };
        sim.ui.draw(&mut sim.env);
        sim
    }

    /// Feed an event.
    pub fn event(&mut self, ev: Event) -> Refresh {
        let t = std::time::Instant::now();
        self.env.millis = self.env.millis.wrapping_add(50);
        let r = self.ui.handle(&mut self.env, ev);
        self.last_ms = t.elapsed().as_millis();
        self.last_refresh = r;
        r
    }

    /// A short press.
    pub fn press(&mut self, key: Key) -> Refresh {
        self.event(Event::Key(KeyEvent::press(key)))
    }

    /// A long press (fires Long, then Release).
    pub fn long(&mut self, key: Key) -> Refresh {
        let r = self.event(Event::Key(KeyEvent::long(key)));
        self.event(Event::Key(KeyEvent { key, kind: quire_ui::KeyKind::Release }));
        r
    }

    /// Hold a key: Long, then `repeats` repeats, then Release.
    pub fn hold(&mut self, key: Key, repeats: u32) -> Refresh {
        self.event(Event::Key(KeyEvent::long(key)));
        for _ in 0..repeats {
            self.env.millis = self.env.millis.wrapping_add(200);
            self.event(Event::Key(KeyEvent { key, kind: quire_ui::KeyKind::Repeat }));
        }
        self.event(Event::Key(KeyEvent { key, kind: quire_ui::KeyKind::Release }))
    }

    /// Type text as if from the phone.
    pub fn phone(&mut self, text: &str) -> Refresh {
        self.event(Event::PhoneText(String::from(text)))
    }

    /// A run of short presses.
    pub fn presses(&mut self, keys: &[Key]) {
        for k in keys {
            self.press(*k);
        }
    }

    /// Open a Jump target directly.
    pub fn open(&mut self, name: &'static str) -> Refresh {
        let mut phone_text = None;
        let action = {
            let mut cx = quire_ui::Ctx {
                env: &mut self.env,
                lib: &mut self.ui.lib,
                stats: &mut self.ui.stats,
                settings: &mut self.ui.settings,
                reader: &mut self.ui.reader,
                ingesting: &self.ui.ingesting,
                phone_text: &mut phone_text,
                locked: false,
            };
            jump::open_target(&mut cx, &Target::Screen(name))
        };
        let action = match action {
            Action::Replace(s) => Action::Push(s),
            other => other,
        };
        self.ui.apply(&mut self.env, action)
    }

    /// Finish building the open book's page index (the device does this between frames).
    pub fn index_all(&mut self) {
        let fs = &self.env.fs;
        if let Some(r) = self.ui.reader.as_mut() {
            let mut guard = 0;
            while !r.index_complete() && guard < 5000 {
                r.index_step(fs, &mut self.ui.lib);
                guard += 1;
            }
        }
        self.ui.draw(&mut self.env);
    }

    /// Pop everything and show the reading page.
    pub fn reset(&mut self) {
        self.ui.apply(&mut self.env, Action::ToReader);
    }

    /// Go to the first TOC entry whose title contains `needle`.
    pub fn goto_chapter(&mut self, needle: &str) -> bool {
        let fs = &self.env.fs;
        let Some(r) = self.ui.reader.as_mut() else { return false };
        let Some(i) = r.book.toc.iter().position(|t| t.title.contains(needle)) else { return false };
        let Some(loc) = r.book.toc_target(i) else { return false };
        r.goto(fs, loc);
        self.ui.draw(&mut self.env);
        true
    }

    /// Name of the top screen.
    pub fn top(&self) -> &'static str {
        self.ui.top_name()
    }

    /// The frame.
    pub fn frame(&self) -> &Frame {
        self.ui.frame()
    }

    /// A stable 64-bit hash of the frame.
    pub fn hash(&self) -> u64 {
        frame_hash(self.frame())
    }
}

/// FNV-1a over the frame's pixels.
pub fn frame_hash(frame: &Frame) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for y in 0..frame.height() as i32 {
        let mut acc = 0u8;
        for x in 0..frame.width() as i32 {
            acc = (acc << 1) | frame.get(x, y) as u8;
            if x % 8 == 7 {
                h ^= acc as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
                acc = 0;
            }
        }
        h ^= acc as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Write a frame as an 8-bit greyscale PNG.
pub fn frame_to_png(frame: &Frame, path: &Path) -> anyhow::Result<()> {
    let (w, h) = (frame.width(), frame.height());
    let mut img = image::GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = if frame.get(x as i32, y as i32) { 0 } else { 255 };
            img.put_pixel(x, y, image::Luma([v]));
        }
    }
    img.save(path)?;
    Ok(())
}

/// One captured screen.
pub struct Shot {
    /// Snapshot name (artboard-style).
    pub name: String,
    /// The frame.
    pub frame: Frame,
    /// Screen stack at capture.
    pub stack: Vec<&'static str>,
    /// Refresh requested by the last event.
    pub refresh: Refresh,
    /// Host milliseconds for the last event.
    pub ms: u128,
}

/// Walk the whole device: every Jump target, every reading overlay, the sub-screens
/// behind each list, and capture a frame for each. Returns the shots in order.
pub fn tour(sim: &mut Sim) -> Vec<Shot> {
    let mut shots = Vec::new();
    let mut shot = |sim: &mut Sim, name: &str| {
        shots.push(Shot {
            name: name.to_string(),
            frame: sim.frame().clone(),
            stack: sim.ui.stack_names(),
            refresh: sim.last_refresh,
            ms: sim.last_ms,
        });
    };

    // Reading page and its overlays; each sequence starts from the page.
    sim.index_all();
    sim.goto_chapter("Loomings");
    shot(sim, "20-reading");
    sim.press(Key::Right);
    shot(sim, "20-reading-next");
    sim.press(Key::Left);
    sim.event(Event::Key(KeyEvent::long(Key::Right)));
    for _ in 0..6 {
        sim.env.millis = sim.env.millis.wrapping_add(200);
        sim.event(Event::Key(KeyEvent { key: Key::Right, kind: quire_ui::KeyKind::Repeat }));
    }
    shot(sim, "21-skim");
    sim.event(Event::Key(KeyEvent { key: Key::Right, kind: quire_ui::KeyKind::Release }));
    shot(sim, "21-skim-released");
    sim.reset();
    sim.press(Key::Back);
    shot(sim, "10-home");
    sim.reset();
    sim.press(Key::Confirm);
    shot(sim, "24-compass");
    sim.press(Key::Down);
    shot(sim, "24-compass-more");
    sim.reset();
    sim.long(Key::Confirm);
    shot(sim, "26-cursor");
    sim.presses(&[Key::Right, Key::Right, Key::Confirm]);
    shot(sim, "27-dictionary");
    sim.reset();
    sim.press(Key::Confirm);
    sim.press(Key::Left);
    shot(sim, "22-contents");
    sim.press(Key::Down);
    sim.press(Key::Down);
    shot(sim, "22-contents-down");
    sim.reset();
    sim.press(Key::Confirm);
    sim.press(Key::Right);
    shot(sim, "23-goto");
    sim.press(Key::Right);
    shot(sim, "23-goto-right");
    sim.reset();
    sim.press(Key::Confirm);
    sim.press(Key::Up);
    shot(sim, "25-type");
    sim.press(Key::Right);
    shot(sim, "25-type-bigger");
    sim.press(Key::Left);
    sim.reset();
    sim.long(Key::Back);
    shot(sim, "42-jump");
    sim.phone("we");
    shot(sim, "42-jump-filtered");
    sim.reset();

    // Every Jump target, with its first sub-screens.
    let targets: Vec<(&'static str, Vec<Key>)> = vec![
        ("11-library", vec![Key::Right, Key::Right]),
        ("35-bookshop", vec![Key::Confirm, Key::Confirm]),
        ("30-drop", vec![]),
        ("60a-overview", vec![Key::Right, Key::Right, Key::Right, Key::Right]),
        ("61-yearinreview", vec![Key::Right, Key::Right]),
        (
            "50-settings",
            vec![Key::Confirm, Key::Back, Key::Down, Key::Confirm, Key::Back, Key::Down, Key::Confirm, Key::Back, Key::Down, Key::Confirm],
        ),
        ("50-battery", vec![]),
        ("50-about", vec![Key::Confirm]),
        ("28-highlights", vec![]),
        ("25-layout", vec![]),
        ("44-picker", vec![Key::Down, Key::Down]),
        ("31-wifi", vec![]),
        ("13-folders", vec![Key::Confirm]),
        ("32-opds", vec![]),
        ("33-calibre", vec![]),
        ("34-sync", vec![]),
        ("39-downloads", vec![]),
        ("90-developer", vec![]),
        ("70-apps", vec![]),
        ("80-games", vec![]),
        ("73-clock", vec![Key::Right, Key::Right]),
        ("71-flashcards", vec![Key::Confirm]),
        ("72-news", vec![]),
        ("74-weather", vec![]),
        ("77-wikipedia", vec![]),
        ("78-calculator", vec![Key::Right, Key::Confirm, Key::Right, Key::Right]),
        ("79-notes", vec![Key::Confirm]),
        ("75-images", vec![]),
        ("76-fiction", vec![]),
        ("80-sudoku", vec![Key::Confirm]),
        ("80-2048", vec![Key::Left, Key::Up]),
        ("80-minesweeper", vec![Key::Confirm]),
        ("80-chess", vec![Key::Confirm, Key::Confirm]),
        ("80-wordle", vec![]),
    ];
    for (name, keys) in targets {
        sim.reset();
        sim.open(name);
        shot(sim, name);
        for (i, k) in keys.iter().enumerate() {
            sim.press(*k);
            let top = sim.top();
            shot(sim, &format!("{name}--{}-{}", i + 1, top));
        }
    }

    // Power and sleep.
    sim.reset();
    sim.long(Key::Power);
    shot(sim, "41-power");
    sim.press(Key::Back);
    sim.press(Key::Power);
    shot(sim, "40-sleep");
    sim.event(Event::Wake);
    sim.reset();
    shot(sim, "20-reading-after-wake");
    shots
}

//! A host platform for the real UI: a card in a temp directory, a fixed clock, scripted
//! keys, and PNG snapshots. `quire-sim` (the binary) and the snapshot tests use it.

use quire_fs::host::HostFs;
use quire_gfx::Frame;
use quire_library::stats::Session;
use quire_library::{ingest_book, scan, Library, Stats};
use quire_ui::net::{NetEvent, NetState, NoNet, OtaInfo};
use quire_ui::screens::jump::{self, Target};
use quire_ui::settings::SleepVariant;
use quire_ui::{Action, Battery, Ctx, DeviceInfo, Env, Event, Key, KeyEvent, Refresh, Settings, SysRequest, Ui, WifiNetwork, WifiState};
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
    /// Free bytes on the card as reported to the UI (`None` = no card).
    pub card_free: Option<u64>,
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
            card_free: Some(29_100_000_000),
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
            card_free: self.card_free,
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

/// The repository's sleep-image packs (`sleep-packs/`, next to `firmware/`).
pub fn sleep_packs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../sleep-packs")
}

/// The pack the fixture card installs under `/sleep/packs/`.
pub const FIXTURE_PACK: &str = "mountains";

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
    // A story for interactive fiction, when the crate ships one.
    let advent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/quire-ui/data/advent.z5");
    if advent.exists() {
        std::fs::create_dir_all(dir.join("stories")).unwrap();
        std::fs::copy(&advent, dir.join("stories/advent.z5")).unwrap();
    }
    // One real sleep-image pack: the manifest, a plain image and a compressed one (the
    // rest of its images are left out, so a missing file is exercised too).
    let pack_src = sleep_packs().join(FIXTURE_PACK);
    let pack_dst = dir.join("sleep/packs").join(FIXTURE_PACK);
    std::fs::create_dir_all(&pack_dst).unwrap();
    for f in ["pack.json", "01.pbm", "02.pbm.z"] {
        std::fs::copy(pack_src.join(f), pack_dst.join(f))
            .unwrap_or_else(|e| panic!("sleep pack fixture {}: {e}", pack_src.join(f).display()));
    }

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

    /// Run `f` with a screen context over the UI's state (what a screen sees).
    pub fn with_ctx<R>(&mut self, f: impl FnOnce(&mut Ctx<HostEnv>) -> R) -> R {
        let mut phone_text = None;
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
        f(&mut cx)
    }

    /// The action Jump takes for a target (a `Replace` of the Jump screen).
    fn target_action(&mut self, name: &'static str) -> Action<HostEnv> {
        self.with_ctx(|cx| jump::open_target(cx, &Target::Screen(name)))
    }

    /// Push a screen over the current one (the platform does this for boot, recovery,
    /// OTA and the working card).
    pub fn push(&mut self, s: Box<dyn quire_ui::Screen<HostEnv>>) -> Refresh {
        self.ui.push(&mut self.env, s)
    }

    /// Pop the top screen without a key (what the platform does when a job finishes).
    pub fn pop(&mut self) -> Refresh {
        self.ui.apply(&mut self.env, Action::Pop)
    }

    /// Sleep with a given sleep-screen variant (a Power press), then wake again.
    fn sleep_as(&mut self, v: SleepVariant, visit: &mut dyn FnMut(&mut Sim, &str, bool), name: &str) {
        let was = self.ui.settings.sleep;
        self.ui.settings.sleep = v;
        self.press(Key::Power);
        visit(self, name, false);
        self.event(Event::Wake);
        self.ui.settings.sleep = was;
        self.reset();
    }

    /// Open a Jump target directly (pushed over the current screen, without Jump).
    pub fn open(&mut self, name: &'static str) -> Refresh {
        let action = match self.target_action(name) {
            Action::Replace(s) => Action::Push(s),
            other => other,
        };
        self.ui.apply(&mut self.env, action)
    }

    /// Open a Jump target the way a reader does: long Back opens Jump, then the target
    /// takes Jump's place on the stack.
    pub fn jump_to(&mut self, name: &'static str) -> Refresh {
        self.long(Key::Back);
        let action = self.target_action(name);
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

/// The Jump targets the tour opens, each with the keys pressed after it (every sub-screen
/// the tour reaches).
pub fn tour_targets() -> Vec<(&'static str, Vec<Key>)> {
    vec![
        ("11-library", vec![Key::Right, Key::Right]),
        ("35-bookshop", vec![Key::Confirm, Key::Confirm]),
        ("30-drop", vec![]),
        ("60a-overview", vec![Key::Right, Key::Right, Key::Right, Key::Right]),
        ("61-yearinreview", vec![Key::Left, Key::Left]),
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
        ("73-clock", vec![Key::Right, Key::Right, Key::Down, Key::Confirm]),
        ("71-flashcards", vec![Key::Confirm]),
        ("72-news", vec![]),
        ("74-weather", vec![]),
        ("77-wikipedia", vec![]),
        ("78-calculator", vec![Key::Right, Key::Confirm, Key::Right, Key::Right]),
        ("79-notes", vec![Key::Right]),
        ("75-images", vec![]),
        ("76-fiction", vec![]),
        ("80-sudoku", vec![Key::Confirm]),
        ("80-2048", vec![Key::Left, Key::Up]),
        ("80-minesweeper", vec![Key::Confirm]),
        ("80-chess", vec![Key::Confirm, Key::Confirm]),
        ("80-wordle", vec![]),
    ]
}

/// Walk the whole device: every Jump target, every reading overlay, the sub-screens
/// behind each list. `visit` is called at every point a snapshot is taken with the shot
/// name; `leaf` is true when the tour resets to the page straight afterwards, so a visitor
/// may navigate away there without disturbing the walk.
pub fn tour_with(sim: &mut Sim, visit: &mut dyn FnMut(&mut Sim, &str, bool)) {
    // Reading page and its overlays; each sequence starts from the page.
    sim.index_all();
    sim.goto_chapter("Loomings");
    visit(sim, "20-reading", false);
    sim.press(Key::Right);
    visit(sim, "20-reading-next", false);
    sim.press(Key::Left);
    sim.event(Event::Key(KeyEvent::long(Key::Right)));
    for _ in 0..6 {
        sim.env.millis = sim.env.millis.wrapping_add(200);
        sim.event(Event::Key(KeyEvent { key: Key::Right, kind: quire_ui::KeyKind::Repeat }));
    }
    visit(sim, "21-skim", false);
    sim.event(Event::Key(KeyEvent { key: Key::Right, kind: quire_ui::KeyKind::Release }));
    visit(sim, "21-skim-released", true);
    sim.reset();
    sim.press(Key::Back);
    visit(sim, "10-home", true);
    sim.reset();
    sim.press(Key::Confirm);
    visit(sim, "24-compass", false);
    sim.press(Key::Down);
    visit(sim, "24-compass-more", true);
    sim.reset();
    sim.long(Key::Confirm);
    visit(sim, "26-cursor", false);
    sim.presses(&[Key::Right, Key::Right, Key::Confirm]);
    visit(sim, "27-dictionary", true);
    sim.reset();
    sim.press(Key::Confirm);
    sim.press(Key::Left);
    visit(sim, "22-contents", false);
    sim.press(Key::Down);
    sim.press(Key::Down);
    visit(sim, "22-contents-down", true);
    sim.reset();
    sim.press(Key::Confirm);
    sim.press(Key::Right);
    visit(sim, "23-goto", false);
    sim.press(Key::Right);
    visit(sim, "23-goto-right", true);
    sim.reset();
    sim.press(Key::Confirm);
    sim.press(Key::Up);
    visit(sim, "25-type", false);
    // Down to the Size row, then Right: the specimen re-renders one size up.
    sim.presses(&[Key::Down, Key::Down, Key::Down, Key::Down, Key::Right]);
    visit(sim, "25-type-bigger", false);
    sim.press(Key::Left);
    sim.reset();
    sim.long(Key::Back);
    visit(sim, "42-jump", false);
    sim.phone("we");
    visit(sim, "42-jump-filtered", true);
    sim.reset();

    // Every Jump target, with its first sub-screens.
    for (name, keys) in tour_targets() {
        sim.reset();
        sim.open(name);
        visit(sim, name, keys.is_empty());
        for (i, k) in keys.iter().enumerate() {
            sim.press(*k);
            let top = sim.top();
            visit(sim, &format!("{name}--{}-{}", i + 1, top), i + 1 == keys.len());
        }
    }

    tour_more(sim, visit);

    // Power and sleep.
    sim.reset();
    sim.long(Key::Power);
    visit(sim, "41-power", false);
    sim.press(Key::Back);
    sim.press(Key::Power);
    visit(sim, "40-sleep", false);
    sim.event(Event::Wake);
    sim.reset();
    visit(sim, "20-reading-after-wake", true);
    for (v, name) in [
        (SleepVariant::Poster, "40-sleep-poster"),
        (SleepVariant::Quote, "40-sleep-quote"),
        (SleepVariant::QuickResume, "40-sleep-quickresume"),
        (SleepVariant::Custom, "40-sleep-custom"),
        (SleepVariant::Blank, "40-sleep-blank"),
    ] {
        sim.sleep_as(v, visit, name);
    }
    sim.env.battery.charging = true;
    sim.sleep_as(SleepVariant::Cover, visit, "40-sleep-charging");
    sim.env.battery.charging = false;
    sim.reset();

    // A pack image with its live clock, then the minute tick while asleep that repaints
    // only the clock slot.
    let saved = sim.ui.settings.clone();
    sim.ui.settings.sleep = SleepVariant::Custom;
    sim.ui.settings.sleep_pack = Some(String::from(FIXTURE_PACK));
    sim.ui.settings.sleep_rotation = quire_ui::settings::ImageRotation::Fixed;
    sim.ui.settings.sleep_image = Some(String::from("02.pbm"));
    sim.press(Key::Power);
    visit(sim, "40-sleep-pack", false);
    sim.env.now += 60;
    sim.event(Event::Tick);
    visit(sim, "40-sleep-pack-tick", false);
    sim.env.now = NOW;
    sim.event(Event::Wake);
    sim.ui.settings = saved.clone();
    sim.reset();

    // The picker with the Images variant focused, its source set to the pack.
    sim.ui.settings.sleep_pack = Some(String::from(FIXTURE_PACK));
    sim.ui.settings.sleep_rotation = quire_ui::settings::ImageRotation::Fixed;
    sim.ui.settings.sleep_image = Some(String::from("02.pbm"));
    sim.open("44-picker");
    sim.press(Key::Down);
    visit(sim, "44-picker-pack", true);
    sim.ui.settings = saved;
    sim.reset();
}

/// The stops where the universal key grammar does not apply (boot and recovery own the
/// keys; nothing is open on the empty home; the first-run wizard runs on another card),
/// so they are captured by [`tour`] but not walked by [`tour_with`].
pub fn tour_special(sim: &mut Sim, visit: &mut dyn FnMut(&mut Sim, &str, bool)) {
    sim.reset();
    sim.push(Box::new(quire_ui::screens::boot::Boot { status: String::from("Indexing 2 of 3 books"), permille: 600 }));
    visit(sim, "01-boot", false);
    sim.pop();
    sim.push(Box::new(quire_ui::screens::settings::Recovery::new("The last update did not verify.")));
    visit(sim, "99-recovery", false);
    sim.pop();
    // The pause card over a game (Right on it quits, so a round trip through Jump from
    // here is not a plain Back).
    sim.open("80-2048");
    sim.press(Key::Back);
    visit(sim, "80-paused", false);
    sim.press(Key::Right);
    sim.reset();
    tour_home_empty(sim, visit);
    tour_first_run(visit);
}

/// The stops beyond the Jump targets: the states a reader gets into by doing things
/// (the library's list view and book compass, the end of a book, Drop when connected,
/// the Wi-Fi keyboard, the stats pages, a study session, news, alarms, a story, the
/// paused and finished games) and the cards the platform pushes (boot, OTA, recovery,
/// the working card, a dialog).
fn tour_more(sim: &mut Sim, visit: &mut dyn FnMut(&mut Sim, &str, bool)) {
    // Library: list view, the book compass, book info, the delete dialog.
    sim.reset();
    sim.ui.settings.library_grid = false;
    sim.open("11-library");
    visit(sim, "11-library-list", true);
    sim.reset();
    sim.ui.settings.library_grid = true;
    sim.open("11-library");
    sim.long(Key::Confirm);
    visit(sim, "11-library-compass", false);
    sim.press(Key::Up);
    visit(sim, "11-dialog-delete", false);
    sim.press(Key::Back);
    sim.press(Key::Down);
    visit(sim, "12-bookinfo", false);
    sim.press(Key::Right);
    visit(sim, "12-bookinfo-2", true);
    sim.reset();

    // Browse, behind the Bookshop's Left.
    sim.open("35-bookshop");
    sim.press(Key::Left);
    visit(sim, "38-browse", true);
    sim.reset();

    // Stats pages behind the overview.
    sim.open("60a-overview");
    sim.press(Key::Up);
    visit(sim, "60b-rhythm", false);
    sim.press(Key::Back);
    sim.press(Key::Down);
    visit(sim, "60c-calendar", false);
    sim.press(Key::Back);
    sim.press(Key::Right);
    sim.press(Key::Confirm);
    visit(sim, "60d-books", false);
    sim.press(Key::Right);
    visit(sim, "60e-goals", true);
    sim.reset();

    // Drop when connected, on the hotspot, and with a full card.
    sim.env.wifi =
        WifiState::Connected { ssid: String::from("HomeNet"), ip: String::from("192.168.1.23"), host: String::from("quire"), signal: 3 };
    sim.open("30-drop");
    visit(sim, "30-drop-connected", true);
    sim.reset();
    sim.env.wifi =
        WifiState::Hotspot { ssid: String::from("Quire-4F2A"), password: String::from("readmore"), ip: String::from("192.168.4.1") };
    sim.open("30-drop");
    visit(sim, "30-drop-hotspot", true);
    sim.reset();
    sim.env.card_free = Some(1_000_000);
    sim.open("30-drop");
    visit(sim, "30-drop-full", true);
    sim.env.card_free = Some(29_100_000_000);
    sim.reset();
    sim.env.wifi = WifiState::Off;

    // Wi-Fi: a scan, then the password keyboard for a secured network.
    sim.open("31-wifi");
    sim.press(Key::Down);
    sim.press(Key::Confirm);
    visit(sim, "31-wifi-scanning", false);
    sim.event(Event::WifiScan(vec![
        WifiNetwork { ssid: String::from("Bibliothek"), signal: 4, secured: true, saved: false },
        WifiNetwork { ssid: String::from("Cafe Lumiere"), signal: 2, secured: false, saved: false },
        WifiNetwork { ssid: String::from("HomeNet"), signal: 3, secured: true, saved: true },
    ]));
    visit(sim, "31-wifi-scan", false);
    sim.press(Key::Confirm);
    visit(sim, "31-wifi-password", true);
    sim.reset();

    // Flashcards: the back of a card, then the summary after the deck.
    sim.open("71-flashcards");
    sim.press(Key::Confirm);
    sim.press(Key::Confirm);
    visit(sim, "71-flashcards-back", false);
    for _ in 0..8 {
        if sim.top() == "71-flashcards-summary" {
            break;
        }
        sim.press(Key::Confirm);
    }
    visit(sim, "71-flashcards-summary", true);
    sim.reset();

    // News with a fetched article on the card.
    {
        use quire_ui::screens::apps::news::{store_articles, Article};
        let a = Article {
            feed: String::from("https://example.org/feed.xml"),
            feed_title: String::from("The Quiet Review"),
            title: String::from("On reading slowly"),
            url: String::from("https://example.org/reading-slowly"),
            published: NOW - 3 * 3600,
            text: String::from("There is a case for reading slowly: the page turns when you are ready, not before."),
            read: false,
        };
        store_articles(sim.env.fs(), vec![a]);
        sim.ui.settings.news_feeds.push(String::from("https://example.org/feed.xml"));
    }
    sim.open("72-news");
    visit(sim, "72-news-feeds", false);
    sim.press(Key::Confirm);
    visit(sim, "72-news-articles", true);
    sim.reset();

    // Interactive fiction: the transcript and the verb compass (when a story shipped).
    sim.open("76-fiction");
    sim.press(Key::Confirm);
    if sim.top() == "76-fiction-play" {
        visit(sim, "76-fiction-play", false);
        sim.press(Key::Left);
        visit(sim, "76-fiction-verbs", true);
    }
    sim.reset();

    // A game paused, and one played to the end.
    sim.open("80-2048");
    let mut last = sim.hash();
    let mut still = 0;
    for i in 0..3000 {
        sim.press([Key::Left, Key::Up, Key::Right, Key::Down][i % 4]);
        let h = sim.hash();
        still = if h == last { still + 1 } else { 0 };
        last = h;
        if still >= 4 {
            break;
        }
    }
    visit(sim, "80-2048-over", true);
    sim.reset();

    // Developer, after a few refreshes: the heap line has samples to step through.
    sim.open("90-developer");
    for _ in 0..6 {
        sim.press(Key::Confirm);
    }
    visit(sim, "90-developer-heap", true);
    sim.reset();

    // The keys locked: any key shows the strip for one refresh.
    sim.ui.locked = true;
    sim.press(Key::Confirm);
    visit(sim, "45-locked", false);
    sim.ui.locked = false;
    sim.event(Event::Tick);
    sim.reset();

    // Cards the platform pushes.
    sim.push(Box::new(quire_ui::screens::settings::OtaScreen::available(OtaInfo {
        version: String::from("0.2.0"),
        notes: String::from("Faster page turns on long chapters. The Spine now marks parts as well as chapters. Fixes a sleep-screen crash with very large covers."),
        url: String::from("https://updates.example.org/quire-0.2.0.bin"),
        size: 3_276_800,
    })));
    visit(sim, "51-ota-available", false);
    sim.press(Key::Confirm);
    sim.event(Event::Net(NetEvent::OtaProgress { done: 1_310_720, total: 3_276_800, finished: None }));
    visit(sim, "51-ota-working", false);
    sim.pop();
    let mut working = quire_ui::screens::Working::new("Adding books", "Moby-Dick; or, The Whale", "2 of 3");
    working.set(620, "2 of 3 · 1.2 MB");
    sim.push(working);
    visit(sim, "12-working", false);
    sim.pop();
    sim.reset();

    // A footnote card over the page (the note itself is not in this book).
    let card = sim.with_ctx(|cx| quire_ui::screens::reading::FootnoteCard::new(cx, "#note-1", "1"));
    sim.push(Box::new(card));
    visit(sim, "29-footnote", true);
    sim.reset();

    // The end of the book: the last page, then Right twice.
    let total = sim.ui.reader.as_ref().map(|r| r.book.total_chars()).unwrap_or(0);
    if total > 10 {
        let fs = &sim.env.fs;
        if let Some(r) = sim.ui.reader.as_mut() {
            r.goto_chars(fs, total - 10);
        }
        sim.ui.draw(&mut sim.env);
        for _ in 0..4 {
            if sim.top() == "2A-endofbook" {
                break;
            }
            sim.press(Key::Right);
        }
        visit(sim, "2A-endofbook", true);
        sim.reset();
        // Undo the finish so the rest of the walk sees the book as it was.
        if let Some(id) = sim.ui.reader.as_ref().map(|r| r.id) {
            let day = quire_library::time::day_of(sim.env.now);
            sim.ui.lib.set_finished(id, false, day);
        }
        sim.goto_chapter("Loomings");
    }
}

/// Nothing open: the empty home page (closing the book records a session, so this
/// comes last).
fn tour_home_empty(sim: &mut Sim, visit: &mut dyn FnMut(&mut Sim, &str, bool)) {
    let cur = sim.ui.reader.as_ref().map(|r| r.id);
    sim.ui.close_book(&mut sim.env);
    sim.reset();
    visit(sim, "10-home-empty", true);
    if let Some(id) = cur {
        sim.ui.apply(&mut sim.env, Action::Open(id));
        sim.index_all();
        sim.goto_chapter("Loomings");
    }
}

/// The first-run wizard on an empty card: its four pages.
fn tour_first_run(visit: &mut dyn FnMut(&mut Sim, &str, bool)) {
    let dir = std::env::temp_dir().join(format!("quire-sim-firstrun-tour-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut sim = Sim::boot(&dir);
    visit(&mut sim, "02-firstrun-language", false);
    sim.press(Key::Confirm);
    visit(&mut sim, "02-firstrun-time", false);
    sim.press(Key::Confirm);
    visit(&mut sim, "02-time-picker", false);
    sim.press(Key::Back);
    sim.press(Key::Right);
    visit(&mut sim, "02-firstrun-reader", false);
    sim.press(Key::Right);
    visit(&mut sim, "02-firstrun-books", false);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Walk the whole device (see [`tour_with`] and [`tour_special`]) and capture a frame at
/// each stop. Returns the shots in order.
pub fn tour(sim: &mut Sim) -> Vec<Shot> {
    let mut shots = Vec::new();
    let mut shoot = |sim: &mut Sim, name: &str, _leaf: bool| {
        shots.push(Shot {
            name: name.to_string(),
            frame: sim.frame().clone(),
            stack: sim.ui.stack_names(),
            refresh: sim.last_refresh,
            ms: sim.last_ms,
        });
    };
    tour_with(sim, &mut shoot);
    tour_special(sim, &mut shoot);
    shots
}

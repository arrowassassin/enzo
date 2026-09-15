//! Drive the whole UI with thousands of pseudo-random events from a fixed seed: nothing
//! may panic, the screen stack stays bounded, every frame has ink, and every event is
//! handled within the device budget (scaled for the host).

use quire_sim::{fixture_card, Sim};
use quire_ui::net::{NetEvent, OpdsEntry, OtaInfo};
use quire_ui::{Battery, Event, Key, KeyEvent, KeyKind, WifiNetwork, WifiState};

/// xorshift32 over a fixed seed: the same event stream every run.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u32) as usize]
    }
}

const KEYS: [Key; 7] = [Key::Left, Key::Back, Key::Confirm, Key::Right, Key::Up, Key::Down, Key::Power];

const PHONE: [&str; 8] =
    ["we", "ahab", "", "42", "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ", "é ü ß", "\n", "loomings"];

fn random_event(rng: &mut Rng, held: &mut Option<Key>) -> Event {
    // A held key produces Repeats until it is released; keep the key stream well formed
    // most of the time, with occasional stray Release/Repeat events like a bouncing switch.
    if let Some(k) = *held {
        return match rng.below(4) {
            0 => {
                *held = None;
                Event::Key(KeyEvent { key: k, kind: KeyKind::Release })
            }
            _ => Event::Key(KeyEvent { key: k, kind: KeyKind::Repeat }),
        };
    }
    match rng.below(100) {
        0..=54 => Event::Key(KeyEvent::press(*rng.pick(&KEYS))),
        55..=69 => {
            let k = *rng.pick(&KEYS);
            if rng.below(3) == 0 {
                *held = Some(k);
            }
            Event::Key(KeyEvent::long(k))
        }
        70..=72 => Event::Key(KeyEvent { key: *rng.pick(&KEYS), kind: KeyKind::Release }),
        73..=74 => Event::Key(KeyEvent { key: *rng.pick(&KEYS), kind: KeyKind::Repeat }),
        75..=79 => Event::PhoneText(String::from(*rng.pick(&PHONE))),
        80..=81 => Event::PhoneKey(KeyEvent::press(*rng.pick(&KEYS))),
        82..=85 => Event::Tick,
        86..=87 => Event::Timer,
        88 => Event::Battery(Battery {
            percent: rng.below(101) as u8,
            charging: rng.below(2) == 0,
            days_left: if rng.below(2) == 0 { None } else { Some(rng.below(60) as u16) },
            cycles: Some(rng.below(500) as u16),
            health: Some(rng.below(101) as u8),
            millivolts: 3300 + rng.below(900) as u16,
        }),
        89..=90 => Event::Wifi(match rng.below(5) {
            0 => WifiState::Off,
            1 => WifiState::Connecting("HomeNet".into()),
            2 => {
                WifiState::Connected { ssid: "HomeNet".into(), ip: "192.168.1.20".into(), host: "quire".into(), signal: rng.below(5) as u8 }
            }
            3 => WifiState::Hotspot { ssid: "Quire-1234".into(), password: "readmore".into(), ip: "192.168.4.1".into() },
            _ => WifiState::Failed("HomeNet".into()),
        }),
        91 => Event::WifiScan(vec![
            WifiNetwork { ssid: "HomeNet".into(), signal: 3, secured: true, saved: true },
            WifiNetwork { ssid: "Cafe".into(), signal: 1, secured: false, saved: false },
        ]),
        92..=93 => Event::Ingest {
            id: quire_library::BookId(rng.below(6) as u64),
            done: rng.below(10),
            total: rng.below(10),
            finished: match rng.below(3) {
                0 => None,
                1 => Some(Ok(())),
                _ => Some(Err("bad zip".into())),
            },
        },
        94 => Event::BooksChanged,
        95 => Event::Wake,
        _ => Event::Net(match rng.below(11) {
            0 => NetEvent::Downloads,
            1 => NetEvent::Opds(Ok(("Feed".into(), vec![OpdsEntry { title: "A".into(), ..Default::default() }]))),
            2 => NetEvent::Opds(Err("timeout".into())),
            3 => NetEvent::Shelves,
            4 => NetEvent::Wikipedia(Ok(("Whale".into(), "A large marine mammal.".into()))),
            5 => NetEvent::Weather,
            6 => NetEvent::News,
            7 => NetEvent::Ota(Ok(Some(OtaInfo { version: "9.9.9".into(), notes: "notes".into(), url: "http://x".into(), size: 1 }))),
            8 => NetEvent::OtaProgress { done: 1, total: 2, finished: Some(Err("fail".into())) },
            9 => NetEvent::Sync(Ok(3)),
            _ => NetEvent::SleepPacks,
        }),
    }
}

/// Feed `n` random events; assert the invariants after each one.
fn drive(sim: &mut Sim, seed: u32, n: usize, label: &str) {
    let mut rng = Rng(seed);
    let mut held = None;
    let budget_ms: u128 = if cfg!(debug_assertions) { 5000 } else { 500 };
    let mut worst = (0u128, String::new(), 0usize);
    for i in 0..n {
        let ev = random_event(&mut rng, &mut held);
        let desc = format!("{ev:?}");
        // The clock moves so sessions, timers and autosave get exercised.
        sim.env.now += 1 + rng.below(20);
        sim.event(ev);
        let stack = sim.ui.stack_names();
        assert!(!stack.is_empty(), "{label}: event {i} ({desc}) emptied the stack");
        assert!(stack.len() <= 8, "{label}: event {i} ({desc}) grew the stack to {stack:?}");
        let ink = sim.frame().ink_count();
        assert!(ink > 0, "{label}: event {i} ({desc}) left a blank frame on {stack:?}");
        if sim.last_ms > worst.0 {
            worst = (sim.last_ms, format!("{desc} on {stack:?}"), i);
        }
        assert!(sim.last_ms < budget_ms, "{label}: event {i} ({desc}) on {stack:?} took {} ms", sim.last_ms);
    }
    eprintln!("{label}: {n} events, worst {} ms at event {} ({})", worst.0, worst.2, worst.1);
}

#[test]
fn random_events_on_the_fixture_card() {
    let card = fixture_card("fuzz");
    let mut sim = Sim::boot(&card);
    assert_eq!(sim.top(), "20-reading");
    drive(&mut sim, 0x9E37_79B9, 4000, "fixture");
    // The UI is still usable: everything pops back to the page and it draws.
    sim.reset();
    assert_eq!(sim.top(), "20-reading");
    assert!(sim.frame().ink_count() > 200);
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn random_events_on_an_empty_card() {
    let dir = std::env::temp_dir().join(format!("quire-sim-fuzz-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut sim = Sim::boot(&dir);
    assert!(sim.top().starts_with("02-firstrun"), "{}", sim.top());
    drive(&mut sim, 0x1234_5678, 3000, "empty");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn random_events_with_no_book_open() {
    let card = fixture_card("fuzz-closed");
    let mut sim = Sim::boot(&card);
    sim.ui.close_book(&mut sim.env);
    sim.ui.lib.current = None;
    sim.reset();
    assert!(sim.ui.reader.is_none());
    drive(&mut sim, 0xDEAD_BEEF, 3000, "closed");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn sweep_many_seeds() {
    let n: u32 = std::env::var("FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    if n == 0 {
        return;
    }
    let card = fixture_card("fuzz-sweep");
    for seed in 1..=n {
        let mut sim = Sim::boot(&card);
        if seed % 3 == 0 {
            sim.ui.close_book(&mut sim.env);
            sim.ui.lib.current = None;
            sim.reset();
        }
        drive(&mut sim, seed.wrapping_mul(0x9E37_79B9) | 1, 2500, &format!("seed {seed}"));
    }
    let _ = std::fs::remove_dir_all(&card);
}

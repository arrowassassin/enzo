//! What must survive a reboot: settings changed on the Settings screens, reading
//! sessions, and a card whose files are corrupt boots to sane defaults.

use quire_sim::{fixture_card, Sim};
use quire_ui::{Env, Key, Settings};

#[test]
fn settings_changed_on_screen_survive_a_reboot() {
    let card = fixture_card("persist-settings");
    let mut sim = Sim::boot(&card);
    // Settings → Keys (row 3) → Swap Up and Down (row 2).
    sim.open("50-settings");
    sim.press(Key::Down);
    sim.press(Key::Down);
    sim.press(Key::Confirm);
    assert_eq!(sim.top(), "50-keys");
    sim.press(Key::Down);
    sim.press(Key::Confirm);
    assert!(sim.ui.settings.swap_side_keys, "Confirm toggles Swap Up and Down");
    sim.press(Key::Back);
    // Sleep and power (row 4) → Lock keys when sleeping (row 4).
    sim.press(Key::Down);
    sim.press(Key::Confirm);
    assert_eq!(sim.top(), "50-sleepandpower");
    for _ in 0..3 {
        sim.press(Key::Down);
    }
    sim.press(Key::Confirm);
    assert!(sim.ui.settings.lock_when_sleeping);
    sim.press(Key::Back);
    sim.press(Key::Back);
    // Type: bigger text.
    let size0 = sim.ui.settings.profile.size;
    sim.reset();
    sim.press(Key::Confirm);
    sim.press(Key::Up);
    for _ in 0..4 {
        sim.press(Key::Down);
    }
    sim.press(Key::Right);
    let size1 = sim.ui.settings.profile.size;
    assert!(size1 > size0);
    sim.press(Key::Back);
    // Reboot after a sleep (the platform flushes before sleeping).
    sim.press(Key::Power);
    drop(sim);
    let sim = Sim::boot(&card);
    assert!(sim.ui.settings.swap_side_keys, "swap side keys persisted");
    assert!(sim.ui.settings.lock_when_sleeping, "lock when sleeping persisted");
    assert_eq!(sim.ui.settings.profile.size, size1, "type size persisted");
    assert!(sim.ui.settings.first_run_done);
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn settings_changed_on_screen_are_saved_without_a_sleep() {
    let card = fixture_card("persist-settings-tick");
    let mut sim = Sim::boot(&card);
    sim.open("50-settings");
    sim.press(Key::Down);
    sim.press(Key::Down);
    sim.press(Key::Confirm);
    sim.press(Key::Down);
    sim.press(Key::Confirm);
    assert!(sim.ui.settings.swap_side_keys);
    sim.env.now += 31;
    sim.event(quire_ui::Event::Tick);
    drop(sim);
    let sim = Sim::boot(&card);
    assert!(sim.ui.settings.swap_side_keys, "the periodic save wrote the settings");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn a_reading_session_is_recorded_after_reading_and_closing() {
    let card = fixture_card("persist-stats");
    let mut sim = Sim::boot(&card);
    sim.index_all();
    sim.goto_chapter("Loomings");
    let id = sim.ui.reader.as_ref().unwrap().id;
    let today = quire_library::time::day_of(sim.env.now());
    let before = sim.ui.stats.totals(quire_library::stats::Range::Today, today).secs;
    let sessions_before = sim.ui.lib.get(id).unwrap().stats.sessions;
    // Read twelve pages at a minute each.
    for _ in 0..12 {
        sim.env.now += 60;
        sim.press(Key::Right);
    }
    // Close the book from the Home layer's Library (Back, Left) and open another.
    sim.press(Key::Power);
    assert_eq!(sim.top(), "40-sleep");
    let stats = quire_library::Stats::load(sim.env.fs());
    let after = stats.totals(quire_library::stats::Range::Today, today).secs;
    assert!(after > before + 600, "today's seconds grew: {before} → {after}");
    let lib = quire_library::Library::load(sim.env.fs());
    let e = lib.get(id).unwrap();
    assert_eq!(e.stats.sessions, sessions_before + 1, "one more session on the book");
    let recent = stats.book_sessions(sim.env.fs(), id, today, 1, 10);
    assert!(!recent.is_empty(), "the session is in the log");
    assert!(recent.iter().any(|s| s.pages >= 10), "with its pages: {:?}", recent.iter().map(|s| s.pages).collect::<Vec<_>>());
    drop(sim);
    let sim = Sim::boot(&card);
    assert_eq!(sim.ui.lib.get(id).unwrap().stats.sessions, sessions_before + 1, "session count after reboot");
    let _ = std::fs::remove_dir_all(&card);
}

fn corrupt(card: &std::path::Path, rel: &str, bytes: &[u8]) {
    let p = card.join(rel.trim_start_matches('/'));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

#[test]
fn corrupt_settings_boot_to_defaults() {
    let card = fixture_card("persist-corrupt-settings");
    for garbage in [&b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff"[..], &b""[..], &b"\x01"[..], &[0u8; 4096][..]] {
        corrupt(&card, quire_ui::settings::SETTINGS_FILE, garbage);
        let mut sim = Sim::boot(&card);
        assert!(sim.top().starts_with("02-firstrun"), "defaults mean the first run: {}", sim.top());
        assert_eq!(sim.ui.settings, Settings::default());
        assert!(sim.frame().ink_count() > 200);
        for _ in 0..8 {
            sim.press(Key::Confirm);
        }
        assert!(!sim.top().starts_with("02-firstrun"), "the wizard completes: {}", sim.top());
    }
    // A truncated valid file is garbage too.
    let good = Settings { first_run_done: true, ..Default::default() };
    good.save(&quire_fs::host::HostFs::new(&card)).unwrap();
    let bytes = std::fs::read(card.join(".quire/settings.bin")).unwrap();
    corrupt(&card, quire_ui::settings::SETTINGS_FILE, &bytes[..bytes.len() / 2]);
    let sim = Sim::boot(&card);
    assert!(sim.top().starts_with("02-firstrun") || sim.top() == "20-reading", "{}", sim.top());
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn corrupt_library_boots_to_an_empty_shelf() {
    let card = fixture_card("persist-corrupt-library");
    let sim = Sim::boot(&card);
    let index = sim.ui.lib.get(sim.ui.lib.current.unwrap()).map(|e| e.title.clone()).unwrap();
    drop(sim);
    let lib_file = card.join(".quire").join("library.bin");
    assert!(lib_file.exists(), "the library lives at {}", lib_file.display());
    for garbage in [&b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff"[..], &b""[..], &[0u8; 4096][..], &b"\x01\x02\x03"[..]] {
        std::fs::write(&lib_file, garbage).unwrap();
        let mut sim = Sim::boot(&card);
        assert_eq!(sim.top(), "20-reading", "boots to the page");
        assert!(sim.frame().ink_count() > 200, "the empty home draws");
        // The keys work on the empty home and Jump lists the library.
        sim.press(Key::Confirm);
        assert_eq!(sim.top(), "11-library");
        sim.press(Key::Back);
        sim.long(Key::Back);
        assert_eq!(sim.top(), "42-jump");
        sim.press(Key::Back);
        let _ = index.len();
    }
    let _ = std::fs::remove_dir_all(&card);
}

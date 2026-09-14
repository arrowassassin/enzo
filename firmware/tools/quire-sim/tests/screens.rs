//! Every screen renders, nothing panics, the key grammar holds, and the pixels match the
//! recorded snapshot hashes (`snapshots.txt`; regenerate with `UPDATE_SNAPSHOTS=1`).

use quire_sim::{fixture_card, frame_to_png, tour, Sim};
use quire_ui::{Env, Key, Refresh};
use std::collections::BTreeMap;
use std::path::Path;

fn snapshot_file() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("snapshots.txt")
}

#[test]
fn every_screen_draws_and_matches_snapshots() {
    let card = fixture_card("tour");
    let mut sim = Sim::boot(&card);
    assert_eq!(sim.top(), "20-reading", "boots into the book");
    let shots = tour(&mut sim);
    assert!(shots.len() > 80, "{} shots", shots.len());

    let mut seen = BTreeMap::new();
    for s in &shots {
        let ink = s.frame.ink_count();
        assert!(ink > 200, "{} drew almost nothing ({ink} px)", s.name);
        assert!(ink < 792 * 528 * 9 / 10, "{} is nearly all black ({ink} px)", s.name);
        assert!(!s.stack.is_empty(), "{}: empty stack", s.name);
        assert!(s.stack.len() <= 6, "{}: stack grew to {:?}", s.name, s.stack);
        assert!(seen.insert(s.name.clone(), quire_sim::frame_hash(&s.frame)).is_none(), "duplicate shot name {}", s.name);
    }
    // Dialog and compass ask for GC; page turns are DU.
    let by_name: BTreeMap<&str, &quire_sim::Shot> = shots.iter().map(|s| (s.name.as_str(), s)).collect();
    assert_eq!(by_name["20-reading-next"].refresh, Refresh::Du);
    assert_eq!(by_name["24-compass"].refresh, Refresh::Gc);
    assert_eq!(by_name["40-sleep"].refresh, Refresh::Gc);

    let file = snapshot_file();
    let recorded: BTreeMap<String, u64> = std::fs::read_to_string(&file)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (n, h) = l.split_once(' ')?;
            Some((n.to_string(), u64::from_str_radix(h, 16).ok()?))
        })
        .collect();
    let update = std::env::var("UPDATE_SNAPSHOTS").is_ok();
    if update || recorded.is_empty() {
        let text: String = seen.iter().map(|(n, h)| format!("{n} {h:016x}\n")).collect();
        std::fs::write(&file, text).unwrap();
    } else {
        let mut diffs = Vec::new();
        for (n, h) in &seen {
            match recorded.get(n) {
                Some(r) if r == h => {}
                Some(_) => diffs.push(format!("changed: {n}")),
                None => diffs.push(format!("new: {n}")),
            }
        }
        for n in recorded.keys() {
            if !seen.contains_key(n) {
                diffs.push(format!("gone: {n}"));
            }
        }
        if !diffs.is_empty() {
            let out = std::env::temp_dir().join("quire-sim-diff");
            std::fs::create_dir_all(&out).unwrap();
            for s in &shots {
                frame_to_png(&s.frame, &out.join(format!("{}.png", s.name))).unwrap();
            }
            panic!("snapshots differ (PNGs in {}):\n{}\nrun with UPDATE_SNAPSHOTS=1 to accept", out.display(), diffs.join("\n"));
        }
    }
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn key_grammar_holds_everywhere() {
    let card = fixture_card("grammar");
    let mut sim = Sim::boot(&card);
    // Long Back opens Jump from anywhere and Back closes it.
    for target in ["11-library", "50-settings", "60a-overview", "78-calculator", "80-sudoku"] {
        sim.open(target);
        sim.long(Key::Back);
        assert_eq!(sim.top(), "42-jump", "from {target}");
        sim.press(Key::Back);
        assert_eq!(sim.top(), target);
        // Back pops every pushed screen; in a game Back pauses and Right on the pause card quits.
        for _ in 0..8 {
            let top = sim.top();
            if top == "20-reading" {
                break;
            }
            sim.press(Key::Back);
            if top.starts_with("80-") && top != "80-games" {
                assert_eq!(sim.top(), "80-paused", "Back pauses {top}");
                sim.press(Key::Right);
            }
        }
        assert_eq!(sim.top(), "20-reading", "Back returns to the book from {target}");
    }
    // Power sleeps; Wake returns to the page.
    sim.press(Key::Power);
    assert_eq!(sim.top(), "40-sleep");
    assert!(sim.env.requests.iter().any(|r| matches!(r, quire_ui::SysRequest::Sleep)));
    sim.event(quire_ui::Event::Wake);
    assert_eq!(sim.top(), "20-reading", "waking returns to the page");
    // Pages advance and the position persists (from a text chapter: the front matter's
    // image pages carry no characters).
    sim.index_all();
    assert!(sim.goto_chapter("Loomings"));
    let before = sim.ui.reader.as_ref().unwrap().loc().chars;
    sim.press(Key::Right);
    sim.press(Key::Right);
    let after = sim.ui.reader.as_ref().unwrap().loc().chars;
    assert!(after > before, "two pages forward: {before} → {after}");
    sim.press(Key::Left);
    let back = sim.ui.reader.as_ref().unwrap().loc().chars;
    assert!(back < after && back >= before);
    sim.ui.flush(&mut sim.env);
    let lib = quire_library::Library::load(sim.env.fs());
    let e = lib.get(lib.current.unwrap()).unwrap();
    assert_eq!(e.loc.chars, back, "position saved to the card");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn first_run_when_nothing_is_set_up() {
    let dir = std::env::temp_dir().join(format!("quire-sim-firstrun-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut sim = Sim::boot(&dir);
    assert!(sim.top().starts_with("02-firstrun"), "{}", sim.top());
    assert!(sim.frame().ink_count() > 500);
    for _ in 0..6 {
        sim.press(Key::Confirm);
    }
    assert!(sim.ui.settings.first_run_done || !sim.top().starts_with("02-firstrun"), "wizard advances");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn events_stay_fast() {
    let card = fixture_card("speed");
    let mut sim = Sim::boot(&card);
    sim.index_all();
    let mut worst = 0u128;
    for _ in 0..40 {
        sim.press(Key::Right);
        worst = worst.max(sim.last_ms);
    }
    // Debug builds are ~10× slower than release; the device budget is 150 ms per page.
    assert!(worst < 1500, "page turn took {worst} ms on the host (debug)");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn edge_labels_fit_their_cells() {
    use quire_gfx::{measure_text, TextStyle};
    // Rail cells are 132 px at mono 18; compass choices are 26 px in the same cell.
    let mono = quire_fonts::ui::mono();
    for l in [
        "Library", "Close", "Continue", "Bookshop", "Contents", "Cancel", "Delete", "Rhythm", "Calendar", "Books", "Verbs", "Send", "Type",
        "Open", "Back", "Restart", "Retry",
    ] {
        let w = measure_text(mono, l, TextStyle::INK);
        assert!(w <= 132 - 8, "rail label {l} is {w} px");
    }
    // Compass choices try 26 px and fall back to 22 px for the row; every label must fit at 22.
    let body = quire_fonts::ui::body();
    for l in ["Contents", "Close", "Bookmark", "Go to", "Type", "More", "Cursor", "Notes", "Stats", "Layout", "Sleep", "Bookmarked"] {
        let w = measure_text(body, l, TextStyle::INK);
        assert!(w <= 132 - 8, "compass label {l} is {w} px at 22");
    }
}

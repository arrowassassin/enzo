//! Throwaway efficiency benchmark (segment-3 review): counts heap allocations, transient
//! peak and retained heap per event, and host-release timing, for the reading page and
//! every major screen. Run with:
//!   cargo test -p quire-sim --release --test efficiency -- --nocapture --test-threads=1

use quire_sim::{fixture_card, Sim};
use quire_ui::{Action, Env, Event, Key, KeyEvent, KeyKind};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

struct Counting;
static COUNT: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static BIGGEST: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            COUNT.fetch_add(1, Relaxed);
            BYTES.fetch_add(l.size(), Relaxed);
            let live = LIVE.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(live, Relaxed);
            BIGGEST.fetch_max(l.size(), Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        System.dealloc(p, l)
    }
}

#[global_allocator]
static A: Counting = Counting;

struct Row {
    name: String,
    allocs: usize,
    bytes: usize,
    peak: usize,
    retained: i64,
    biggest: usize,
    us: u128,
    refresh: String,
    top: &'static str,
}

fn measure<F: FnOnce(&mut Sim)>(sim: &mut Sim, name: &str, f: F) -> Row {
    let live0 = LIVE.load(Relaxed);
    PEAK.store(live0, Relaxed);
    COUNT.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    BIGGEST.store(0, Relaxed);
    let t = Instant::now();
    f(sim);
    let us = t.elapsed().as_micros();
    Row {
        name: name.to_string(),
        allocs: COUNT.load(Relaxed),
        bytes: BYTES.load(Relaxed),
        peak: PEAK.load(Relaxed).saturating_sub(live0),
        retained: LIVE.load(Relaxed) as i64 - live0 as i64,
        biggest: BIGGEST.load(Relaxed),
        us,
        refresh: format!("{:?}", sim.last_refresh),
        top: sim.top(),
    }
}

fn kb(b: usize) -> String {
    format!("{:.1}", b as f64 / 1024.0)
}

fn print(rows: &[Row]) {
    println!("\n| event | allocs | bytes KB | transient peak KB | retained KB | biggest KB | host ms | refresh | top |");
    println!("|---|---:|---:|---:|---:|---:|---:|---|---|");
    for r in rows {
        println!(
            "| {} | {} | {} | {} | {:.1} | {} | {:.2} | {} | {} |",
            r.name,
            r.allocs,
            kb(r.bytes),
            kb(r.peak),
            r.retained as f64 / 1024.0,
            kb(r.biggest),
            r.us as f64 / 1000.0,
            r.refresh,
            r.top
        );
    }
}

fn press(sim: &mut Sim, k: Key) {
    sim.press(k);
}
fn tick(sim: &mut Sim) {
    sim.event(Event::Tick);
}
fn redraw(sim: &mut Sim) {
    sim.ui.apply(&mut sim.env, Action::Redraw);
}

#[test]
fn efficiency_numbers() {
    let card = fixture_card("eff");
    // A Z-machine story on the card so Interactive fiction can be measured.
    std::fs::create_dir_all(card.join("stories")).unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/quire-ui/data/advent.z5"),
        card.join("stories/advent.z5"),
    )
    .unwrap();
    let mut rows = Vec::new();
    let live_start = LIVE.load(Relaxed);
    let t = Instant::now();
    let mut sim = Sim::boot(&card);
    println!(
        "boot (Ui::new + first draw): {} ms, live heap after boot {} KB",
        t.elapsed().as_millis(),
        kb(LIVE.load(Relaxed).saturating_sub(live_start))
    );
    let after_boot = LIVE.load(Relaxed);

    // --- reading page ---------------------------------------------------------------
    rows.push(measure(&mut sim, "reading: Tick (prerender+index_step), index incomplete", tick));
    rows.push(measure(&mut sim, "reading: Tick again", tick));
    rows.push(measure(&mut sim, "reading: Right (page turn, prerendered)", |s| press(s, Key::Right)));
    rows.push(measure(&mut sim, "reading: Right (page turn, not prerendered)", |s| press(s, Key::Right)));
    rows.push(measure(&mut sim, "reading: Right (page turn, not prerendered) #2", |s| press(s, Key::Right)));
    rows.push(measure(&mut sim, "reading: Left (page turn back)", |s| press(s, Key::Left)));
    rows.push(measure(&mut sim, "reading: Redraw (frame_cache hit)", redraw));
    rows.push(measure(&mut sim, "reading: Down long (next chapter, loads section)", |s| {
        s.event(Event::Key(KeyEvent::long(Key::Down)));
        s.event(Event::Key(KeyEvent { key: Key::Down, kind: KeyKind::Release }));
    }));
    // 40 turns, average.
    let live0 = LIVE.load(Relaxed);
    COUNT.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    PEAK.store(live0, Relaxed);
    let t = Instant::now();
    for _ in 0..40 {
        sim.press(Key::Right);
    }
    let el = t.elapsed().as_micros();
    println!(
        "40 page turns (no ticks): {:.2} ms avg, {} allocs avg, {} KB allocated avg, transient peak {} KB, retained {} KB",
        el as f64 / 40_000.0,
        COUNT.load(Relaxed) / 40,
        kb(BYTES.load(Relaxed) / 40),
        kb(PEAK.load(Relaxed).saturating_sub(live0)),
        kb(LIVE.load(Relaxed).saturating_sub(live0))
    );
    let live0 = LIVE.load(Relaxed);
    COUNT.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    PEAK.store(live0, Relaxed);
    let t = Instant::now();
    for _ in 0..40 {
        sim.event(Event::Tick);
        sim.press(Key::Right);
    }
    let el = t.elapsed().as_micros();
    println!(
        "40 x (Tick + Right) with prerender: {:.2} ms avg per pair, {} allocs avg, {} KB allocated avg, transient peak {} KB",
        el as f64 / 40_000.0,
        COUNT.load(Relaxed) / 40,
        kb(BYTES.load(Relaxed) / 40),
        kb(PEAK.load(Relaxed).saturating_sub(live0))
    );
    println!("live heap on reading page (book open, frame + frame_cache + next_frame): {} KB", kb(LIVE.load(Relaxed).saturating_sub(live_start)));
    // Skim.
    rows.push(measure(&mut sim, "reading: hold Right (Long + 6 Repeats + Release)", |s| {
        s.hold(Key::Right, 6);
    }));
    rows.push(measure(&mut sim, "reading: index_all (full page index build, host)", |s| s.index_all()));
    let live_after_index = LIVE.load(Relaxed);
    println!(
        "live heap after index_all: {} KB (delta vs boot {} KB)",
        kb(live_after_index.saturating_sub(live_start)),
        (live_after_index as i64 - after_boot as i64) / 1024
    );
    rows.push(measure(&mut sim, "reading: Tick with index complete", tick));

    // --- overlays ---------------------------------------------------------------------
    rows.push(measure(&mut sim, "Back -> 10-home", |s| press(s, Key::Back)));
    rows.push(measure(&mut sim, "10-home: Down (redraw)", |s| press(s, Key::Down)));
    sim.reset();
    rows.push(measure(&mut sim, "Confirm -> 21-compass", |s| press(s, Key::Confirm)));
    rows.push(measure(&mut sim, "21-compass: Down -> compass-more (page_words + rarest)", |s| press(s, Key::Down)));
    sim.reset();
    rows.push(measure(&mut sim, "long Confirm -> 26-cursor", |s| {
        s.long(Key::Confirm);
    }));
    rows.push(measure(&mut sim, "26-cursor: Right", |s| press(s, Key::Right)));
    rows.push(measure(&mut sim, "26-cursor: Confirm -> 27-dictionary (no dict)", |s| press(s, Key::Confirm)));
    sim.reset();
    rows.push(measure(&mut sim, "Confirm,Left -> 22-contents", |s| {
        press(s, Key::Confirm);
        press(s, Key::Left);
    }));
    rows.push(measure(&mut sim, "22-contents: Down", |s| press(s, Key::Down)));
    sim.reset();
    rows.push(measure(&mut sim, "Confirm,Right -> 23-goto", |s| {
        press(s, Key::Confirm);
        press(s, Key::Right);
    }));
    rows.push(measure(&mut sim, "23-goto: Right (percent +1)", |s| press(s, Key::Right)));
    sim.reset();
    rows.push(measure(&mut sim, "Confirm,Up -> 25-type (specimen)", |s| {
        press(s, Key::Confirm);
        press(s, Key::Up);
    }));
    rows.push(measure(&mut sim, "25-type: Down x4 (focus size), Right (bigger: set_profile)", |s| {
        for _ in 0..4 {
            press(s, Key::Down);
        }
        press(s, Key::Right);
    }));
    rows.push(measure(&mut sim, "25-type: Left (smaller: set_profile)", |s| press(s, Key::Left)));
    sim.reset();
    rows.push(measure(&mut sim, "reading after type change: Tick", tick));
    rows.push(measure(&mut sim, "long Back -> 42-jump", |s| {
        s.long(Key::Back);
    }));
    rows.push(measure(&mut sim, "42-jump: Down", |s| press(s, Key::Down)));
    rows.push(measure(&mut sim, "42-jump: phone 'we' (filter)", |s| {
        s.phone("we");
    }));
    sim.reset();

    // --- jump targets -----------------------------------------------------------------
    let targets: &[(&'static str, &[Key])] = &[
        ("11-library", &[Key::Down, Key::Right, Key::Right]),
        ("35-bookshop", &[Key::Down, Key::Confirm, Key::Confirm]),
        ("30-drop", &[]),
        ("60a-overview", &[Key::Right, Key::Right, Key::Right, Key::Right]),
        ("61-yearinreview", &[Key::Right]),
        ("50-settings", &[Key::Down, Key::Confirm, Key::Down]),
        ("50-battery", &[]),
        ("50-about", &[]),
        ("28-highlights", &[]),
        ("25-layout", &[Key::Down, Key::Right]),
        ("44-picker", &[Key::Down, Key::Down, Key::Right]),
        ("31-wifi", &[Key::Down]),
        ("13-folders", &[Key::Confirm, Key::Down]),
        ("32-opds", &[]),
        ("39-downloads", &[]),
        ("90-developer", &[Key::Down]),
        ("70-apps", &[Key::Down]),
        ("80-games", &[Key::Down]),
        ("73-clock", &[Key::Down]),
        ("71-flashcards", &[Key::Confirm]),
        ("72-news", &[]),
        ("74-weather", &[]),
        ("77-wikipedia", &[]),
        ("78-calculator", &[Key::Right, Key::Confirm]),
        ("79-notes", &[Key::Confirm]),
        ("75-images", &[]),
        ("76-fiction", &[Key::Confirm, Key::Down]),
        ("80-sudoku", &[Key::Confirm, Key::Right]),
        ("80-2048", &[Key::Left]),
        ("80-minesweeper", &[Key::Confirm, Key::Right]),
        ("80-wordle", &[Key::Right]),
    ];
    for (name, keys) in targets {
        sim.reset();
        rows.push(measure(&mut sim, &format!("open {name}"), |s| {
            s.open(name);
        }));
        for (i, k) in keys.iter().enumerate() {
            rows.push(measure(&mut sim, &format!("{name}: key {i} {k:?}"), |s| press(s, *k)));
        }
        rows.push(measure(&mut sim, &format!("{name}: Redraw"), redraw));
        rows.push(measure(&mut sim, &format!("{name}: Tick"), tick));
    }
    // Library grid mode.
    sim.reset();
    sim.ui.settings.library_grid = true;
    rows.push(measure(&mut sim, "open 11-library (grid)", |s| {
        s.open("11-library");
    }));
    rows.push(measure(&mut sim, "11-library grid: Right", |s| press(s, Key::Right)));
    sim.ui.settings.library_grid = false;
    // Chess engine move.
    sim.reset();
    sim.open("80-chess");
    rows.push(measure(&mut sim, "80-chess: open + Confirm (select e2)", |s| press(s, Key::Confirm)));
    rows.push(measure(&mut sim, "80-chess: Up,Up,Confirm (e2e4 -> thinking)", |s| {
        press(s, Key::Up);
        press(s, Key::Up);
        press(s, Key::Confirm);
    }));
    rows.push(measure(&mut sim, "80-chess: Timer (engine best_move depth 3)", |s| {
        s.event(Event::Timer);
    }));
    rows.push(measure(&mut sim, "80-chess: Redraw", redraw));
    // Sleep and power.
    sim.reset();
    rows.push(measure(&mut sim, "long Power -> 41-power", |s| {
        s.long(Key::Power);
    }));
    press(&mut sim, Key::Back);
    rows.push(measure(&mut sim, "Power -> 40-sleep (Cover)", |s| press(s, Key::Power)));
    rows.push(measure(&mut sim, "40-sleep: Battery event (redraw)", |s| {
        s.event(Event::Battery(s.env.battery));
    }));
    sim.event(Event::Wake);
    sim.reset();
    for v in quire_ui::settings::SleepVariant::ALL {
        sim.ui.settings.sleep = v;
        rows.push(measure(&mut sim, &format!("Power -> 40-sleep ({v:?})"), |s| press(s, Key::Power)));
        sim.event(Event::Wake);
        sim.reset();
    }
    sim.ui.settings.sleep = quire_ui::settings::SleepVariant::Cover;
    rows.push(measure(&mut sim, "reading: Wake + reset", |s| {
        s.reset();
    }));
    print(&rows);
    println!("final live heap: {} KB", kb(LIVE.load(Relaxed).saturating_sub(live_start)));
    let _ = std::fs::remove_dir_all(&card);
}

fn m2<F: FnOnce(&mut Sim)>(sim: &mut Sim, name: &str, f: F) {
    let r = measure(sim, name, f);
    println!(
        "| {} | {} | {} | {} | {:.1} | {} | {:.3} |",
        r.name,
        r.allocs,
        kb(r.bytes),
        kb(r.peak),
        r.retained as f64 / 1024.0,
        kb(r.biggest),
        r.us as f64 / 1000.0
    );
}

#[test]
fn reader_method_costs() {
    let card = fixture_card("eff2");
    let mut sim = Sim::boot(&card);
    sim.index_all();
    sim.goto_chapter("Loomings");
    for _ in 0..3 {
        sim.press(Key::Right);
    }
    println!("\n| call | allocs | bytes KB | transient peak KB | retained KB | biggest KB | host ms |");
    println!("|---|---:|---:|---:|---:|---:|---:|");
    m2(&mut sim, "Reader::loc() (chars_at -> chars_before)", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.loc();
    });
    m2(&mut sim, "Reader::info()", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.info();
    });
    m2(&mut sim, "Reader::chapter_title()", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.chapter_title();
    });
    m2(&mut sim, "Reader::page_number()", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.page_number();
    });
    m2(&mut sim, "Reader::total_pages()", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.total_pages();
    });
    m2(&mut sim, "Reader::spine_model()", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.spine_model();
    });
    m2(&mut sim, "Reader::time_left()", |s| {
        let (lib, stats) = (&s.ui.lib, &s.ui.stats);
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.time_left(lib, stats);
    });
    m2(&mut sim, "Reader::page() (cached_page None -> layout)", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        r.force_gc();
        let _ = r.page();
    });
    m2(&mut sim, "Reader::page() (cached: clone)", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.page();
    });
    m2(&mut sim, "Reader::page_words()", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.page_words();
    });
    m2(&mut sim, "cursor::rarest_word(page_words)", |s| {
        let r = s.ui.reader.as_mut().unwrap();
        let w = r.page_words();
        let _ = quire_ui::screens::cursor::rarest_word(&w);
    });
    let mut frame = quire_gfx::Frame::panel();
    m2(&mut sim, "Reader::render() cache hit (copy_rect_from)", |s| {
        let fs = s.env.fs();
        let settings = &s.ui.settings;
        let r = s.ui.reader.as_mut().unwrap();
        let _ = r.render(fs, &mut frame, settings);
    });
    let f2 = frame.clone();
    let t = Instant::now();
    for _ in 0..10 {
        frame.copy_rect_from(&f2, f2.bounds());
    }
    println!("Frame::copy_rect_from full frame: {:.3} ms host each", t.elapsed().as_micros() as f64 / 10_000.0);
    let t = Instant::now();
    for _ in 0..10 {
        let _ = f2.rotated(quire_gfx::Rotation::Cw90);
    }
    println!("Frame::rotated(Cw90) full frame: {:.3} ms host each", t.elapsed().as_micros() as f64 / 10_000.0);
    let t = Instant::now();
    for _ in 0..10 {
        frame.screen_rect(frame.bounds(), quire_gfx::Pattern::Dots50);
    }
    println!("Frame::screen_rect full frame Dots50: {:.3} ms host each", t.elapsed().as_micros() as f64 / 10_000.0);
    let t = Instant::now();
    for _ in 0..10 {
        frame.blit(0, 0, f2.as_bitmap(), quire_gfx::BlitMode::Or);
    }
    println!("Frame::blit full frame Or: {:.3} ms host each", t.elapsed().as_micros() as f64 / 10_000.0);
    // Word rarity lookups.
    let t = Instant::now();
    let mut acc = 0u32;
    for w in ["whale", "Ishmael", "the", "circumambulate", "purse", "spleen"] {
        acc = acc.wrapping_add(quire_ui::screens::cursor::rarity(w));
    }
    println!("cursor::rarity x6 words: {:.3} ms host total ({acc})", t.elapsed().as_micros() as f64 / 1000.0);
    let t = Instant::now();
    let a = quire_ui::screens::games::wordle::answer_for(20_000);
    println!("wordle::answer_for: {:.1} ms host ({a})", t.elapsed().as_micros() as f64 / 1000.0);
    m2(&mut sim, "Reader::render() uncached (page turn cost incl. frame clone)", |s| {
        let fs = s.env.fs();
        let settings = &s.ui.settings;
        let r = s.ui.reader.as_mut().unwrap();
        r.next_page(fs, 0);
        let _ = r.render(fs, &mut frame, settings);
    });
    m2(&mut sim, "Reader::prefetch_next()", |s| {
        let fs = s.env.fs();
        let r = s.ui.reader.as_mut().unwrap();
        r.prefetch_next(fs);
    });
    m2(&mut sim, "Reader::index_step() (one section)", |s| {
        let fs = s.env.fs();
        let lib = &mut s.ui.lib;
        let r = s.ui.reader.as_mut().unwrap();
        // Force a rebuild list entry by pretending a section is missing is not possible; time a no-op step.
        let _ = r.index_step(fs, lib);
    });
    // Stale starts after set_profile?
    let before_pages = {
        let r = s_reader(&mut sim);
        (r.section, r.page, r.section_pages(r.section))
    };
    sim.ui.settings.profile.size = 34;
    {
        let fs = sim.env.fs();
        let settings = &sim.ui.settings;
        let r = sim.ui.reader.as_mut().unwrap();
        r.set_profile(fs, settings);
    }
    let (sec, page_after, pages_est) = {
        let r = s_reader(&mut sim);
        (r.section, r.page, r.section_pages(r.section))
    };
    // Now force a real reload of the same section and compare the page count.
    {
        let fs = sim.env.fs();
        let r = sim.ui.reader.as_mut().unwrap();
        let loc = r.loc();
        r.goto_section(fs, sec + 1);
        r.goto(fs, loc);
    }
    let (page_reloaded, pages_real) = {
        let r = s_reader(&mut sim);
        (r.page, r.section_pages(r.section))
    };
    println!(
        "set_profile stale-index check: before size26 (section {}, page {}, pages {}) -> after size38 page {} pages(est/stale) {} -> after forced reload page {} pages {}",
        before_pages.0, before_pages.1, before_pages.2, page_after, pages_est, page_reloaded, pages_real
    );
    let _ = std::fs::remove_dir_all(&card);
}

fn s_reader(sim: &mut Sim) -> &mut quire_ui::Reader {
    sim.ui.reader.as_mut().unwrap()
}

#[test]
fn wordle_and_cursor_repeat() {
    for i in 0..3 {
        let t = Instant::now();
        let a = quire_ui::screens::games::wordle::answer_for(20_000 + i);
        println!("wordle::answer_for run {i}: {:.1} ms host ({a})", t.elapsed().as_micros() as f64 / 1000.0);
    }
    let t = Instant::now();
    let mut n = 0;
    for w in ["whale", "Ishmael", "the", "circumambulate", "purse", "spleen", "zebra", "aardvark", "xylophone", "of"] {
        n += quire_ui::screens::cursor::rarity(w);
    }
    println!("cursor::rarity x10: {:.2} ms host ({n})", t.elapsed().as_micros() as f64 / 1000.0);
}

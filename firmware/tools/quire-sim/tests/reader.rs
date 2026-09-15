//! Reader behaviour through the UI: the end of the book, Go to, bookmarks, type size,
//! positions across reboots, and the Spine's monotonic progress.

use quire_sim::{fixture_card, Sim};
use quire_ui::{Env, Key};

fn loc(sim: &Sim) -> quire_library::Loc {
    sim.ui.reader.as_ref().expect("a book is open").loc()
}

fn first_word(sim: &mut Sim) -> String {
    sim.ui.reader.as_mut().expect("a book is open").page_words().first().map(|(w, _)| w.clone()).unwrap_or_default()
}

#[test]
fn paging_past_the_last_page_shows_the_end_of_book() {
    let card = fixture_card("reader-end");
    let mut sim = Sim::boot(&card);
    sim.index_all();
    // Go to the last chapter, then page to the end.
    let n = sim.ui.reader.as_ref().unwrap().book.toc.len();
    let last = sim.ui.reader.as_ref().unwrap().book.toc[n - 1].title.clone();
    assert!(sim.goto_chapter(&last), "go to {last}");
    let mut turns = 0;
    while !sim.ui.reader.as_ref().unwrap().at_end() {
        sim.press(Key::Right);
        turns += 1;
        assert!(turns < 400, "the last chapter never ends");
        assert_eq!(sim.top(), "20-reading");
    }
    assert!(turns > 0, "the last chapter has more than one page");
    assert!(sim.frame().ink_count() > 200, "the last page draws");
    sim.press(Key::Right);
    assert_eq!(sim.top(), "2A-endofbook", "Right on the last page finishes the book");
    assert!(sim.frame().ink_count() > 500);
    let id = sim.ui.reader.as_ref().unwrap().id;
    assert_eq!(sim.ui.lib.get(id).unwrap().status, quire_library::Status::Finished);
    // Down on the side keys stays on the page at the end; Back returns to the page.
    sim.press(Key::Back);
    assert_eq!(sim.top(), "20-reading");
    sim.press(Key::Down);
    assert_eq!(sim.top(), "20-reading");
    assert!(sim.ui.reader.as_ref().unwrap().at_end());
    // The start of the book: Left and Up stay put without panicking.
    sim.ui.reader.as_mut().unwrap().goto_section(sim.env.fs(), 0);
    sim.ui.draw(&mut sim.env);
    sim.press(Key::Left);
    sim.press(Key::Up);
    assert!(sim.ui.reader.as_ref().unwrap().at_start());
    assert_eq!(sim.top(), "20-reading");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn go_to_percent_chapter_and_bookmark_land_where_they_say() {
    let card = fixture_card("reader-goto");
    let mut sim = Sim::boot(&card);
    sim.index_all();
    sim.goto_chapter("Loomings");
    // Percent: the row starts at the current percent; Right steps 1 %.
    let start = sim.ui.reader.as_mut().unwrap().info().permille / 10;
    sim.press(Key::Confirm);
    sim.press(Key::Right);
    assert_eq!(sim.top(), "23-goto");
    for _ in 0..40 {
        sim.press(Key::Right);
    }
    sim.press(Key::Confirm);
    assert_eq!(sim.top(), "20-reading");
    let want = start + 40;
    let got = sim.ui.reader.as_mut().unwrap().info().permille / 10;
    assert!(got.abs_diff(want) <= 1, "asked for {want}%, landed at {got}%");
    // Chapter: the stepper starts on the current chapter; Right moves to the next.
    let ch = sim.ui.reader.as_ref().unwrap().toc_index().expect("in a chapter");
    sim.press(Key::Confirm);
    sim.press(Key::Right);
    sim.press(Key::Down);
    sim.press(Key::Right);
    sim.press(Key::Confirm);
    assert_eq!(sim.top(), "20-reading");
    let r = sim.ui.reader.as_ref().unwrap();
    assert_eq!(r.toc_index(), Some(ch + 1), "next chapter");
    assert_eq!(r.loc(), r.book.toc_target(ch + 1).unwrap(), "at the chapter's start");
    // Bookmark: set one here, move far away, Go to → Bookmark returns to it.
    let here = loc(&sim);
    sim.press(Key::Confirm);
    sim.press(Key::Confirm);
    assert_eq!(sim.top(), "20-reading");
    assert!(sim.ui.reader.as_ref().unwrap().bookmarked(), "Confirm on the compass bookmarks the page");
    sim.goto_chapter("Loomings");
    assert_ne!(loc(&sim), here);
    sim.press(Key::Confirm);
    sim.press(Key::Right);
    sim.press(Key::Down);
    sim.press(Key::Down);
    sim.press(Key::Confirm);
    assert_eq!(sim.top(), "20-reading");
    assert_eq!(loc(&sim), here, "Go to → Bookmark lands on the bookmarked page");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn bookmarks_toggle_and_survive_a_reboot() {
    let card = fixture_card("reader-bookmarks");
    let mut sim = Sim::boot(&card);
    sim.index_all();
    sim.goto_chapter("Loomings");
    sim.press(Key::Right);
    let here = loc(&sim);
    assert!(!sim.ui.reader.as_ref().unwrap().bookmarked());
    // Long Confirm on the compass toggles in one.
    sim.press(Key::Confirm);
    sim.long(Key::Confirm);
    assert!(sim.ui.reader.as_ref().unwrap().bookmarked());
    sim.long(Key::Confirm);
    assert!(!sim.ui.reader.as_ref().unwrap().bookmarked(), "toggles off");
    sim.long(Key::Confirm);
    assert!(sim.ui.reader.as_ref().unwrap().bookmarked());
    sim.press(Key::Back);
    sim.press(Key::Power);
    drop(sim);
    let mut sim = Sim::boot(&card);
    assert_eq!(loc(&sim), here, "position after reboot");
    assert!(sim.ui.reader.as_ref().unwrap().bookmarked(), "bookmark after reboot");
    let n = sim.ui.reader.as_ref().unwrap().marks.items.len();
    assert_eq!(n, 1, "exactly one mark");
    sim.press(Key::Confirm);
    sim.press(Key::Confirm);
    assert!(!sim.ui.reader.as_ref().unwrap().bookmarked(), "Confirm on the compass toggles it off");
    sim.press(Key::Power);
    drop(sim);
    let sim = Sim::boot(&card);
    assert!(!sim.ui.reader.as_ref().unwrap().bookmarked(), "removal persists");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn type_size_changes_keep_the_reading_position() {
    let card = fixture_card("reader-type");
    let mut sim = Sim::boot(&card);
    sim.index_all();
    sim.goto_chapter("Loomings");
    for _ in 0..5 {
        sim.press(Key::Right);
    }
    let chars = loc(&sim).chars;
    let word = first_word(&mut sim);
    assert!(!word.is_empty());
    let size0 = sim.ui.settings.profile.size;
    // Type screen: Down to the size row, Right twice bigger, then Left twice back.
    sim.press(Key::Confirm);
    sim.press(Key::Up);
    assert_eq!(sim.top(), "25-type");
    for _ in 0..4 {
        sim.press(Key::Down);
    }
    sim.press(Key::Right);
    sim.press(Key::Right);
    assert!(sim.ui.settings.profile.size > size0, "bigger type");
    sim.press(Key::Back);
    assert_eq!(sim.top(), "20-reading");
    let after = loc(&sim).chars;
    let words: Vec<String> = sim.ui.reader.as_mut().unwrap().page_words().into_iter().map(|(w, _)| w).collect();
    assert!(after <= chars, "the page holding the old position: {after} <= {chars}");
    assert!(words.contains(&word), "the old first word {word:?} is on the new page (starts {:?})", words.first());
    assert_eq!(first_word(&mut sim), word, "same first word at the bigger size");
    sim.press(Key::Confirm);
    sim.press(Key::Up);
    for _ in 0..4 {
        sim.press(Key::Down);
    }
    sim.press(Key::Left);
    sim.press(Key::Left);
    assert_eq!(sim.ui.settings.profile.size, size0);
    sim.press(Key::Back);
    assert_eq!(first_word(&mut sim), word, "same first word back at the original size");
    assert_eq!(loc(&sim).chars, chars, "same position back at the original size");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn positions_survive_sleep_and_reboot() {
    let card = fixture_card("reader-position");
    let mut sim = Sim::boot(&card);
    sim.index_all();
    sim.goto_chapter("Loomings");
    for _ in 0..7 {
        sim.press(Key::Right);
    }
    let here = loc(&sim);
    let word = first_word(&mut sim);
    sim.press(Key::Power);
    assert_eq!(sim.top(), "40-sleep");
    drop(sim);
    let mut sim = Sim::boot(&card);
    assert_eq!(sim.top(), "20-reading");
    assert_eq!(loc(&sim), here, "same location after a reboot");
    assert_eq!(first_word(&mut sim), word, "same page after a reboot");
    // Without a sleep: the periodic save (every 30 s) catches the position too.
    sim.press(Key::Right);
    sim.press(Key::Right);
    let moved = loc(&sim);
    sim.env.now += 31;
    sim.event(quire_ui::Event::Tick);
    drop(sim);
    let sim = Sim::boot(&card);
    assert_eq!(loc(&sim), moved, "periodic save keeps the position");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn the_spine_is_monotonic_while_paging() {
    let card = fixture_card("reader-spine");
    let mut sim = Sim::boot(&card);
    sim.index_all();
    sim.goto_chapter("Loomings");
    let r = sim.ui.reader.as_ref().unwrap();
    let total = r.total_pages();
    let mut last = r.spine_model();
    let mut last_chars = r.loc().chars;
    let mut last_number = r.page_number();
    for i in 0..120 {
        sim.press(Key::Right);
        let r = sim.ui.reader.as_ref().unwrap();
        let m = r.spine_model();
        assert_eq!(m.total, total, "turn {i}: the total is stable once indexed");
        assert_eq!(m.current, last.current + 1, "turn {i}: one page forward");
        assert!(m.permille() >= last.permille(), "turn {i}: permille");
        assert!(m.current < m.total, "turn {i}: current within total");
        assert_eq!(r.page_number(), last_number + 1, "turn {i}: page number");
        assert!(r.loc().chars > last_chars, "turn {i}: characters advance");
        assert_eq!(m.chapters, last.chapters, "turn {i}: chapter notches are stable");
        last = m;
        last_chars = r.loc().chars;
        last_number = r.page_number();
    }
    for i in 0..120 {
        sim.press(Key::Left);
        let r = sim.ui.reader.as_ref().unwrap();
        let m = r.spine_model();
        assert_eq!(m.current + 1, last.current, "back {i}: one page back");
        assert!(r.loc().chars < last_chars, "back {i}: characters retreat");
        last = m;
        last_chars = r.loc().chars;
    }
    let _ = std::fs::remove_dir_all(&card);
}

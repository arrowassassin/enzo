//! The universal key grammar (brief §2) holds on every screen the tour reaches: long Back
//! is Jump from anywhere and Back closes it; Back pops every pushed screen down to the
//! book (pausing a game first, Right on the pause card quits); long Power is the power
//! menu; a Jump target opened from any screen returns there with Back.

use quire_sim::{fixture_card, tour_targets, tour_with, Sim};
use quire_ui::Key;

/// Screens where the grammar hands the keys to the screen itself.
fn owns_keys(top: &str) -> bool {
    top == "42-jump" || top.starts_with("40-sleep") || top == "44-picker-preview" || top == "45-locked" || top == "41-power"
}

/// Press Back until the reading page shows, pausing and quitting games on the way. A
/// screen may spend one Back cancelling something inside itself (a digit picker, a
/// focused field) but the next must pop it. Returns the number of presses.
fn back_to_reading(sim: &mut Sim, from: &str) -> usize {
    let mut n = 0;
    let mut stayed = 0;
    while sim.top() != "20-reading" {
        assert!(n < 24, "{from}: Back never reached the page, stuck at {:?}", sim.ui.stack_names());
        let top = sim.top();
        let depth = sim.ui.stack_names().len();
        sim.press(Key::Back);
        n += 1;
        if top.starts_with("80-") && top != "80-games" && top != "80-paused" && sim.top() == "80-paused" {
            sim.press(Key::Right);
            n += 1;
        }
        if sim.ui.stack_names().len() < depth {
            stayed = 0;
        } else {
            stayed += 1;
            assert!(stayed < 2, "{from}: Back on {top} left the stack at {:?}", sim.ui.stack_names());
        }
    }
    n
}

#[test]
fn long_back_opens_jump_and_back_closes_it_on_every_tour_screen() {
    let card = fixture_card("grammar-jump");
    let mut sim = Sim::boot(&card);
    let mut checked = 0;
    tour_with(&mut sim, &mut |sim, name, _| {
        if owns_keys(sim.top()) || name == "21-skim" {
            return;
        }
        let stack = sim.ui.stack_names();
        sim.long(Key::Back);
        assert_eq!(sim.top(), "42-jump", "long Back on {name} ({stack:?})");
        assert_eq!(sim.ui.stack_names().len(), stack.len() + 1, "Jump is pushed over {name}");
        sim.press(Key::Back);
        assert_eq!(sim.ui.stack_names(), stack, "Back closes Jump over {name}");
        // Long Power is the power menu everywhere; Back closes it.
        sim.long(Key::Power);
        assert_eq!(sim.top(), "41-power", "long Power on {name}");
        sim.press(Key::Back);
        assert_eq!(sim.ui.stack_names(), stack, "Back closes the power menu over {name}");
        checked += 1;
    });
    assert!(checked > 80, "{checked} screens checked");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn back_returns_to_the_book_from_every_tour_leaf() {
    let card = fixture_card("grammar-back");
    let mut sim = Sim::boot(&card);
    let mut leaves = 0;
    tour_with(&mut sim, &mut |sim, name, leaf| {
        if !leaf || owns_keys(sim.top()) {
            return;
        }
        back_to_reading(sim, name);
        leaves += 1;
    });
    assert!(leaves > 30, "{leaves} leaves checked");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn jump_to_any_target_and_back_returns_where_you_came_from() {
    let card = fixture_card("grammar-return");
    let mut sim = Sim::boot(&card);
    let targets: Vec<&'static str> = tour_targets().into_iter().map(|(n, _)| n).collect();
    let mut checked = 0;
    tour_with(&mut sim, &mut |sim, name, _| {
        if owns_keys(sim.top()) || name == "21-skim" {
            return;
        }
        let stack = sim.ui.stack_names();
        for &target in &targets {
            sim.jump_to(target);
            assert_eq!(sim.top(), target, "Jump from {name} to {target}");
            assert_eq!(sim.ui.stack_names().len(), stack.len() + 1, "{target} takes Jump's place over {name}");
            sim.press(Key::Back);
            if sim.top() == "80-paused" {
                sim.press(Key::Right);
            }
            assert_eq!(sim.ui.stack_names(), stack, "Back from {target} returns to {name}");
            checked += 1;
        }
    });
    assert!(checked > 1000, "{checked} round trips");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn every_tour_target_pops_back_to_the_book() {
    let card = fixture_card("grammar-targets");
    let mut sim = Sim::boot(&card);
    for (name, keys) in tour_targets() {
        sim.reset();
        sim.open(name);
        assert_eq!(sim.top(), name);
        for k in &keys {
            sim.press(*k);
        }
        let n = back_to_reading(&mut sim, name);
        assert!(n <= keys.len() * 2 + 4, "{name}: {n} presses to get back");
    }
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn power_sleeps_and_wakes_back_to_the_page_from_anywhere() {
    let card = fixture_card("grammar-power");
    let mut sim = Sim::boot(&card);
    for target in ["11-library", "50-settings", "80-sudoku", "78-calculator"] {
        sim.reset();
        sim.open(target);
        let before = sim.env.requests.iter().filter(|r| matches!(r, quire_ui::SysRequest::Sleep)).count();
        sim.press(Key::Power);
        assert_eq!(sim.top(), "40-sleep", "Power sleeps from {target}");
        let after = sim.env.requests.iter().filter(|r| matches!(r, quire_ui::SysRequest::Sleep)).count();
        assert_eq!(after, before + 1, "one sleep request from {target}");
        // Keys while asleep never stack screens; Power wakes.
        let depth = sim.ui.stack_names().len();
        sim.press(Key::Confirm);
        sim.long(Key::Back);
        sim.long(Key::Power);
        assert_eq!(sim.ui.stack_names().len(), depth, "asleep: {:?}", sim.ui.stack_names());
        sim.press(Key::Power);
        assert_eq!(sim.top(), target, "Power wakes back to {target}");
        assert!(!sim.ui.asleep, "awake after the sleep screen popped");
        // Sleeping again asks the platform again.
        sim.press(Key::Power);
        let again = sim.env.requests.iter().filter(|r| matches!(r, quire_ui::SysRequest::Sleep)).count();
        assert_eq!(again, after + 1, "second sleep from {target} is requested");
        sim.event(quire_ui::Event::Wake);
        assert_eq!(sim.top(), target, "Wake returns to {target}");
    }
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn locked_keys_show_one_strip_and_hold_power_unlocks() {
    let card = fixture_card("grammar-lock");
    let mut sim = Sim::boot(&card);
    sim.long(Key::Power);
    sim.press(Key::Down);
    assert!(sim.ui.locked, "Down on the power menu locks the keys");
    let depth = sim.ui.stack_names().len();
    for k in [Key::Right, Key::Confirm, Key::Left, Key::Up] {
        sim.press(k);
        assert_eq!(sim.top(), "45-locked");
        assert!(sim.ui.stack_names().len() <= depth + 1, "{:?}", sim.ui.stack_names());
    }
    sim.event(quire_ui::Event::Tick);
    assert_ne!(sim.top(), "45-locked", "the strip leaves on the next tick");
    sim.long(Key::Power);
    assert!(!sim.ui.locked, "holding Power unlocks");
    let _ = std::fs::remove_dir_all(&card);
}

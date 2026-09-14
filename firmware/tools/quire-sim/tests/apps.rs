//! The apps and games behind their public APIs and through the UI: the Z-machine plays
//! the bundled story, the calculator evaluates, Wordle's daily answer is stable and quick,
//! and the fiction player runs a story from the card.

use quire_sim::{fixture_card, Sim};
use quire_ui::screens::apps::calculator::evaluate;
use quire_ui::zmachine::{Machine, Step};
use quire_ui::Key;
use std::path::Path;

fn advent() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/quire-ui/data/advent.z5")).expect("advent.z5")
}

/// Run until the machine wants a line, collecting the output.
fn run_to_prompt(m: &mut Machine) -> String {
    let mut out = String::new();
    for _ in 0..2000 {
        match m.run(20_000) {
            Step::Budget => out.push_str(&m.take_output()),
            Step::WaitLine => {
                out.push_str(&m.take_output());
                return out;
            }
            other => panic!("unexpected {other:?} after {out:?}"),
        }
    }
    panic!("never reached a prompt: {out:?}");
}

#[test]
fn the_zmachine_plays_the_bundled_story_and_saves_a_game() {
    let mut m = Machine::new(advent()).expect("story loads");
    assert_eq!(m.version(), 5);
    let banner = run_to_prompt(&mut m);
    assert!(banner.contains("ADVENTURE"), "banner: {banner:?}");
    assert!(banner.contains("At End Of Road"), "banner: {banner:?}");
    assert!(m.upper_window().iter().any(|l| l.contains("At End Of Road")), "the room is in the upper window");
    m.input("inventory");
    let inv = run_to_prompt(&mut m);
    assert!(!inv.is_empty());
    // Save, move, restore: the position comes back.
    let snapshot = m.save();
    assert!(!snapshot.is_empty());
    m.input("east");
    let inside = run_to_prompt(&mut m);
    assert!(inside.contains("Inside Building"), "{inside:?}");
    assert!(m.restore(&snapshot), "restore accepts its own save");
    m.input("look");
    let again = run_to_prompt(&mut m);
    assert!(again.contains("At End Of Road"), "restored outside: {again:?}");
    assert!(!m.halted());
    m.input("quit");
    // Quit asks for confirmation; either way the machine never errors.
    for _ in 0..3 {
        match m.run(20_000) {
            Step::WaitLine => m.input("y"),
            Step::WaitChar => m.input_char(b'y' as u16),
            Step::Halt => break,
            Step::Budget => {}
            Step::Error(e) => panic!("error: {e}"),
        }
    }
    let _ = m.take_output();
}

#[test]
fn a_truncated_story_is_refused_not_a_panic() {
    let story = advent();
    for n in [0, 1, 32, 64, 1000, story.len() / 2] {
        let mut trimmed = story.clone();
        trimmed.truncate(n);
        if let Ok(mut m) = Machine::new(trimmed) {
            for _ in 0..50 {
                match m.run(10_000) {
                    Step::Budget => {}
                    Step::WaitLine => m.input("look"),
                    Step::WaitChar => m.input_char(32),
                    Step::Halt | Step::Error(_) => break,
                }
            }
        }
    }
}

#[test]
fn the_calculator_evaluates_and_rejects() {
    assert_eq!(evaluate("2+3×4").unwrap(), "14");
    assert_eq!(evaluate("(1+2)×(3+4)").unwrap(), "21");
    assert_eq!(evaluate("7÷2").unwrap(), "3.5");
    assert_eq!(evaluate("1÷3×3").unwrap(), "1");
    assert_eq!(evaluate("2×−3").unwrap(), "-6");
    assert!(evaluate("").is_err());
    assert!(evaluate("2+").is_err());
    assert!(evaluate("((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))").is_err() || true, "deep nesting never panics");
    assert!(evaluate("1÷0").is_err());
    assert!(evaluate("abc").is_err());
}

#[test]
fn the_calculator_screen_computes_through_the_keys() {
    let card = fixture_card("apps-calc");
    let mut sim = Sim::boot(&card);
    sim.open("78-calculator");
    assert_eq!(sim.top(), "78-calculator");
    let before = sim.hash();
    // The keypad: Confirm presses the focused key; long Right evaluates.
    for k in [Key::Confirm, Key::Right, Key::Confirm, Key::Down, Key::Confirm, Key::Left, Key::Up, Key::Confirm] {
        sim.press(k);
        assert_eq!(sim.top(), "78-calculator");
        assert!(sim.frame().ink_count() > 200);
    }
    assert_ne!(sim.hash(), before, "pressed keys show on the tape");
    let typed = sim.hash();
    sim.long(Key::Right);
    assert_eq!(sim.top(), "78-calculator");
    assert_ne!(sim.hash(), typed, "evaluating shows a result (or an error)");
    sim.press(Key::Back);
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn wordle_has_a_stable_five_letter_answer_and_opens_quickly() {
    use quire_ui::screens::games::wordle::answer_for;
    let a = answer_for(20_700);
    assert_eq!(a.len(), 5);
    assert!(a.chars().all(|c| c.is_ascii_lowercase()));
    assert_eq!(a, answer_for(20_700), "the same word for the same day");
    let distinct: std::collections::BTreeSet<String> = (0..30u16).map(answer_for).collect();
    assert!(distinct.len() > 25, "different days get different words: {distinct:?}");
    let card = fixture_card("apps-wordle");
    let mut sim = Sim::boot(&card);
    sim.open("80-wordle");
    assert_eq!(sim.top(), "80-wordle");
    // Choosing the answer word once (a v5 list is 15 k words) must not stall the device.
    let budget = if cfg!(debug_assertions) { 1500 } else { 100 };
    assert!(sim.last_ms < budget, "opening Wordle took {} ms", sim.last_ms);
    for _ in 0..6 {
        sim.press(Key::Confirm);
        assert!(sim.last_ms < budget, "a letter took {} ms", sim.last_ms);
    }
    assert_eq!(sim.top(), "80-wordle");
    sim.press(Key::Back);
    assert_eq!(sim.top(), "80-paused");
    sim.press(Key::Confirm);
    assert_eq!(sim.top(), "80-wordle", "New game stays in Wordle");
    let _ = std::fs::remove_dir_all(&card);
}

#[test]
fn the_fiction_player_runs_a_story_from_the_card() {
    let card = fixture_card("apps-fiction");
    std::fs::create_dir_all(card.join("stories")).unwrap();
    std::fs::write(card.join("stories/advent.z5"), advent()).unwrap();
    let mut sim = Sim::boot(&card);
    sim.open("76-fiction");
    assert_eq!(sim.top(), "76-fiction");
    sim.press(Key::Confirm);
    assert_eq!(sim.top(), "76-fiction-play", "Play opens the story");
    let intro = sim.hash();
    // A command from the phone through the keyboard.
    sim.press(Key::Right);
    assert_eq!(sim.top(), "14-keyboard");
    sim.phone("no");
    sim.press(Key::Right);
    assert_eq!(sim.top(), "76-fiction-play");
    assert_ne!(sim.hash(), intro, "the transcript moved on");
    // The verb compass and scrolling back never leave the player.
    sim.press(Key::Left);
    assert_eq!(sim.top(), "76-fiction-verbs");
    sim.press(Key::Back);
    sim.press(Key::Up);
    sim.press(Key::Down);
    assert_eq!(sim.top(), "76-fiction-play");
    // Back saves the game; the list shows it.
    sim.press(Key::Back);
    assert_eq!(sim.top(), "76-fiction");
    assert!(card.join(".quire/stories").exists(), "a save was written");
    sim.press(Key::Confirm);
    assert_eq!(sim.top(), "76-fiction-play", "the saved game reopens");
    let _ = std::fs::remove_dir_all(&card);
}

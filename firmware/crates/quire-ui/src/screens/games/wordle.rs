//! Wordle: five letters, six tries, 72 px tiles, a T9 keyboard with state marks.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{Frame, Ink, Pattern, Rect, TextStyle};

use super::{board_x, Paused};
use crate::text::{centered_baseline, draw_centered};
use crate::widgets::{self, rail, running_head};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

use crate::screens::cursor::words::{WORDLE_ACCEPTED, WORDLE_ANSWERS};

const TILE: i32 = 72;
const ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];

/// Letter state on the keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mark {
    Unknown,
    Absent,
    Present,
    Correct,
}

/// The Wordle game.
pub struct Wordle {
    answer: String,
    guesses: Vec<String>,
    current: String,
    marks: [Mark; 26],
    focus: (usize, usize),
    message: String,
    day: u16,
    started: bool,
}

/// The answer for a day: a common 5-letter word from the frequency list that the
/// Wordle list accepts (a build-time table, so this is one index — no scan).
pub fn answer_for(day: u16) -> String {
    // Scramble the day so consecutive days are not frequency neighbours.
    let i = (day as u32).wrapping_mul(2654435761) as usize % WORDLE_ANSWERS.len();
    String::from_utf8_lossy(&WORDLE_ANSWERS[i]).into_owned()
}

/// Whether a guess is in the accepted list (a binary search over the packed table).
fn is_word(w: &str) -> bool {
    let b = w.as_bytes();
    if b.len() != 5 || !b.iter().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    let mut key = [0u8; 5];
    key.copy_from_slice(b);
    WORDLE_ACCEPTED.binary_search(&key).is_ok()
}

impl Wordle {
    /// New.
    pub fn new() -> Self {
        Wordle {
            answer: String::new(),
            guesses: Vec::new(),
            current: String::new(),
            marks: [Mark::Unknown; 26],
            focus: (0, 0),
            message: String::new(),
            day: 0,
            started: false,
        }
    }
    fn start(&mut self, day: u16) {
        self.day = day;
        self.answer = answer_for(day);
        self.guesses.clear();
        self.current.clear();
        self.marks = [Mark::Unknown; 26];
        self.message.clear();
        self.started = true;
    }
    fn score(&self, guess: &str) -> [Mark; 5] {
        let a: Vec<char> = self.answer.chars().collect();
        let g: Vec<char> = guess.chars().collect();
        let mut out = [Mark::Absent; 5];
        let mut used = [false; 5];
        for i in 0..5 {
            if g[i] == a[i] {
                out[i] = Mark::Correct;
                used[i] = true;
            }
        }
        for i in 0..5 {
            if out[i] == Mark::Correct {
                continue;
            }
            if let Some(j) = (0..5).find(|j| !used[*j] && a[*j] == g[i]) {
                used[j] = true;
                out[i] = Mark::Present;
            }
        }
        out
    }
    fn won(&self) -> bool {
        self.guesses.last().map(|g| *g == self.answer).unwrap_or(false)
    }
    fn over(&self) -> bool {
        self.won() || self.guesses.len() >= 6
    }
    fn submit(&mut self) {
        if self.current.len() != 5 {
            self.message = String::from("Five letters");
            return;
        }
        if !is_word(&self.current) {
            self.message = String::from("Not in the word list");
            return;
        }
        let g = core::mem::take(&mut self.current);
        let s = self.score(&g);
        for (i, c) in g.chars().enumerate() {
            let k = (c as u8 - b'a') as usize;
            let m = s[i];
            let cur = self.marks[k];
            self.marks[k] = match (cur, m) {
                (Mark::Correct, _) => Mark::Correct,
                (_, Mark::Correct) => Mark::Correct,
                (Mark::Present, _) => Mark::Present,
                (_, Mark::Present) => Mark::Present,
                (_, Mark::Absent) => Mark::Absent,
                _ => cur,
            };
        }
        self.guesses.push(g);
        self.message = if self.won() {
            String::from(["Genius", "Magnificent", "Impressive", "Splendid", "Great", "Phew"][self.guesses.len() - 1])
        } else if self.guesses.len() >= 6 {
            alloc::format!("It was {}", self.answer.to_uppercase())
        } else {
            String::new()
        };
    }
}

impl Default for Wordle {
    fn default() -> Self {
        Self::new()
    }
}

fn tile(f: &mut Frame, r: Rect, c: char, m: Mark) {
    let font = quire_fonts::ui::title();
    match m {
        Mark::Correct => f.fill_rect(r, Ink::Black),
        Mark::Present => {
            f.pattern_rect(r, Pattern::Dots50);
            f.fill_rect(r.inset(10), Ink::White);
        }
        _ => {}
    }
    f.stroke_rect(r, 2, Ink::Black);
    if c != ' ' {
        draw_centered(
            f,
            font,
            r.x + r.w as i32 / 2,
            centered_baseline(font, r.y, r.h as i32),
            &c.to_uppercase().collect::<String>(),
            TextStyle { inverted: m == Mark::Correct, ..TextStyle::INK },
        );
    }
}

impl<E: Env> Screen<E> for Wordle {
    fn name(&self) -> &'static str {
        "80-wordle"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let day = cx.today();
        if !self.started || self.day != day {
            self.start(day);
        }
        running_head(f, "Wordle", Some(&alloc::format!("{} / 6", self.guesses.len())));
        let gap = 6;
        let bw = 5 * TILE + 4 * gap;
        let bx = board_x(f, bw);
        let by = widgets::CONTENT_TOP;
        for row in 0..6 {
            let y = by + row as i32 * (TILE + gap);
            let (word, marks): (Vec<char>, [Mark; 5]) = if row < self.guesses.len() {
                (self.guesses[row].chars().collect(), self.score(&self.guesses[row]))
            } else if row == self.guesses.len() {
                let mut w: Vec<char> = self.current.chars().collect();
                w.resize(5, ' ');
                (w, [Mark::Unknown; 5])
            } else {
                (alloc::vec![' '; 5], [Mark::Unknown; 5])
            };
            for i in 0..5 {
                tile(f, Rect::new(bx + i as i32 * (TILE + gap), y, TILE as u32, TILE as u32), word[i], marks[i]);
            }
        }
        let ky = by + 6 * (TILE + gap) + 8;
        let fl = quire_fonts::ui::label();
        if !self.message.is_empty() {
            draw_centered(f, fl, f.width() as i32 / 2, ky + fl.ascent(), &self.message, TextStyle::INK);
        }
        // Keyboard with state marks.
        let kw = 42;
        let kh = 40;
        let ky = ky + 24;
        let font = quire_fonts::ui::body();
        for (r, row) in ROWS.iter().enumerate() {
            let n = row.len() as i32;
            let x0 = (f.width() as i32 - n * kw) / 2;
            for (c, ch) in row.chars().enumerate() {
                let rect = Rect::new(x0 + c as i32 * kw, ky + r as i32 * kh, kw as u32, kh as u32);
                let m = self.marks[(ch as u8 - b'a') as usize];
                let focused = self.focus == (r, c) && !self.over();
                match m {
                    Mark::Correct => f.fill_rect(rect.inset(2), Ink::Black),
                    Mark::Present => f.pattern_rect(rect.inset(2), Pattern::Dots50),
                    Mark::Absent => f.pattern_rect(rect.inset(2), Pattern::Sparse),
                    Mark::Unknown => {}
                }
                if focused {
                    f.fill_rect(rect.inset(2), Ink::Black);
                }
                let inv = focused || m == Mark::Correct;
                if m == Mark::Present && !focused {
                    f.fill_rect(Rect::new(rect.x + kw / 2 - 9, rect.y + 8, 18, (kh - 16) as u32), Ink::White);
                }
                draw_centered(
                    f,
                    font,
                    rect.x + kw / 2,
                    centered_baseline(font, rect.y, kh),
                    &ch.to_uppercase().collect::<String>(),
                    TextStyle { inverted: inv, ..TextStyle::INK },
                );
            }
        }
        if self.over() {
            rail(f, ["", "Back", "", "Share"], None);
        } else {
            rail(f, ["Delete", "Pause", "Letter", "Enter"], None);
        }
        Refresh::Du
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if self.over() {
            return match ev.key {
                Key::Back => Action::Pop,
                Key::Right => {
                    // Share: the classic grid as text into the message.
                    let mut s = alloc::format!("Quire Wordle {} {}/6\n", self.day, self.guesses.len());
                    for g in &self.guesses {
                        for m in self.score(g) {
                            s.push(match m {
                                Mark::Correct => '■',
                                Mark::Present => '▣',
                                _ => '□',
                            });
                        }
                        s.push('\n');
                    }
                    self.message = s.replace('\n', " ");
                    Action::Redraw
                }
                _ => Action::None,
            };
        }
        match (ev.key, ev.kind) {
            (Key::Back, KeyKind::Press) => Action::Push(Paused::new("Wordle")),
            (Key::Left, KeyKind::Long) => {
                self.current.pop();
                Action::Redraw
            }
            (Key::Right, KeyKind::Long) => {
                self.submit();
                Action::Redraw
            }
            (Key::Left, _) => {
                let n = ROWS[self.focus.0].len();
                self.focus.1 = (self.focus.1 + n - 1) % n;
                Action::Redraw
            }
            (Key::Right, _) => {
                let n = ROWS[self.focus.0].len();
                self.focus.1 = (self.focus.1 + 1) % n;
                Action::Redraw
            }
            (Key::Up, _) => {
                self.focus.0 = (self.focus.0 + 2) % 3;
                self.focus.1 = self.focus.1.min(ROWS[self.focus.0].len() - 1);
                Action::Redraw
            }
            (Key::Down, _) => {
                self.focus.0 = (self.focus.0 + 1) % 3;
                self.focus.1 = self.focus.1.min(ROWS[self.focus.0].len() - 1);
                Action::Redraw
            }
            (Key::Confirm, KeyKind::Long) => {
                self.submit();
                Action::Redraw
            }
            (Key::Confirm, KeyKind::Press) => {
                if self.current.len() < 5 {
                    let ch = ROWS[self.focus.0].chars().nth(self.focus.1).unwrap_or('a');
                    self.current.push(ch);
                    self.message.clear();
                } else {
                    self.submit();
                }
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        match r {
            Result_::Choice(1) => {
                // A new game: a different day's word (the next one) so the daily stays.
                self.start(cx.today().wrapping_add(1000 + self.guesses.len() as u16));
                Action::Redraw
            }
            Result_::Choice(2) => Action::Pop,
            _ => Action::Redraw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring_handles_repeats() {
        let mut w = Wordle::new();
        w.answer = String::from("apple");
        let s = w.score("paper");
        assert_eq!(s, [Mark::Present, Mark::Present, Mark::Correct, Mark::Present, Mark::Absent]);
        assert!(is_word("apple"));
        assert_eq!(answer_for(5).len(), 5);
        assert_ne!(answer_for(5), answer_for(6));
    }
}

//! Sudoku: 52 px cells, pencil marks at 14 px, three difficulties.

use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};

use super::{board_x, Paused, Rng};
use crate::text::{centered_baseline, draw_centered};
use crate::widgets::{rail, running_head};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

const CELL: i32 = 52;

/// The Sudoku game.
pub struct Sudoku {
    given: [u8; 81],
    grid: [u8; 81],
    marks: [u16; 81],
    cursor: usize,
    difficulty: u8,
    rng: Rng,
    started: bool,
    /// Entering a number: the digit picker is open.
    pick: Option<u8>,
    pencil: bool,
}

impl Sudoku {
    /// New.
    pub fn new() -> Self {
        Sudoku {
            given: [0; 81],
            grid: [0; 81],
            marks: [0; 81],
            cursor: 40,
            difficulty: 1,
            rng: Rng(0x9E37),
            started: false,
            pick: None,
            pencil: false,
        }
    }
    fn generate(&mut self) {
        let mut g = [0u8; 81];
        fill(&mut g, 0, &mut self.rng);
        let remove = match self.difficulty {
            0 => 36,
            1 => 46,
            _ => 54,
        };
        let mut given = g;
        let mut removed = 0;
        let mut tries = 0;
        while removed < remove && tries < 400 {
            tries += 1;
            let i = self.rng.below(81) as usize;
            if given[i] == 0 {
                continue;
            }
            let v = given[i];
            given[i] = 0;
            let mut probe = given;
            if count_solutions(&mut probe, 0, 2) == 1 {
                removed += 1;
            } else {
                given[i] = v;
            }
        }
        self.given = given;
        self.grid = given;
        self.marks = [0; 81];
        self.started = true;
    }
    fn solved(&self) -> bool {
        self.grid.iter().all(|v| *v != 0) && (0..81).all(|i| valid(&self.grid, i, self.grid[i]))
    }
}

impl Default for Sudoku {
    fn default() -> Self {
        Self::new()
    }
}

fn valid(g: &[u8; 81], i: usize, v: u8) -> bool {
    let (r, c) = (i / 9, i % 9);
    for k in 0..9 {
        let rr = r * 9 + k;
        let cc = k * 9 + c;
        if rr != i && g[rr] == v {
            return false;
        }
        if cc != i && g[cc] == v {
            return false;
        }
    }
    let (br, bc) = (r / 3 * 3, c / 3 * 3);
    for dr in 0..3 {
        for dc in 0..3 {
            let j = (br + dr) * 9 + bc + dc;
            if j != i && g[j] == v {
                return false;
            }
        }
    }
    true
}

fn fill(g: &mut [u8; 81], i: usize, rng: &mut Rng) -> bool {
    if i == 81 {
        return true;
    }
    let mut digits = [1u8, 2, 3, 4, 5, 6, 7, 8, 9];
    for k in (1..9).rev() {
        let j = rng.below(k as u32 + 1) as usize;
        digits.swap(k, j);
    }
    for d in digits {
        if valid(g, i, d) {
            g[i] = d;
            if fill(g, i + 1, rng) {
                return true;
            }
            g[i] = 0;
        }
    }
    false
}

fn count_solutions(g: &mut [u8; 81], start: usize, cap: u32) -> u32 {
    let Some(i) = (start..81).find(|k| g[*k] == 0) else { return 1 };
    let mut n = 0;
    for d in 1..=9u8 {
        if valid(g, i, d) {
            g[i] = d;
            n += count_solutions(g, i + 1, cap - n);
            g[i] = 0;
            if n >= cap {
                break;
            }
        }
    }
    n
}

impl<E: Env> Screen<E> for Sudoku {
    fn name(&self) -> &'static str {
        "80-sudoku"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.started {
            self.rng = Rng(cx.env.random() | 1);
            self.generate();
        }
        let names = ["Easy", "Medium", "Hard"];
        running_head(f, "Sudoku", Some(names[self.difficulty as usize]));
        let bx = board_x(f, 9 * CELL);
        let by = widgets_top();
        let font = quire_fonts::ui::list_title();
        let small = quire_fonts::ui::label();
        for i in 0..81 {
            let (r, c) = (i / 9, i % 9);
            let cell = Rect::new(bx + c as i32 * CELL, by + r as i32 * CELL, CELL as u32, CELL as u32);
            let focused = i == self.cursor;
            if focused {
                f.fill_rect(cell, Ink::Black);
            }
            let v = self.grid[i];
            let style = TextStyle { inverted: focused, darker: self.given[i] != 0, ..TextStyle::INK };
            if v != 0 {
                let ok = valid(&self.grid, i, v);
                draw_centered(f, font, cell.x + CELL / 2, centered_baseline(font, cell.y, CELL), &alloc::format!("{v}"), style);
                if !ok {
                    f.fill_rect(
                        Rect::new(cell.x + 8, cell.bottom() - 6, (CELL - 16) as u32, 2),
                        if focused { Ink::White } else { Ink::Black },
                    );
                }
            } else if self.marks[i] != 0 {
                for d in 1..=9u16 {
                    if self.marks[i] & (1 << d) != 0 {
                        let (mr, mc) = ((d as i32 - 1) / 3, (d as i32 - 1) % 3);
                        draw_text(f, small, cell.x + 5 + mc * 15, cell.y + 14 + mr * 15, &alloc::format!("{d}"), style);
                    }
                }
            }
        }
        for k in 0u32..=9 {
            let t: i32 = if k.is_multiple_of(3) { 3 } else { 1 };
            let k = k as i32;
            f.fill_rect(Rect::new(bx + k * CELL - t / 2, by - 1, t as u32, (9 * CELL + 2) as u32), Ink::Black);
            f.fill_rect(Rect::new(bx - 1, by + k * CELL - t / 2, (9 * CELL + 2) as u32, t as u32), Ink::Black);
        }
        // Digit picker beneath the board.
        let py = by + 9 * CELL + 20;
        if let Some(p) = self.pick {
            for d in 1..=9u8 {
                let r = Rect::new(bx + (d as i32 - 1) * CELL, py, CELL as u32, 44);
                let focused = d == p;
                if focused {
                    f.fill_rect(r, Ink::Black);
                }
                f.stroke_rect(r, 1, Ink::Black);
                draw_centered(
                    f,
                    font,
                    r.x + CELL / 2,
                    centered_baseline(font, r.y, 44),
                    &alloc::format!("{d}"),
                    TextStyle { inverted: focused, ..TextStyle::INK },
                );
            }
            rail(f, ["", "Cancel", if self.pencil { "Pencil" } else { "Enter" }, ""], None);
        } else if self.solved() {
            draw_centered(f, quire_fonts::ui::title(), f.width() as i32 / 2, py + 30, "Solved", TextStyle::INK);
            rail(f, ["", "Back", "New", ""], None);
        } else {
            let fl = quire_fonts::ui::label();
            draw_text(f, fl, bx, py + fl.ascent(), "Confirm enters · hold Confirm pencils · Left clears", TextStyle::INK);
            rail(f, ["Clear", "Pause", "Enter", "Pencil"], None);
        }
        Refresh::Du
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if let Some(p) = self.pick {
            match ev.key {
                Key::Left => self.pick = Some(if p == 1 { 9 } else { p - 1 }),
                Key::Right => self.pick = Some(if p == 9 { 1 } else { p + 1 }),
                Key::Up => self.pick = Some(if p > 3 { p - 3 } else { p + 6 }),
                Key::Down => self.pick = Some(if p <= 6 { p + 3 } else { p - 6 }),
                Key::Back => self.pick = None,
                Key::Confirm => {
                    if self.given[self.cursor] == 0 {
                        if self.pencil {
                            self.marks[self.cursor] ^= 1 << p;
                        } else {
                            self.grid[self.cursor] = p;
                            self.marks[self.cursor] = 0;
                        }
                    }
                    self.pick = None;
                }
                Key::Power => {}
            }
            return Action::Redraw;
        }
        match (ev.key, ev.kind) {
            (Key::Back, KeyKind::Press) => Action::Push(Paused::new("Sudoku")),
            (Key::Confirm, KeyKind::Press) => {
                if self.solved() {
                    self.started = false;
                    return Action::Redraw;
                }
                if self.given[self.cursor] == 0 {
                    self.pencil = false;
                    self.pick = Some(self.grid[self.cursor].max(1));
                }
                Action::Redraw
            }
            (Key::Confirm, KeyKind::Long) => {
                if self.given[self.cursor] == 0 {
                    self.pencil = true;
                    self.pick = Some(5);
                }
                Action::Redraw
            }
            (Key::Left, KeyKind::Long) | (Key::Right, KeyKind::Long) => {
                if self.given[self.cursor] == 0 {
                    self.grid[self.cursor] = 0;
                    self.marks[self.cursor] = 0;
                }
                Action::Redraw
            }
            (Key::Left, _) => {
                self.cursor = if self.cursor.is_multiple_of(9) { self.cursor + 8 } else { self.cursor - 1 };
                Action::Redraw
            }
            (Key::Right, _) => {
                self.cursor = if self.cursor % 9 == 8 { self.cursor - 8 } else { self.cursor + 1 };
                Action::Redraw
            }
            (Key::Up, _) => {
                self.cursor = (self.cursor + 72) % 81;
                Action::Redraw
            }
            (Key::Down, _) => {
                self.cursor = (self.cursor + 9) % 81;
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn result(&mut self, _cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        match r {
            Result_::Choice(1) => {
                self.difficulty = (self.difficulty + 1) % 3;
                self.started = false;
                Action::Redraw
            }
            Result_::Choice(2) => Action::Pop,
            _ => Action::Redraw,
        }
    }
}

fn widgets_top() -> i32 {
    crate::widgets::CONTENT_TOP + 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_unique_puzzles() {
        let mut s = Sudoku::new();
        s.rng = Rng(12345);
        s.difficulty = 2;
        s.generate();
        let mut probe = s.given;
        assert_eq!(count_solutions(&mut probe, 0, 2), 1);
        assert!(s.given.iter().filter(|v| **v == 0).count() >= 40);
    }
}

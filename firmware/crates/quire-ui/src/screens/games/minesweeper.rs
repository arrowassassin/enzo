//! Minesweeper: 9 × 9, ten mines, 30 px cells, first press is safe.

use quire_gfx::{Frame, Ink, Pattern, Rect, TextStyle};

use super::{board_x, Paused, Rng};
use crate::icons::{self, Icon};
use crate::text::{centered_baseline, draw_centered};
use crate::widgets::{self, rail, running_head};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

const N: usize = 9;
const MINES: usize = 10;
const CELL: i32 = 48;

/// The Minesweeper game.
pub struct Minesweeper {
    mines: [bool; N * N],
    open: [bool; N * N],
    flag: [bool; N * N],
    cursor: usize,
    placed: bool,
    dead: bool,
    won: bool,
    rng: Rng,
    started: bool,
}

impl Minesweeper {
    /// New.
    pub fn new() -> Self {
        Minesweeper {
            mines: [false; N * N],
            open: [false; N * N],
            flag: [false; N * N],
            cursor: 40,
            placed: false,
            dead: false,
            won: false,
            rng: Rng(0x11),
            started: false,
        }
    }
    fn reset(&mut self) {
        self.mines = [false; N * N];
        self.open = [false; N * N];
        self.flag = [false; N * N];
        self.placed = false;
        self.dead = false;
        self.won = false;
        self.started = true;
    }
    fn place(&mut self, avoid: usize) {
        let mut n = 0;
        while n < MINES {
            let i = self.rng.below((N * N) as u32) as usize;
            if i == avoid || self.mines[i] || neighbours(avoid).contains(&i) {
                continue;
            }
            self.mines[i] = true;
            n += 1;
        }
        self.placed = true;
    }
    fn count(&self, i: usize) -> u8 {
        neighbours(i).iter().filter(|j| self.mines[**j]).count() as u8
    }
    fn reveal(&mut self, i: usize) {
        if self.open[i] || self.flag[i] {
            return;
        }
        let mut stack = alloc::vec![i];
        while let Some(k) = stack.pop() {
            if self.open[k] || self.flag[k] {
                continue;
            }
            self.open[k] = true;
            if self.mines[k] {
                self.dead = true;
                return;
            }
            if self.count(k) == 0 {
                for j in neighbours(k) {
                    if !self.open[j] {
                        stack.push(j);
                    }
                }
            }
        }
        self.won = (0..N * N).all(|k| self.open[k] || self.mines[k]);
    }
}

fn neighbours(i: usize) -> alloc::vec::Vec<usize> {
    let (r, c) = ((i / N) as i32, (i % N) as i32);
    let mut v = alloc::vec::Vec::with_capacity(8);
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let (rr, cc) = (r + dr, c + dc);
            if rr >= 0 && rr < N as i32 && cc >= 0 && cc < N as i32 {
                v.push(rr as usize * N + cc as usize);
            }
        }
    }
    v
}

impl Default for Minesweeper {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Minesweeper {
    fn name(&self) -> &'static str {
        "80-minesweeper"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.started {
            self.rng = Rng(cx.env.random() | 1);
            self.reset();
        }
        let flags = self.flag.iter().filter(|x| **x).count();
        running_head(f, "Minesweeper", Some(&alloc::format!("{} mines left", MINES.saturating_sub(flags))));
        let bx = board_x(f, N as i32 * CELL);
        let by = widgets::CONTENT_TOP + 8;
        let font = quire_fonts::ui::list_title();
        for i in 0..N * N {
            let (r, c) = (i / N, i % N);
            let rect = Rect::new(bx + c as i32 * CELL, by + r as i32 * CELL, CELL as u32, CELL as u32);
            let focused = i == self.cursor;
            if self.open[i] {
                if self.mines[i] {
                    f.fill_rect(rect, Ink::Black);
                    draw_centered(f, font, rect.x + CELL / 2, centered_baseline(font, rect.y, CELL), "✸", TextStyle::PAPER);
                } else {
                    let n = self.count(i);
                    if n > 0 {
                        draw_centered(
                            f,
                            font,
                            rect.x + CELL / 2,
                            centered_baseline(font, rect.y, CELL),
                            &alloc::format!("{n}"),
                            TextStyle::INK,
                        );
                    }
                }
            } else {
                f.pattern_rect(rect, Pattern::Dots25);
                if self.flag[i] {
                    f.fill_rect(rect.inset(8), Ink::White);
                    icons::draw(f, Icon::Warning, rect.x + CELL / 2 - 12, rect.y + CELL / 2 - 12, Ink::Black);
                }
                if self.dead && self.mines[i] && !self.flag[i] {
                    draw_centered(f, font, rect.x + CELL / 2, centered_baseline(font, rect.y, CELL), "✸", TextStyle::INK);
                }
            }
            f.stroke_rect(rect, 1, Ink::Black);
            if focused {
                f.stroke_rect(rect, 3, Ink::Black);
                f.invert_rect(rect.inset(3));
            }
        }
        let y = by + N as i32 * CELL + 24;
        let fl = quire_fonts::ui::label();
        if self.dead {
            draw_centered(f, quire_fonts::ui::title(), f.width() as i32 / 2, y + 20, "Boom", TextStyle::INK);
            rail(f, ["", "Back", "New game", ""], None);
        } else if self.won {
            draw_centered(f, quire_fonts::ui::title(), f.width() as i32 / 2, y + 20, "Cleared", TextStyle::INK);
            rail(f, ["", "Back", "New game", ""], None);
        } else {
            draw_centered(f, fl, f.width() as i32 / 2, y + fl.ascent(), "Confirm opens · long Confirm flags", TextStyle::INK);
            rail(f, ["Left", "Pause", "Open", "Right"], None);
        }
        Refresh::Du
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        match (ev.key, ev.kind) {
            (Key::Back, KeyKind::Press) => Action::Push(Paused::new("Minesweeper")),
            (Key::Confirm, KeyKind::Long) => {
                if !self.open[self.cursor] && !self.dead && !self.won {
                    self.flag[self.cursor] = !self.flag[self.cursor];
                }
                Action::Redraw
            }
            (Key::Confirm, KeyKind::Press) => {
                if self.dead || self.won {
                    self.reset();
                    return Action::Redraw;
                }
                if !self.placed {
                    self.place(self.cursor);
                }
                self.reveal(self.cursor);
                Action::Redraw
            }
            (Key::Left, _) => {
                self.cursor = if self.cursor.is_multiple_of(N) { self.cursor + N - 1 } else { self.cursor - 1 };
                Action::Redraw
            }
            (Key::Right, _) => {
                self.cursor = if self.cursor % N == N - 1 { self.cursor + 1 - N } else { self.cursor + 1 };
                Action::Redraw
            }
            (Key::Up, _) => {
                self.cursor = (self.cursor + N * N - N) % (N * N);
                Action::Redraw
            }
            (Key::Down, _) => {
                self.cursor = (self.cursor + N) % (N * N);
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn result(&mut self, _cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        match r {
            Result_::Choice(1) => {
                self.reset();
                Action::Redraw
            }
            Result_::Choice(2) => Action::Pop,
            _ => Action::Redraw,
        }
    }
}

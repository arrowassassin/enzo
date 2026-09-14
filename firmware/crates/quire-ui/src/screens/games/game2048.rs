//! 2048: 112 px tiles, numerals in Literata.

use quire_gfx::{Frame, Ink, Rect, TextStyle};

use super::{board_x, Paused, Rng};
use crate::text::{centered_baseline, draw_centered};
use crate::widgets::{self, rail, running_head};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

const TILE: i32 = 112;
const GAP: i32 = 8;

/// The 2048 game.
pub struct Game2048 {
    board: [u16; 16],
    score: u32,
    best: u32,
    rng: Rng,
    started: bool,
    over: bool,
}

impl Game2048 {
    /// New.
    pub fn new() -> Self {
        Game2048 { board: [0; 16], score: 0, best: 0, rng: Rng(0x2048), started: false, over: false }
    }
    fn spawn(&mut self) {
        let empty: alloc::vec::Vec<usize> = (0..16).filter(|i| self.board[*i] == 0).collect();
        if empty.is_empty() {
            return;
        }
        let i = empty[self.rng.below(empty.len() as u32) as usize];
        self.board[i] = if self.rng.below(10) == 0 { 4 } else { 2 };
    }
    fn reset(&mut self) {
        self.board = [0; 16];
        self.score = 0;
        self.over = false;
        self.spawn();
        self.spawn();
        self.started = true;
    }
    /// Slide in a direction; returns whether anything moved.
    fn slide(&mut self, dir: Key) -> bool {
        let mut moved = false;
        for line in 0..4 {
            let idx: [usize; 4] = match dir {
                Key::Left => [line * 4, line * 4 + 1, line * 4 + 2, line * 4 + 3],
                Key::Right => [line * 4 + 3, line * 4 + 2, line * 4 + 1, line * 4],
                Key::Up => [line, line + 4, line + 8, line + 12],
                _ => [line + 12, line + 8, line + 4, line],
            };
            let vals: [u16; 4] = [self.board[idx[0]], self.board[idx[1]], self.board[idx[2]], self.board[idx[3]]];
            let mut out = [0u16; 4];
            let mut k = 0;
            let mut last: Option<usize> = None;
            for v in vals {
                if v == 0 {
                    continue;
                }
                match last {
                    Some(l) if out[l] == v => {
                        out[l] = v * 2;
                        self.score += (v * 2) as u32;
                        last = None;
                    }
                    _ => {
                        out[k] = v;
                        last = Some(k);
                        k += 1;
                    }
                }
            }
            for (n, i) in idx.iter().enumerate() {
                if self.board[*i] != out[n] {
                    moved = true;
                }
                self.board[*i] = out[n];
            }
        }
        moved
    }
    fn can_move(&self) -> bool {
        for i in 0..16 {
            if self.board[i] == 0 {
                return true;
            }
            let (r, c) = (i / 4, i % 4);
            if c < 3 && self.board[i] == self.board[i + 1] {
                return true;
            }
            if r < 3 && self.board[i] == self.board[i + 4] {
                return true;
            }
        }
        false
    }
}

impl Default for Game2048 {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Game2048 {
    fn name(&self) -> &'static str {
        "80-2048"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.started {
            self.rng = Rng(cx.env.random() | 1);
            self.reset();
        }
        running_head(f, "2048", Some(&alloc::format!("{} · best {}", self.score, self.best.max(self.score))));
        let size = 4 * TILE + 3 * GAP;
        let bx = board_x(f, size);
        let by = widgets::CONTENT_TOP + 8;
        let poster = quire_fonts::ui::poster();
        let title = quire_fonts::ui::title();
        for i in 0..16 {
            let (r, c) = (i / 4, i % 4);
            let rect = Rect::new(bx + c as i32 * (TILE + GAP), by + r as i32 * (TILE + GAP), TILE as u32, TILE as u32);
            let v = self.board[i];
            f.stroke_rect(rect, if v == 0 { 1 } else { 2 }, Ink::Black);
            if v == 0 {
                continue;
            }
            // Bigger tiles get a hatched surround so the eye sees the hierarchy.
            let inv = v >= 128;
            if inv {
                f.fill_rect(rect, Ink::Black);
            } else if v >= 16 {
                f.pattern_rect(rect.inset(3), quire_gfx::Pattern::Sparse);
                f.fill_rect(rect.inset(24), Ink::White);
            }
            let s = alloc::format!("{v}");
            let font = if v >= 1000 { title } else { poster };
            draw_centered(
                f,
                font,
                rect.x + TILE / 2,
                centered_baseline(font, rect.y, TILE),
                &s,
                TextStyle { inverted: inv, ..TextStyle::INK },
            );
        }
        let fl = quire_fonts::ui::label();
        if self.over {
            draw_centered(f, quire_fonts::ui::title(), f.width() as i32 / 2, by + size + 40, "No more moves", TextStyle::INK);
            rail(f, ["", "Back", "New game", ""], None);
        } else {
            draw_centered(f, fl, f.width() as i32 / 2, by + size + 30, "Left · Right · Up · Down slide the tiles", TextStyle::INK);
            rail(f, ["Left", "Pause", "", "Right"], None);
        }
        Refresh::Du
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Push(Paused::new("2048")),
            Key::Confirm => {
                if self.over {
                    self.best = self.best.max(self.score);
                    self.reset();
                }
                Action::Redraw
            }
            Key::Left | Key::Right | Key::Up | Key::Down => {
                if !self.over && self.slide(ev.key) {
                    self.spawn();
                    if !self.can_move() {
                        self.over = true;
                        self.best = self.best.max(self.score);
                    }
                }
                Action::Redraw
            }
            Key::Power => Action::None,
        }
    }
    fn result(&mut self, _cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        match r {
            Result_::Choice(1) => {
                self.best = self.best.max(self.score);
                self.reset();
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
    fn slides_and_merges() {
        let mut g = Game2048::new();
        g.board = [2, 2, 0, 0, 4, 0, 4, 0, 2, 4, 2, 4, 0, 0, 0, 8];
        assert!(g.slide(Key::Left));
        assert_eq!(&g.board[..4], &[4, 0, 0, 0]);
        assert_eq!(&g.board[4..8], &[8, 0, 0, 0]);
        assert_eq!(&g.board[8..12], &[2, 4, 2, 4]);
        assert_eq!(&g.board[12..16], &[8, 0, 0, 0]);
        assert_eq!(g.score, 12);
    }
}

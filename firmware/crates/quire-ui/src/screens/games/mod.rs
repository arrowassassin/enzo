//! 80 games: a list with descriptions, boards on the inner 480 px, inversion cursors,
//! a paused card for each.

pub mod chess;
pub mod game2048;
pub mod minesweeper;
pub mod sudoku;
pub mod wordle;

use alloc::boxed::Box;
use quire_gfx::Frame;

use crate::text::page_indicator;
use crate::theme::*;
use crate::widgets::{self, rail, row, running_head, ListNav, RowState};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

const GAMES: [(&str, &str, &str); 5] = [
    ("Sudoku", "three difficulties, pencil marks", "80-sudoku"),
    ("2048", "slide the tiles, reach 2048", "80-2048"),
    ("Minesweeper", "9 × 9, ten mines, first press is safe", "80-minesweeper"),
    ("Chess", "play white against the reader", "80-chess"),
    ("Wordle", "five letters, six tries, a word a day", "80-wordle"),
];

/// Open a game by name.
pub fn open_game<E: Env>(name: &str) -> Option<Box<dyn Screen<E>>> {
    Some(match name {
        "80-sudoku" => Box::new(sudoku::Sudoku::new()),
        "80-2048" => Box::new(game2048::Game2048::new()),
        "80-minesweeper" => Box::new(minesweeper::Minesweeper::new()),
        "80-chess" => Box::new(chess::ChessScreen::new()),
        "80-wordle" => Box::new(wordle::Wordle::new()),
        _ => return None,
    })
}

/// The games list.
pub struct GamesList {
    nav: ListNav,
}

impl GamesList {
    /// New.
    pub fn new() -> Self {
        GamesList { nav: ListNav::new(GAMES.len(), 9) }
    }
}

impl Default for GamesList {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for GamesList {
    fn name(&self) -> &'static str {
        "80-games"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, "Games", Some(&page_indicator(self.nav.page(), self.nav.pages())));
        let mut y = widgets::CONTENT_TOP;
        for i in self.nav.visible() {
            let (t, s, _) = GAMES[i];
            row(f, y, row_h, t, Some(s), None, if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        rail(f, ["Apps", "Back", "Play", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        if ev.is(Key::Left) {
            return Action::Replace(Box::new(super::apps::AppsList::new()));
        }
        if ev.is(Key::Confirm) {
            return match open_game::<E>(GAMES[self.nav.focus].2) {
                Some(s) => Action::Push(s),
                None => Action::None,
            };
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
}

/// The paused card: Resume (Back), New game (Confirm), Quit (Right).
pub struct Paused {
    title: &'static str,
}

impl Paused {
    /// New.
    pub fn new(title: &'static str) -> Box<Self> {
        Box::new(Paused { title })
    }
}

impl<E: Env> Screen<E> for Paused {
    fn name(&self) -> &'static str {
        "80-paused"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        widgets::screen(f, f.bounds());
        let card =
            widgets::dialog(f, "Paused", &alloc::format!("{} is paused. Right quits to the list.", self.title), "Resume", "New game");
        let _ = card;
        rail(f, ["", "Resume", "New game", "Quit"], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::PopWith(Result_::Cancel),
            Key::Confirm => Action::PopWith(Result_::Choice(1)),
            Key::Right => Action::PopWith(Result_::Choice(2)),
            _ => Action::None,
        }
    }
}

/// A tiny xorshift for the games (seeded from the platform).
#[derive(Clone, Copy)]
pub struct Rng(pub u32);

impl Rng {
    /// Next value.
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// Below `n`.
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n.max(1)
    }
}

/// Left inset for a board of width `w` on the page.
pub fn board_x(f: &Frame, w: i32) -> i32 {
    (f.width() as i32 - w) / 2
}

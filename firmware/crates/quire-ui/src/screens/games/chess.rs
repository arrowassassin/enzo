//! Chess: 60 px squares, hatched dark squares, clear piece glyphs, an alpha-beta engine.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Pattern, Rect, TextStyle};

use super::{board_x, Paused, Rng};
use crate::widgets::{self, rail, running_head};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

const SQ: i32 = 60;

/// Piece codes: 0 empty; 1–6 white P N B R Q K; 7–12 black.
type Board = [u8; 64];

const START: Board = [
    10, 8, 9, 11, 12, 9, 8, 10, 7, 7, 7, 7, 7, 7, 7, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 4, 2, 3, 5, 6, 3, 2, 4,
];

fn is_white(p: u8) -> bool {
    (1..=6).contains(&p)
}
fn is_black(p: u8) -> bool {
    p >= 7
}
fn kind(p: u8) -> u8 {
    if p == 0 {
        0
    } else if p > 6 {
        p - 6
    } else {
        p
    }
}
fn own(p: u8, white: bool) -> bool {
    p != 0 && is_white(p) == white
}

/// A move: from, to, promotion kind (0 none), flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Mv {
    from: u8,
    to: u8,
    promo: u8,
    castle: bool,
    ep: bool,
}

/// Game state.
#[derive(Clone)]
pub struct Position {
    board: Board,
    white_to_move: bool,
    /// Castling rights: wk, wq, bk, bq.
    castle: [bool; 4],
    ep: Option<u8>,
    halfmove: u16,
}

impl Position {
    fn start() -> Self {
        Position { board: START, white_to_move: true, castle: [true; 4], ep: None, halfmove: 0 }
    }

    fn attacked(&self, sq: usize, by_white: bool) -> bool {
        let (r, c) = ((sq / 8) as i32, (sq % 8) as i32);
        let at = |rr: i32, cc: i32| -> u8 {
            if (0..8).contains(&rr) && (0..8).contains(&cc) {
                self.board[(rr * 8 + cc) as usize]
            } else {
                255
            }
        };
        // Pawns.
        let dr = if by_white { 1 } else { -1 };
        for dc in [-1, 1] {
            let p = at(r + dr, c + dc);
            if p != 255 && own(p, by_white) && kind(p) == 1 {
                return true;
            }
        }
        // Knights.
        for (dr, dc) in [(1, 2), (2, 1), (-1, 2), (-2, 1), (1, -2), (2, -1), (-1, -2), (-2, -1)] {
            let p = at(r + dr, c + dc);
            if p != 255 && own(p, by_white) && kind(p) == 2 {
                return true;
            }
        }
        // King.
        for dr in -1..=1 {
            for dc in -1..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let p = at(r + dr, c + dc);
                if p != 255 && own(p, by_white) && kind(p) == 6 {
                    return true;
                }
            }
        }
        // Sliders.
        for (dr, dc, diag) in
            [(1, 0, false), (-1, 0, false), (0, 1, false), (0, -1, false), (1, 1, true), (1, -1, true), (-1, 1, true), (-1, -1, true)]
        {
            let (mut rr, mut cc) = (r + dr, c + dc);
            loop {
                let p = at(rr, cc);
                if p == 255 {
                    break;
                }
                if p != 0 {
                    if own(p, by_white) {
                        let k = kind(p);
                        if k == 5 || (diag && k == 3) || (!diag && k == 4) {
                            return true;
                        }
                    }
                    break;
                }
                rr += dr;
                cc += dc;
            }
        }
        false
    }

    fn king_sq(&self, white: bool) -> Option<usize> {
        let k = if white { 6 } else { 12 };
        self.board.iter().position(|p| *p == k)
    }

    fn in_check(&self, white: bool) -> bool {
        self.king_sq(white).map(|k| self.attacked(k, !white)).unwrap_or(false)
    }

    fn pseudo_moves(&self, out: &mut Vec<Mv>) {
        let white = self.white_to_move;
        for from in 0..64usize {
            let p = self.board[from];
            if !own(p, white) {
                continue;
            }
            let (r, c) = ((from / 8) as i32, (from % 8) as i32);
            let push = |out: &mut Vec<Mv>, rr: i32, cc: i32, capture_only: bool, quiet_only: bool| -> bool {
                if !(0..8).contains(&rr) || !(0..8).contains(&cc) {
                    return false;
                }
                let to = (rr * 8 + cc) as usize;
                let t = self.board[to];
                if own(t, white) {
                    return false;
                }
                if capture_only && t == 0 {
                    return false;
                }
                if quiet_only && t != 0 {
                    return false;
                }
                out.push(Mv { from: from as u8, to: to as u8, ..Default::default() });
                t == 0
            };
            match kind(p) {
                1 => {
                    let dir = if white { -1 } else { 1 };
                    let start_row = if white { 6 } else { 1 };
                    let last_row = if white { 0 } else { 7 };
                    let one = r + dir;
                    if (0..8).contains(&one) && self.board[(one * 8 + c) as usize] == 0 {
                        if one == last_row {
                            for promo in [5u8, 2, 4, 3] {
                                out.push(Mv { from: from as u8, to: (one * 8 + c) as u8, promo, ..Default::default() });
                            }
                        } else {
                            out.push(Mv { from: from as u8, to: (one * 8 + c) as u8, ..Default::default() });
                            if r == start_row && self.board[((r + 2 * dir) * 8 + c) as usize] == 0 {
                                out.push(Mv { from: from as u8, to: ((r + 2 * dir) * 8 + c) as u8, ..Default::default() });
                            }
                        }
                    }
                    for dc in [-1, 1] {
                        let (rr, cc) = (r + dir, c + dc);
                        if !(0..8).contains(&rr) || !(0..8).contains(&cc) {
                            continue;
                        }
                        let to = (rr * 8 + cc) as usize;
                        let t = self.board[to];
                        if t != 0 && !own(t, white) {
                            if rr == last_row {
                                for promo in [5u8, 2, 4, 3] {
                                    out.push(Mv { from: from as u8, to: to as u8, promo, ..Default::default() });
                                }
                            } else {
                                out.push(Mv { from: from as u8, to: to as u8, ..Default::default() });
                            }
                        } else if t == 0 && self.ep == Some(to as u8) {
                            out.push(Mv { from: from as u8, to: to as u8, ep: true, ..Default::default() });
                        }
                    }
                }
                2 => {
                    for (dr, dc) in [(1, 2), (2, 1), (-1, 2), (-2, 1), (1, -2), (2, -1), (-1, -2), (-2, -1)] {
                        push(out, r + dr, c + dc, false, false);
                    }
                }
                6 => {
                    for dr in -1..=1 {
                        for dc in -1..=1 {
                            if dr != 0 || dc != 0 {
                                push(out, r + dr, c + dc, false, false);
                            }
                        }
                    }
                    // Castling.
                    let (row, ks, qs) = if white { (7, self.castle[0], self.castle[1]) } else { (0, self.castle[2], self.castle[3]) };
                    if r == row && c == 4 && !self.in_check(white) {
                        let b = &self.board;
                        if ks
                            && b[(row * 8 + 5) as usize] == 0
                            && b[(row * 8 + 6) as usize] == 0
                            && kind(b[(row * 8 + 7) as usize]) == 4
                            && !self.attacked((row * 8 + 5) as usize, !white)
                        {
                            out.push(Mv { from: from as u8, to: (row * 8 + 6) as u8, castle: true, ..Default::default() });
                        }
                        if qs
                            && b[(row * 8 + 3) as usize] == 0
                            && b[(row * 8 + 2) as usize] == 0
                            && b[(row * 8 + 1) as usize] == 0
                            && kind(b[(row * 8) as usize]) == 4
                            && !self.attacked((row * 8 + 3) as usize, !white)
                        {
                            out.push(Mv { from: from as u8, to: (row * 8 + 2) as u8, castle: true, ..Default::default() });
                        }
                    }
                }
                k => {
                    let dirs: &[(i32, i32)] = match k {
                        3 => &[(1, 1), (1, -1), (-1, 1), (-1, -1)],
                        4 => &[(1, 0), (-1, 0), (0, 1), (0, -1)],
                        _ => &[(1, 1), (1, -1), (-1, 1), (-1, -1), (1, 0), (-1, 0), (0, 1), (0, -1)],
                    };
                    for (dr, dc) in dirs {
                        let (mut rr, mut cc) = (r + dr, c + dc);
                        while push(out, rr, cc, false, false) {
                            rr += dr;
                            cc += dc;
                        }
                    }
                }
            }
        }
    }

    /// Legal moves.
    pub fn moves(&self) -> Vec<Mv> {
        let mut pseudo = Vec::with_capacity(48);
        self.pseudo_moves(&mut pseudo);
        let white = self.white_to_move;
        pseudo
            .into_iter()
            .filter(|m| {
                let mut p = self.clone();
                p.apply(*m);
                !p.in_check(white)
            })
            .collect()
    }

    fn apply(&mut self, m: Mv) {
        let (from, to) = (m.from as usize, m.to as usize);
        let p = self.board[from];
        let white = is_white(p);
        let capture = self.board[to] != 0 || m.ep;
        self.board[to] = if m.promo != 0 {
            if white {
                m.promo
            } else {
                m.promo + 6
            }
        } else {
            p
        };
        self.board[from] = 0;
        if m.ep {
            let cap = if white { to + 8 } else { to - 8 };
            self.board[cap] = 0;
        }
        if m.castle {
            let row = from / 8;
            if to % 8 == 6 {
                self.board[row * 8 + 5] = self.board[row * 8 + 7];
                self.board[row * 8 + 7] = 0;
            } else {
                self.board[row * 8 + 3] = self.board[row * 8];
                self.board[row * 8] = 0;
            }
        }
        // Rights.
        match from {
            60 => {
                self.castle[0] = false;
                self.castle[1] = false;
            }
            4 => {
                self.castle[2] = false;
                self.castle[3] = false;
            }
            63 => self.castle[0] = false,
            56 => self.castle[1] = false,
            7 => self.castle[2] = false,
            0 => self.castle[3] = false,
            _ => {}
        }
        match to {
            63 => self.castle[0] = false,
            56 => self.castle[1] = false,
            7 => self.castle[2] = false,
            0 => self.castle[3] = false,
            _ => {}
        }
        self.ep = None;
        if kind(p) == 1 && (from as i32 - to as i32).abs() == 16 {
            self.ep = Some(((from + to) / 2) as u8);
        }
        self.halfmove = if kind(p) == 1 || capture { 0 } else { self.halfmove + 1 };
        self.white_to_move = !self.white_to_move;
    }

    /// Material plus a little position, from white's view.
    fn eval(&self) -> i32 {
        const VAL: [i32; 7] = [0, 100, 320, 330, 500, 900, 0];
        let mut s = 0;
        for (i, p) in self.board.iter().enumerate() {
            if *p == 0 {
                continue;
            }
            let k = kind(*p) as usize;
            let (r, c) = ((i / 8) as i32, (i % 8) as i32);
            let centre = -((r - 3).abs() + (r - 4).abs() + (c - 3).abs() + (c - 4).abs()) + 7;
            let mut v = VAL[k] + centre * 2;
            if k == 1 {
                // Advanced pawns.
                v += if is_white(*p) { (6 - r) * 6 } else { (r - 1) * 6 };
            }
            if is_white(*p) {
                s += v;
            } else {
                s -= v;
            }
        }
        s
    }
}

fn search(p: &Position, depth: u8, mut alpha: i32, beta: i32, nodes: &mut u32) -> i32 {
    *nodes += 1;
    let moves = p.moves();
    let white = p.white_to_move;
    if moves.is_empty() {
        return if p.in_check(white) {
            if white {
                -100_000
            } else {
                100_000
            }
        } else {
            0
        };
    }
    if depth == 0 || *nodes > 60_000 {
        return p.eval();
    }
    // Captures first for better cut-offs.
    let mut ordered: Vec<(i32, Mv)> = moves.iter().map(|m| ((p.board[m.to as usize] != 0) as i32 * 10 + m.promo as i32, *m)).collect();
    ordered.sort_by_key(|m| core::cmp::Reverse(m.0));
    if white {
        let mut best = -1_000_000;
        for (_, m) in ordered {
            let mut q = p.clone();
            q.apply(m);
            let v = search(&q, depth - 1, alpha, beta, nodes);
            best = best.max(v);
            alpha = alpha.max(v);
            if beta <= alpha {
                break;
            }
        }
        best
    } else {
        let mut best = 1_000_000;
        let mut beta = beta;
        for (_, m) in ordered {
            let mut q = p.clone();
            q.apply(m);
            let v = search(&q, depth - 1, alpha, beta, nodes);
            best = best.min(v);
            beta = beta.min(v);
            if beta <= alpha {
                break;
            }
        }
        best
    }
}

/// The engine's move for the side to move.
pub fn best_move(p: &Position, depth: u8, rng: &mut Rng) -> Option<Mv> {
    let moves = p.moves();
    if moves.is_empty() {
        return None;
    }
    let white = p.white_to_move;
    let mut best: Vec<(i32, Mv)> = Vec::new();
    let mut nodes = 0u32;
    for m in moves {
        let mut q = p.clone();
        q.apply(m);
        let v = search(&q, depth - 1, -1_000_000, 1_000_000, &mut nodes);
        best.push((if white { v } else { -v }, m));
    }
    best.sort_by_key(|m| core::cmp::Reverse(m.0));
    let top = best[0].0;
    let ties: Vec<Mv> = best.iter().filter(|b| b.0 == top).map(|b| b.1).collect();
    Some(ties[rng.below(ties.len() as u32) as usize])
}

fn square_name(sq: u8) -> String {
    let (r, c) = (sq / 8, sq % 8);
    alloc::format!("{}{}", (b'a' + c) as char, 8 - r)
}

/// The chess screen.
pub struct ChessScreen {
    pos: Position,
    cursor: usize,
    selected: Option<usize>,
    history: Vec<String>,
    rng: Rng,
    thinking: bool,
    message: String,
    started: bool,
    flipped: bool,
}

impl ChessScreen {
    /// New: the player is white.
    pub fn new() -> Self {
        ChessScreen {
            pos: Position::start(),
            cursor: 52,
            selected: None,
            history: Vec::new(),
            rng: Rng(7),
            thinking: false,
            message: String::new(),
            started: false,
            flipped: false,
        }
    }
    fn status(&mut self) {
        let moves = self.pos.moves();
        let white = self.pos.white_to_move;
        self.message = if moves.is_empty() {
            if self.pos.in_check(white) {
                String::from(if white { "Checkmate — the reader wins" } else { "Checkmate — you win" })
            } else {
                String::from("Stalemate")
            }
        } else if self.pos.halfmove >= 100 {
            String::from("Draw by the fifty-move rule")
        } else if self.pos.in_check(white) {
            String::from("Check")
        } else {
            String::new()
        };
    }
    fn game_over(&self) -> bool {
        self.pos.moves().is_empty() || self.pos.halfmove >= 100
    }
}

impl Default for ChessScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for ChessScreen {
    fn name(&self) -> &'static str {
        "80-chess"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.started {
            self.rng = Rng(cx.env.random() | 1);
            self.started = true;
        }
        running_head(f, "Chess", Some(&alloc::format!("move {}", self.history.len() / 2 + 1)));
        let bx = board_x(f, 8 * SQ);
        let by = widgets::CONTENT_TOP;
        for i in 0..64usize {
            let (r, c) = (i / 8, i % 8);
            let (dr, dc) = if self.flipped { (7 - r, 7 - c) } else { (r, c) };
            let rect = Rect::new(bx + dc as i32 * SQ, by + dr as i32 * SQ, SQ as u32, SQ as u32);
            let dark = !(r + c).is_multiple_of(2);
            if dark {
                f.pattern_rect(rect, Pattern::Hatch { pitch: 4 });
            }
            let p = self.pos.board[i];
            if p != 0 {
                super::pieces::draw_piece(f, rect, kind(p), is_black(p));
            }
            if self.selected == Some(i) {
                f.stroke_rect(rect, 4, Ink::Black);
            }
            if i == self.cursor {
                f.stroke_rect(rect.inset(2), 3, Ink::Black);
                f.invert_rect(rect.inset(5));
            }
        }
        f.stroke_rect(Rect::new(bx - 1, by - 1, (8 * SQ + 2) as u32, (8 * SQ + 2) as u32), 2, Ink::Black);
        let y = by + 8 * SQ + 16;
        let fl = quire_fonts::ui::label();
        let mono = quire_fonts::ui::mono();
        let last: Vec<&String> = self.history.iter().rev().take(8).collect();
        let line: String = last.iter().rev().map(|s| s.as_str()).collect::<Vec<_>>().join("  ");
        draw_text(f, mono, bx, y + mono.ascent(), &crate::text::ellipsis(mono, &line, 8 * SQ), TextStyle::INK);
        let msg = if self.thinking { "The reader is thinking…" } else { &self.message };
        draw_text(f, fl, bx, y + 30 + fl.ascent(), msg, TextStyle::INK);
        if self.game_over() {
            rail(f, ["", "Back", "New game", ""], None);
        } else {
            rail(f, ["Left", "Pause", if self.selected.is_some() { "Move" } else { "Select" }, "Right"], None);
        }
        Refresh::Du
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if self.thinking {
            return Action::None;
        }
        match (ev.key, ev.kind) {
            (Key::Back, KeyKind::Press) => {
                if self.selected.is_some() {
                    self.selected = None;
                    return Action::Redraw;
                }
                Action::Push(Paused::new("Chess"))
            }
            (Key::Confirm, KeyKind::Press) => {
                if self.game_over() {
                    *self = ChessScreen::new();
                    return Action::Redraw;
                }
                if !self.pos.white_to_move {
                    return Action::None;
                }
                match self.selected {
                    None => {
                        if own(self.pos.board[self.cursor], true) {
                            self.selected = Some(self.cursor);
                        }
                        Action::Redraw
                    }
                    Some(from) => {
                        let moves = self.pos.moves();
                        let mv = moves.iter().filter(|m| m.from as usize == from && m.to as usize == self.cursor).max_by_key(|m| m.promo);
                        match mv.copied() {
                            Some(m) => {
                                let piece = letter(self.pos.board[from]);
                                self.pos.apply(m);
                                self.history.push(alloc::format!(
                                    "{}{}{}",
                                    if piece == "P" { "" } else { piece },
                                    square_name(m.from),
                                    square_name(m.to)
                                ));
                                self.selected = None;
                                self.status();
                                if !self.game_over() {
                                    self.thinking = true;
                                    cx.env.request(crate::SysRequest::Timer(50));
                                }
                            }
                            None => {
                                if own(self.pos.board[self.cursor], true) {
                                    self.selected = Some(self.cursor);
                                } else {
                                    self.selected = None;
                                }
                            }
                        }
                        Action::Redraw
                    }
                }
            }
            (Key::Left, _) => {
                self.cursor = if self.cursor.is_multiple_of(8) { self.cursor + 7 } else { self.cursor - 1 };
                Action::Redraw
            }
            (Key::Right, _) => {
                self.cursor = if self.cursor % 8 == 7 { self.cursor - 7 } else { self.cursor + 1 };
                Action::Redraw
            }
            (Key::Up, _) => {
                self.cursor = (self.cursor + 56) % 64;
                Action::Redraw
            }
            (Key::Down, _) => {
                self.cursor = (self.cursor + 8) % 64;
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if self.thinking && matches!(ev, Event::Timer | Event::Tick) {
            if let Some(m) = best_move(&self.pos, 3, &mut self.rng) {
                let piece = letter(self.pos.board[m.from as usize]);
                self.pos.apply(m);
                self.history.push(alloc::format!("{}{}{}", if piece == "P" { "" } else { piece }, square_name(m.from), square_name(m.to)));
            }
            self.thinking = false;
            self.status();
            return Action::Redraw;
        }
        Action::None
    }
    fn result(&mut self, _cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        match r {
            Result_::Choice(1) => {
                *self = ChessScreen::new();
                Action::Redraw
            }
            Result_::Choice(2) => Action::Pop,
            _ => Action::Redraw,
        }
    }
}

/// Algebraic letter for a piece (empty for pawns), used in the move list.
fn letter(p: u8) -> &'static str {
    match kind(p) {
        2 => "N",
        3 => "B",
        4 => "R",
        5 => "Q",
        6 => "K",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_position_has_twenty_moves_and_engine_captures_free_queen() {
        let p = Position::start();
        assert_eq!(p.moves().len(), 20);
        // White queen hanging on d4 for black to take with the knight from b8? Set up simply:
        let mut q = Position::start();
        q.board = [0; 64];
        q.board[60] = 6; // white king e1
        q.board[4] = 12; // black king e8
        q.board[27] = 5; // white queen d5 (index 27 = row 3, col 3)
        q.board[10] = 8; // black knight c7 attacks d5
        q.white_to_move = false;
        q.castle = [false; 4];
        let mut rng = Rng(1);
        let m = best_move(&q, 2, &mut rng).unwrap();
        assert_eq!(m.to, 27, "engine takes the queen");
        // Castling is generated when the path is clear.
        let mut c = Position::start();
        c.board[61] = 0;
        c.board[62] = 0;
        assert!(c.moves().iter().any(|m| m.castle));
        let _ = draw_text;
    }
}

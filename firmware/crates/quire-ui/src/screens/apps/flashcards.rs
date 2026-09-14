//! 71 flashcards: decks from `/flashcards/*.txt` (`front | back` per line), spaced
//! repetition (a compact SM-2), rating on the rail, a poster summary.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use serde::{Deserialize, Serialize};

use crate::text::{draw_centered, draw_label, line_h, page_indicator, wrap};
use crate::theme::*;
use crate::widgets::{self, empty_state, poster_tiles, rail, row, running_head, ListNav, RowState};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

/// Deck folder.
pub const DECKS_DIR: &str = "/flashcards";

/// Scheduling state per card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CardState {
    /// Interval in days.
    pub interval: u16,
    /// Ease × 100 (250 = 2.5).
    pub ease: u16,
    /// Due day.
    pub due: u16,
    /// Successful reviews in a row.
    pub reps: u16,
}

/// A deck's saved state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DeckState {
    /// Per-card state, indexed like the deck file's lines.
    pub cards: Vec<CardState>,
}

fn state_path(deck: &str) -> String {
    alloc::format!("/.quire/flashcards/{deck}.bin")
}

/// Load a deck: (fronts, backs).
pub fn load_deck<F: Fs>(fs: &F, name: &str) -> Vec<(String, String)> {
    let Ok(b) = fs.read_to_vec(&quire_fs::join(DECKS_DIR, name)) else { return Vec::new() };
    let text = String::from_utf8_lossy(&b);
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (a, b) = l.split_once('|').or_else(|| l.split_once('\t'))?;
            Some((String::from(a.trim()), String::from(b.trim())))
        })
        .take(5000)
        .collect()
}

/// Apply a rating (0 again, 1 hard, 2 good, 3 easy) to a card state.
pub fn rate(c: &mut CardState, rating: u8, today: u16) {
    if c.ease == 0 {
        c.ease = 250;
    }
    match rating {
        0 => {
            c.reps = 0;
            c.interval = 0;
            c.ease = c.ease.saturating_sub(20).max(130);
            c.due = today;
        }
        _ => {
            c.reps += 1;
            let mult = match rating {
                1 => 120,
                2 => c.ease,
                _ => c.ease + 50,
            };
            c.interval = match c.reps {
                1 => 1,
                2 => 6,
                _ => ((c.interval as u32 * mult as u32) / 100).clamp(1, 3650) as u16,
            };
            if rating == 1 {
                c.ease = c.ease.saturating_sub(15).max(130);
            } else if rating == 3 {
                c.ease += 15;
            }
            c.due = today + c.interval;
        }
    }
}

/// The deck list: (file name, cards, due today), read once per visit — never per draw.
pub struct Decks {
    decks: Vec<(String, usize, usize)>,
    nav: ListNav,
    loaded: bool,
}

impl Decks {
    /// New.
    pub fn new() -> Self {
        Decks { decks: Vec::new(), nav: ListNav::new(0, 9), loaded: false }
    }
}

impl Default for Decks {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Decks {
    fn name(&self) -> &'static str {
        "71-flashcards"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.loaded {
            let fs = cx.env.fs();
            let today = cx.today();
            let mut names: Vec<String> = fs
                .read_dir(DECKS_DIR)
                .unwrap_or_default()
                .into_iter()
                .filter(|e| !e.is_dir && e.name.ends_with(".txt"))
                .map(|e| e.name)
                .collect();
            names.sort();
            self.decks = names
                .into_iter()
                .map(|name| {
                    let cards = load_deck(fs, &name);
                    let state: DeckState =
                        fs.read_to_vec(&state_path(&name)).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default();
                    let due = cards.iter().enumerate().filter(|(i, _)| state.cards.get(*i).map(|c| c.due <= today).unwrap_or(true)).count();
                    (name, cards.len(), due)
                })
                .collect();
            self.nav.set_n(self.decks.len());
            self.loaded = true;
        }
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, "Flashcards", Some(&page_indicator(self.nav.page(), self.nav.pages())));
        if self.decks.is_empty() {
            empty_state(f, 240, "No decks yet", "Drop a text file into /flashcards with one card per line: front | back.");
            rail(f, ["", "Back", "", ""], None);
            return Refresh::Gc;
        }
        let mut y = widgets::CONTENT_TOP;
        for i in self.nav.visible() {
            let (name, cards, due) = &self.decks[i];
            row(
                f,
                y,
                row_h,
                name.trim_end_matches(".txt"),
                Some(&alloc::format!("{cards} {}", if *cards == 1 { "card" } else { "cards" })),
                Some(&alloc::format!("{due} due")),
                if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
            );
            y += row_h;
        }
        rail(f, ["", "Back", "Study", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        if ev.is(Key::Confirm) {
            if let Some((name, _, _)) = self.decks.get(self.nav.focus).cloned() {
                return Action::Push(alloc::boxed::Box::new(Study::new(cx, &name)));
            }
            return Action::None;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
    fn resume(&mut self, _cx: &mut Ctx<E>) {
        self.loaded = false;
    }
}

/// A study session.
pub struct Study {
    deck: String,
    cards: Vec<(String, String)>,
    state: DeckState,
    queue: Vec<usize>,
    pos: usize,
    back: bool,
    rated: [u16; 4],
    done: bool,
}

impl Study {
    /// Start a session on the cards due today (or all when none are due).
    pub fn new<E: Env>(cx: &mut Ctx<E>, deck: &str) -> Self {
        let fs = cx.env.fs();
        let cards = load_deck(fs, deck);
        let mut state: DeckState = fs.read_to_vec(&state_path(deck)).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default();
        state.cards.resize(cards.len(), CardState::default());
        let today = cx.today();
        let mut queue: Vec<usize> = (0..cards.len()).filter(|i| state.cards[*i].due <= today).collect();
        if queue.is_empty() {
            queue = (0..cards.len()).collect();
        }
        queue.truncate(50);
        Study { deck: deck.into(), cards, state, queue, pos: 0, back: false, rated: [0; 4], done: false }
    }
    fn save<E: Env>(&self, cx: &Ctx<E>) {
        let fs = cx.env.fs();
        let _ = fs.mkdir_all("/.quire/flashcards");
        if let Ok(b) = postcard::to_allocvec(&self.state) {
            let _ = fs.write_atomic(&state_path(&self.deck), &b);
        }
    }
}

impl<E: Env> Screen<E> for Study {
    fn name(&self) -> &'static str {
        if self.done {
            "71-flashcards-summary"
        } else if self.back {
            "71-flashcards-back"
        } else {
            "71-flashcards-front"
        }
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let w = f.width() as i32;
        let h = f.height() as i32;
        let title = self.deck.trim_end_matches(".txt");
        if self.done || self.queue.is_empty() {
            running_head(f, title, None);
            let total: u16 = self.rated.iter().sum();
            let tiles = alloc::vec![
                (alloc::format!("{total}"), String::from("Reviewed")),
                (alloc::format!("{}", self.rated[2] + self.rated[3]), String::from("Known")),
                (alloc::format!("{}", self.rated[0]), String::from("Again")),
                (alloc::format!("{}", self.cards.len()), String::from("In deck")),
            ];
            poster_tiles(f, widgets::INSET, widgets::CONTENT_TOP + 20, w - 2 * widgets::INSET, &tiles, 2);
            rail(f, ["", "Back", "Again", ""], None);
            return Refresh::Gc;
        }
        running_head(f, title, Some(&alloc::format!("{} / {}", self.pos + 1, self.queue.len())));
        let (front, back) = &self.cards[self.queue[self.pos]];
        let ft = quire_fonts::ui::title();
        let fb = quire_fonts::ui::list_title();
        let fl = quire_fonts::ui::label();
        let text = if self.back { back } else { front };
        let font = if self.back { fb } else { ft };
        let lines = wrap(font, text, w - 2 * widgets::INSET - 16);
        let block = lines.len() as i32 * line_h(font);
        let mut y = (h - RAIL_H - block) / 2;
        if self.back {
            draw_label(f, widgets::INSET, widgets::CONTENT_TOP + fl.ascent(), front, false);
            f.fill_rect(Rect::new(widgets::INSET, widgets::CONTENT_TOP + line_h(fl) + 6, (w - 2 * widgets::INSET) as u32, 2), Ink::Black);
        }
        for l in lines {
            draw_centered(f, font, w / 2, y + font.ascent(), &l, TextStyle::INK);
            y += line_h(font);
        }
        let _ = draw_text;
        if self.back {
            rail(f, ["Again", "Hard", "Good", "Easy"], None);
        } else {
            draw_centered(f, fl, w / 2, h - RAIL_H - 24, "Confirm — show the back", TextStyle::INK);
            rail(f, ["", "Back", "Flip", "Skip"], None);
        }
        Refresh::Du
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        if self.done || self.queue.is_empty() {
            return match ev.key {
                Key::Confirm => {
                    *self = Study::new(cx, &self.deck.clone());
                    Action::Redraw
                }
                _ => Action::Pop,
            };
        }
        if !self.back {
            return match ev.key {
                Key::Back => {
                    self.save(cx);
                    Action::Pop
                }
                Key::Confirm => {
                    self.back = true;
                    Action::Redraw
                }
                Key::Right => {
                    self.pos += 1;
                    if self.pos >= self.queue.len() {
                        self.done = true;
                        self.save(cx);
                    }
                    Action::Redraw
                }
                _ => Action::None,
            };
        }
        let rating = match ev.key {
            Key::Left => 0,
            Key::Back => 1,
            Key::Confirm => 2,
            Key::Right => 3,
            _ => return Action::None,
        };
        let today = cx.today();
        let idx = self.queue[self.pos];
        rate(&mut self.state.cards[idx], rating, today);
        self.rated[rating as usize] += 1;
        if rating == 0 {
            // Again: the card comes back at the end of the session.
            self.queue.push(idx);
        }
        self.back = false;
        self.pos += 1;
        if self.pos >= self.queue.len() {
            self.done = true;
            self.save(cx);
        }
        Action::Redraw
    }
}

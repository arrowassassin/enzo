//! 70 apps: a typographic list, and the apps themselves.

pub mod calculator;
pub mod clock;
pub mod fiction;
pub mod flashcards;
pub mod images;
pub mod news;
pub mod notes;
pub mod weather;
pub mod wikipedia;

use alloc::boxed::Box;
use quire_gfx::Frame;

use crate::text::page_indicator;
use crate::theme::*;
use crate::widgets::{self, rail, row, running_head, ListNav, RowState};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

const APPS: [(&str, &str, &str); 9] = [
    ("Clock", "time, timer, alarms", "73-clock"),
    ("Flashcards", "decks from the card, spaced repetition", "71-flashcards"),
    ("News", "feeds, read on the page", "72-news"),
    ("Wikipedia", "look anything up", "77-wikipedia"),
    ("Weather", "five days for your place", "74-weather"),
    ("Calculator", "large keys, mono digits", "78-calculator"),
    ("Notes", "typed on the phone", "79-notes"),
    ("Images", "photos on the card, dithered", "75-images"),
    ("Interactive fiction", "Z-machine stories from the card", "76-fiction"),
];

/// Open an app screen by name.
pub fn open_app<E: Env>(name: &str) -> Option<Box<dyn Screen<E>>> {
    Some(match name {
        "73-clock" => Box::new(clock::Clock::new()),
        "71-flashcards" => Box::new(flashcards::Decks::new()),
        "72-news" => Box::new(news::News::new()),
        "77-wikipedia" => Box::new(wikipedia::Wikipedia::new()),
        "74-weather" => Box::new(weather::Weather::new()),
        "78-calculator" => Box::new(calculator::Calculator::new()),
        "79-notes" => Box::new(notes::Notes::new()),
        "75-images" => Box::new(images::Images::new()),
        "76-fiction" => Box::new(fiction::Stories::new()),
        _ => return None,
    })
}

/// The apps list.
pub struct AppsList {
    nav: ListNav,
}

impl AppsList {
    /// New.
    pub fn new() -> Self {
        AppsList { nav: ListNav::new(APPS.len(), 9) }
    }
}

impl Default for AppsList {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for AppsList {
    fn name(&self) -> &'static str {
        "70-apps"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, "Apps", Some(&page_indicator(self.nav.page(), self.nav.pages())));
        let mut y = widgets::CONTENT_TOP;
        for i in self.nav.visible() {
            let (t, s, _) = APPS[i];
            row(f, y, row_h, t, Some(s), None, if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
            y += row_h;
        }
        rail(f, ["Games", "Back", "Open", ""], None);
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
            return Action::Replace(Box::new(super::games::GamesList::new()));
        }
        if ev.is(Key::Confirm) {
            return match open_app::<E>(APPS[self.nav.focus].2) {
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

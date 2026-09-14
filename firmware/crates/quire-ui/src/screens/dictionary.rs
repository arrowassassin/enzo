//! 27 dictionary: headword, definitions paginated, Save word, Wikipedia, next source.
//!
//! Sources are the built-in WordNet (when compiled in) followed by every StarDict
//! dictionary on the card. Each is asked at most once per word, lazily, so a card
//! dictionary that has never been opened (its sampled index takes one pass over the
//! `.idx`) costs nothing until the Dict key reaches it. The definition is wrapped and
//! paginated once per word, not per draw.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, TextStyle};

use crate::dict::{self, builtin, Dict, Entry};
use crate::net::{FetchRequest, NetEvent};
use crate::text::{line_h, page_indicator, paginate};
use crate::widgets::{self, empty_state, rail, running_head};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Screen, SysRequest};

/// Where a definition can come from.
enum Source {
    /// The compiled-in WordNet.
    Builtin,
    /// A StarDict dictionary on the card, by file stem.
    Card(String),
}

/// A source's answer for the word: the entry and the source's display name.
type Answer = Option<(Entry, String)>;

/// The dictionary screen.
pub struct Dictionary {
    word: String,
    sources: Vec<Source>,
    /// Each source's answer, filled the first time it is asked.
    answers: Vec<Option<Answer>>,
    /// The source on show; `None` when no source has the word.
    which: Option<usize>,
    headword: String,
    text: String,
    source: String,
    pages: Vec<Vec<String>>,
    page: usize,
    /// The text changed since it was last paginated.
    dirty: bool,
    /// Frame size the pages were laid out for.
    laid_out: (u32, u32),
    wiki: bool,
    loaded: bool,
}

impl Dictionary {
    /// Look a word up.
    pub fn new(word: String) -> Self {
        Dictionary {
            headword: word.clone(),
            word,
            sources: Vec::new(),
            answers: Vec::new(),
            which: None,
            text: String::new(),
            source: String::new(),
            pages: Vec::new(),
            page: 0,
            dirty: true,
            laid_out: (0, 0),
            wiki: false,
            loaded: false,
        }
    }

    /// Whether no source has the word (and no Wikipedia summary stands in).
    fn missing(&self) -> bool {
        self.which.is_none() && !self.wiki
    }

    fn load<E: Env>(&mut self, cx: &mut Ctx<E>) {
        self.loaded = true;
        let fs = cx.env.fs();
        self.sources.clear();
        if cx.env.dictionary().and_then(builtin::Blob::parse).is_some() {
            self.sources.push(Source::Builtin);
        }
        self.sources.extend(dict::list(fs).into_iter().map(Source::Card));
        self.answers = self.sources.iter().map(|_| None).collect();
        // Start from the preferred card dictionary, else the first source.
        let start = cx
            .settings
            .dictionary
            .as_ref()
            .and_then(|pref| self.sources.iter().position(|s| matches!(s, Source::Card(stem) if stem == pref)))
            .unwrap_or(0);
        match self.next_with_entry(cx, start, self.sources.len()) {
            Some(i) => self.show(i),
            None => {
                self.which = None;
                self.headword = self.word.clone();
                self.text = String::new();
                self.source = String::new();
                self.dirty = true;
            }
        }
    }

    /// Ask source `i` for the word (once), returning its answer.
    fn ask<E: Env>(&mut self, cx: &mut Ctx<E>, i: usize) -> &Answer {
        if self.answers[i].is_none() {
            let fs = cx.env.fs();
            let answer = match &self.sources[i] {
                Source::Builtin => cx
                    .env
                    .dictionary()
                    .and_then(builtin::Blob::parse)
                    .and_then(|b| b.lookup_stemmed(&self.word))
                    .map(|e| (e, String::from(builtin::NAME))),
                Source::Card(stem) => Dict::open(fs, stem).and_then(|d| d.lookup_stemmed(&self.word).map(|e| (e, d.name))),
            };
            self.answers[i] = Some(answer);
        }
        self.answers[i].as_ref().expect("answered")
    }

    /// The first of up to `count` sources from `from` (cyclically) that has the word.
    fn next_with_entry<E: Env>(&mut self, cx: &mut Ctx<E>, from: usize, count: usize) -> Option<usize> {
        let n = self.sources.len();
        (0..count.min(n)).map(|k| (from + k) % n).find(|&i| self.ask(cx, i).is_some())
    }

    /// Show source `i`'s entry (which must exist).
    fn show(&mut self, i: usize) {
        let Some(Some(Some((entry, name)))) = self.answers.get(i) else { return };
        self.which = Some(i);
        self.headword = entry.headword.clone();
        self.text = entry.text.clone();
        self.source = if self.sources.len() > 1 { alloc::format!("{name} · {} of {}", i + 1, self.sources.len()) } else { name.clone() };
        self.page = 0;
        self.wiki = false;
        self.dirty = true;
    }

    /// Wrap and paginate the text for a frame of this size, once per text.
    fn layout(&mut self, size: (u32, u32)) {
        if !self.dirty && self.laid_out == size {
            return;
        }
        let fb = quire_fonts::ui::body();
        let fl = quire_fonts::ui::label();
        let (w, h) = (size.0 as i32, size.1 as i32);
        // Between the running head and the foot line, clear of the side labels.
        let top = widgets::CONTENT_TOP + 8;
        let bottom = h - crate::theme::RAIL_H - 16 - line_h(fl) - 12;
        let lines_per_page = ((bottom - top) / line_h(fb)).max(4) as usize;
        self.pages = paginate(fb, &self.text, w - 2 * widgets::INSET - 12, lines_per_page);
        self.page = self.page.min(self.pages.len().saturating_sub(1));
        self.dirty = false;
        self.laid_out = size;
    }
}

impl<E: Env> Screen<E> for Dictionary {
    fn name(&self) -> &'static str {
        "27-dictionary"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.loaded {
            self.load(cx);
        }
        if self.missing() {
            running_head(f, &self.headword, None);
            empty_state(
                f,
                widgets::EMPTY_Y,
                &alloc::format!("No entry for “{}”", self.word),
                "Try the Wiki, or drop a StarDict dictionary into /dict.",
            );
            widgets::side_labels(f, Some("Wiki"), None, true);
            rail(f, ["", "Back", "Bookshop", "Drop"], None);
            return Refresh::Gc;
        }
        self.layout((f.width(), f.height()));
        running_head(f, &self.headword, Some(&page_indicator(self.page, self.pages.len())));
        let fb = quire_fonts::ui::body();
        let fl = quire_fonts::ui::label();
        let mut y = widgets::CONTENT_TOP + 8;
        if let Some(lines) = self.pages.get(self.page) {
            for l in lines {
                draw_text(f, fb, widgets::INSET, y + fb.ascent(), l, TextStyle::INK);
                y += line_h(fb);
            }
        }
        let foot = f.height() as i32 - crate::theme::RAIL_H - 16;
        draw_text(f, fl, widgets::INSET, foot, &self.source, TextStyle::INK);
        widgets::side_labels(f, Some("Wiki"), if self.sources.len() > 1 { Some("Dict") } else { None }, true);
        let saved = cx.settings.saved_words.iter().any(|w| w.eq_ignore_ascii_case(&self.headword));
        rail(f, ["", "Back", if saved { "Saved ✓" } else { "Save word" }, if self.pages.len() > 1 { "Next" } else { "" }], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        if !self.loaded {
            self.load(cx);
        }
        if self.missing() {
            return match ev.key {
                Key::Back => Action::Pop,
                Key::Confirm => Action::Push(Box::new(super::bookshop::BookshopHome::new())),
                Key::Right => Action::Push(Box::new(super::drop::DropScreen::new())),
                Key::Up => self.wikipedia(cx),
                _ => Action::None,
            };
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Right => {
                if self.page + 1 < self.pages.len() {
                    self.page += 1;
                } else {
                    self.page = 0;
                }
                Action::Redraw
            }
            Key::Left => {
                self.page = self.page.saturating_sub(1);
                Action::Redraw
            }
            Key::Confirm => {
                let w = self.headword.clone();
                if let Some(i) = cx.settings.saved_words.iter().position(|x| x.eq_ignore_ascii_case(&w)) {
                    cx.settings.saved_words.remove(i);
                } else if cx.settings.saved_words.len() < 500 {
                    cx.settings.saved_words.push(w);
                }
                Action::Redraw
            }
            Key::Up => self.wikipedia(cx),
            Key::Down => {
                // The next source that has the word (after a Wikipedia summary, the
                // current source again).
                let cur = self.which.unwrap_or(0);
                let (from, count) = if self.wiki { (cur, self.sources.len()) } else { (cur + 1, self.sources.len().saturating_sub(1)) };
                match self.next_with_entry(cx, from, count) {
                    Some(i) => {
                        self.show(i);
                        cx.settings.dictionary = match &self.sources[i] {
                            Source::Builtin => None,
                            Source::Card(stem) => Some(stem.clone()),
                        };
                        Action::Redraw
                    }
                    None => Action::None,
                }
            }
            Key::Power => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if let Event::Net(NetEvent::Wikipedia(res)) = ev {
            if self.wiki {
                match res {
                    Ok((title, summary)) => {
                        self.headword = title.clone();
                        self.text = summary.clone();
                    }
                    Err(e) => self.text = alloc::format!("Couldn't reach Wikipedia. {e}"),
                }
                self.page = 0;
                self.dirty = true;
                return Action::Redraw;
            }
        }
        Action::None
    }
}

impl Dictionary {
    /// Ask for the Wikipedia summary through the network layer (needs Wi-Fi).
    fn wikipedia<E: Env>(&mut self, cx: &mut Ctx<E>) -> Action<E> {
        match cx.env.wifi() {
            crate::WifiState::Connected { .. } => {
                self.headword = self.word.clone();
                self.text = String::from("Looking up on Wikipedia…");
                self.source = String::from("Wikipedia");
                self.wiki = true;
                self.page = 0;
                self.dirty = true;
                cx.env.request(SysRequest::Fetch(FetchRequest::Wikipedia(self.word.clone())));
                Action::Redraw
            }
            _ => Action::Push(Box::new(super::wifi::WifiScreen::new_with_hint("Wikipedia needs Wi-Fi"))),
        }
    }
}

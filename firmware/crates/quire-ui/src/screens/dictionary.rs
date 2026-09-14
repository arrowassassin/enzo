//! 27 dictionary: headword, definitions paginated, Save word, Wikipedia, next dictionary.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, TextStyle};

use crate::dict::{self, Dict};
use crate::net::{FetchRequest, NetEvent};
use crate::text::{line_h, page_indicator, paginate};
use crate::widgets::{self, rail, running_head};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Screen, SysRequest};

/// The dictionary screen.
pub struct Dictionary {
    word: String,
    dicts: Vec<String>,
    which: usize,
    headword: String,
    text: String,
    source: String,
    pages: Vec<Vec<String>>,
    page: usize,
    wiki: bool,
    loaded: bool,
}

impl Dictionary {
    /// Look up a word.
    pub fn new(word: String) -> Self {
        Dictionary {
            word,
            dicts: Vec::new(),
            which: 0,
            headword: String::new(),
            text: String::new(),
            source: String::new(),
            pages: Vec::new(),
            page: 0,
            wiki: false,
            loaded: false,
        }
    }
    fn load<E: Env>(&mut self, cx: &mut Ctx<E>) {
        self.loaded = true;
        let fs = cx.env.fs();
        self.dicts = dict::list(fs);
        if let Some(pref) = &cx.settings.dictionary {
            if let Some(i) = self.dicts.iter().position(|d| d == pref) {
                self.which = i;
            }
        }
        self.lookup(cx);
    }
    fn lookup<E: Env>(&mut self, cx: &mut Ctx<E>) {
        let fs = cx.env.fs();
        self.page = 0;
        self.wiki = false;
        match self.dicts.get(self.which).and_then(|stem| Dict::open(fs, stem)) {
            Some(d) => match d.lookup_stemmed(&self.word) {
                Some(e) => {
                    self.headword = e.headword;
                    self.text = e.text;
                    self.source = alloc::format!("{} · {} on card", d.name, plural(self.dicts.len(), "dictionary", "dictionaries"));
                }
                None => {
                    self.headword = self.word.clone();
                    self.text = String::from("Not in this dictionary.");
                    self.source = d.name.clone();
                }
            },
            None => {
                self.headword = self.word.clone();
                self.text = String::from("No dictionary on the card. Drop a StarDict dictionary into /dict, or get one from the Bookshop.");
                self.source = String::from("no dictionary");
            }
        }
        self.repaginate();
    }
    fn repaginate(&mut self) {
        let fb = quire_fonts::ui::body();
        let lines_per_page = ((792 - 190 - crate::theme::RAIL_H - 40) / line_h(fb)).max(4) as usize;
        self.pages = paginate(fb, &self.text, 528 - 2 * widgets::INSET, lines_per_page);
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        alloc::format!("1 {one}")
    } else {
        alloc::format!("{n} {many}")
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
        widgets::side_labels(f, Some("Wiki"), Some("Dict"), true);
        let saved = cx.settings.saved_words.iter().any(|w| w.eq_ignore_ascii_case(&self.headword));
        rail(f, ["", "Back", if saved { "Saved ✓" } else { "Save word" }, if self.pages.len() > 1 { "Next" } else { "" }], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
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
            Key::Up => {
                // Wikipedia summary through the network layer.
                match cx.env.wifi() {
                    crate::WifiState::Connected { .. } => {
                        self.text = String::from("Looking up on Wikipedia…");
                        self.source = String::from("Wikipedia");
                        self.wiki = true;
                        self.repaginate();
                        cx.env.request(SysRequest::Fetch(FetchRequest::Wikipedia(self.word.clone())));
                        Action::Redraw
                    }
                    _ => Action::Push(Box::new(super::wifi::WifiScreen::new_with_hint("Wikipedia needs Wi-Fi"))),
                }
            }
            Key::Down => {
                if self.dicts.len() > 1 {
                    self.which = (self.which + 1) % self.dicts.len();
                    cx.settings.dictionary = self.dicts.get(self.which).cloned();
                    self.lookup(cx);
                }
                Action::Redraw
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
                self.repaginate();
                return Action::Redraw;
            }
        }
        Action::None
    }
}

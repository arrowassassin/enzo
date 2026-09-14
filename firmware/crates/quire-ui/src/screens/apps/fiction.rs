//! 76 interactive fiction: Z-machine stories from `/stories`, transcript on the page, a
//! command line, verb row and noun grid on the compass, phone keyboard for the rest.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};

use crate::keyboard::KeyboardScreen;
use crate::text::{ellipsis, line_h, page_indicator, wrap};
use crate::theme::*;
use crate::widgets::{self, empty_state, rail, row, running_head, ListNav, RowState};
use crate::zmachine::{Machine, Step};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

/// Story folder.
pub const STORIES_DIR: &str = "/stories";
/// Largest story file accepted.
#[cfg(target_os = "none")]
const STORY_LIMIT: u64 = 256 * 1024;
#[cfg(not(target_os = "none"))]
const STORY_LIMIT: u64 = 512 * 1024;

const VERBS: [&str; 8] = ["look", "inventory", "north", "south", "east", "west", "up", "down"];
const MORE_VERBS: [&str; 8] = ["take", "drop", "open", "examine", "read", "wait", "again", "save"];

fn save_path(story: &str) -> String {
    alloc::format!("/.quire/stories/{story}.sav")
}

/// The story list.
pub struct Stories {
    files: Vec<String>,
    nav: ListNav,
    loaded: bool,
}

impl Stories {
    /// New.
    pub fn new() -> Self {
        Stories { files: Vec::new(), nav: ListNav::new(0, 9), loaded: false }
    }
}

impl Default for Stories {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Stories {
    fn name(&self) -> &'static str {
        "76-fiction"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.loaded {
            self.files = cx
                .env
                .fs()
                .read_dir(STORIES_DIR)
                .unwrap_or_default()
                .into_iter()
                .filter(|e| {
                    let l = e.name.to_ascii_lowercase();
                    !e.is_dir
                        && (l.ends_with(".z3") || l.ends_with(".z5") || l.ends_with(".z8") || l.ends_with(".dat"))
                        && e.size <= STORY_LIMIT
                })
                .map(|e| e.name)
                .collect();
            self.files.sort();
            self.nav.set_n(self.files.len());
            self.loaded = true;
        }
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, "Interactive fiction", Some(&page_indicator(self.nav.page(), self.nav.pages())));
        if self.files.is_empty() {
            empty_state(f, 240, "No stories yet", "Drop .z3, .z5 or .z8 story files into /stories on the card.");
        }
        let mut y = widgets::CONTENT_TOP;
        for i in self.nav.visible() {
            let saved = cx.env.fs().exists(&save_path(&self.files[i]));
            row(
                f,
                y,
                row_h,
                &self.files[i],
                None,
                Some(if saved { "saved game" } else { "" }),
                if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
            );
            y += row_h;
        }
        rail(f, ["", "Back", "Play", ""], None);
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
            if let Some(name) = self.files.get(self.nav.focus).cloned() {
                return match Play::open(cx, &name) {
                    Some(p) => Action::Push(alloc::boxed::Box::new(p)),
                    None => Action::Push(super::super::Dialog::new(
                        "Couldn't open the story",
                        "The file is not a Z-machine story version 3, 5 or 8, or it is too large.",
                        "Close",
                        "Close",
                    )),
                };
            }
            return Action::None;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
}

/// Playing a story.
pub struct Play {
    story: String,
    machine: Machine,
    /// Transcript lines (wrapped later).
    transcript: String,
    command: String,
    step: Step,
    /// Which verb page the compass shows.
    verbs_more: bool,
    /// Scrolled back this many pages (0 = the end).
    back: usize,
}

impl Play {
    /// Open a story, restoring a saved game when present.
    pub fn open<E: Env>(cx: &mut Ctx<E>, name: &str) -> Option<Self> {
        let fs = cx.env.fs();
        let bytes = fs.read_to_vec(&quire_fs::join(STORIES_DIR, name)).ok()?;
        let mut machine = Machine::new(bytes).ok()?;
        if let Ok(s) = fs.read_to_vec(&save_path(name)) {
            machine.restore(&s);
        }
        let mut p = Play {
            story: name.into(),
            machine,
            transcript: String::new(),
            command: String::new(),
            step: Step::Budget,
            verbs_more: false,
            back: 0,
        };
        p.run();
        Some(p)
    }
    fn run(&mut self) {
        self.step = self.machine.run(200_000);
        let out = self.machine.take_output();
        self.transcript.push_str(&out);
        if self.transcript.len() > 24 * 1024 {
            let cut = self.transcript.len() - 16 * 1024;
            let cut = self.transcript.floor_char_boundary(cut);
            self.transcript = String::from(&self.transcript[cut..]);
        }
    }
    fn send(&mut self, line: &str) {
        self.transcript.push_str(&alloc::format!("> {line}\n"));
        match self.step {
            Step::WaitLine => self.machine.input(line),
            Step::WaitChar => self.machine.input_char(line.chars().next().map(|c| c as u16).unwrap_or(13)),
            _ => {}
        }
        self.back = 0;
        self.run();
    }
    fn save<E: Env>(&mut self, cx: &Ctx<E>) {
        let fs = cx.env.fs();
        let _ = fs.mkdir_all("/.quire/stories");
        let data = self.machine.take_save().unwrap_or_else(|| self.machine.save());
        let _ = fs.write_atomic(&save_path(&self.story), &data);
    }
}

impl<E: Env> Screen<E> for Play {
    fn name(&self) -> &'static str {
        "76-fiction-play"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let w = f.width() as i32;
        let h = f.height() as i32;
        let fb = quire_fonts::ui::body();
        let fl = quire_fonts::ui::label();
        // Status line.
        let status = match self.machine.status_line() {
            Some((loc, a, b, is_time)) => {
                if is_time {
                    alloc::format!("{loc} · {a:02}:{b:02}")
                } else {
                    alloc::format!("{loc} · {a} · {b} turns")
                }
            }
            None => self.machine.upper_window().first().cloned().unwrap_or_else(|| self.story.clone()),
        };
        draw_text(f, fl, MARGIN, MARGIN + 14, &ellipsis(fl, &status, w - 2 * MARGIN), TextStyle::INK);
        f.fill_rect(Rect::new(MARGIN, MARGIN + 22, (w - 2 * MARGIN) as u32, 1), Ink::Black);
        // Transcript: wrap and show the last page (minus `back` pages).
        let lines = wrap(fb, &self.transcript, w - 2 * MARGIN);
        let cmd_h = 48;
        let per = ((h - RAIL_H - cmd_h - MARGIN - 40) / line_h(fb)).max(4) as usize;
        let pages = lines.len().div_ceil(per).max(1);
        let page = pages.saturating_sub(1).saturating_sub(self.back);
        let mut y = MARGIN + 34;
        for l in lines.iter().skip(page * per).take(per) {
            draw_text(f, fb, MARGIN, y + fb.ascent(), l, TextStyle::INK);
            y += line_h(fb);
        }
        // Command line.
        let cy = h - RAIL_H - cmd_h;
        widgets::text_field(
            f,
            Rect::new(MARGIN, cy, (w - 2 * MARGIN) as u32, cmd_h as u32),
            &self.command,
            "Command · Confirm for verbs · long Confirm to type",
            true,
        );
        if self.machine.halted() {
            rail(f, ["", "Back", "", "Restart"], None);
        } else {
            rail(f, ["Verbs", "Back", "Send", "Type"], None);
        }
        let _ = cx;
        Refresh::Du
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        match (ev.key, ev.kind) {
            (Key::Back, KeyKind::Press) => {
                self.save(cx);
                Action::Pop
            }
            (Key::Right, KeyKind::Press) => {
                if self.machine.halted() {
                    self.machine.restart();
                    self.transcript.clear();
                    self.run();
                    return Action::Redraw;
                }
                Action::Push(KeyboardScreen::new("Command", &self.command, "e.g. open mailbox").boxed())
            }
            (Key::Confirm, KeyKind::Long) => Action::Push(KeyboardScreen::new("Command", &self.command, "e.g. open mailbox").boxed()),
            (Key::Confirm, KeyKind::Press) => {
                if !self.command.is_empty() {
                    let c = core::mem::take(&mut self.command);
                    self.send(&c);
                    if let Some(s) = self.machine.take_save() {
                        let fs = cx.env.fs();
                        let _ = fs.mkdir_all("/.quire/stories");
                        let _ = fs.write_atomic(&save_path(&self.story), &s);
                    }
                    return Action::Redraw;
                }
                Action::Push(alloc::boxed::Box::new(VerbCompass { more: self.verbs_more }))
            }
            (Key::Left, KeyKind::Press) => Action::Push(alloc::boxed::Box::new(VerbCompass { more: self.verbs_more })),
            (Key::Up, KeyKind::Press) => {
                self.back += 1;
                Action::Redraw
            }
            (Key::Down, KeyKind::Press) => {
                self.back = self.back.saturating_sub(1);
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        match r {
            Result_::Text(t) => {
                let t = t.trim();
                if !t.is_empty() {
                    self.send(t);
                    if let Some(s) = self.machine.take_save() {
                        let fs = cx.env.fs();
                        let _ = fs.mkdir_all("/.quire/stories");
                        let _ = fs.write_atomic(&save_path(&self.story), &s);
                    }
                }
            }
            Result_::Choice(i) => {
                let verbs = if self.verbs_more { MORE_VERBS } else { VERBS };
                if i < 8 {
                    let v = verbs[i];
                    if v == "save" {
                        self.save(cx);
                        self.transcript.push_str("Saved.\n");
                    } else if matches!(v, "take" | "drop" | "open" | "examine" | "read") {
                        self.command = alloc::format!("{v} ");
                    } else {
                        self.send(v);
                    }
                } else if i == 8 {
                    self.verbs_more = !self.verbs_more;
                    return Action::Push(alloc::boxed::Box::new(VerbCompass { more: self.verbs_more }));
                }
            }
            _ => {}
        }
        Action::Redraw
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if matches!(ev, Event::Tick) && self.step == Step::Budget {
            self.run();
            return Action::Redraw;
        }
        Action::None
    }
}

/// The verb compass: six verbs at the key positions, More for the second set.
struct VerbCompass {
    more: bool,
}

impl<E: Env> Screen<E> for VerbCompass {
    fn name(&self) -> &'static str {
        "76-fiction-verbs"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, _cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let v = if self.more { MORE_VERBS } else { VERBS };
        super::super::reading::draw_compass(
            f,
            if self.more { "More verbs" } else { "Verbs" },
            "long Confirm — type a command",
            [(v[0], ""), (v[1], ""), (v[2], ""), (v[3], "")],
            v[4],
            v[5],
            None,
        );
        let fl = quire_fonts::ui::label();
        draw_text(
            f,
            fl,
            28,
            f.height() as i32 * 60 / 100 + 50 + fl.ascent(),
            &alloc::format!("Back: {} · Down again: {} · Right: {}", "close", v[6], v[7]),
            TextStyle::INK,
        );
        Refresh::Du
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::PopWith(Result_::Cancel),
            Key::Left => Action::PopWith(Result_::Choice(0)),
            Key::Confirm => Action::PopWith(Result_::Choice(2)),
            Key::Right => Action::PopWith(Result_::Choice(3)),
            Key::Up => Action::PopWith(Result_::Choice(4)),
            Key::Down => Action::PopWith(Result_::Choice(5)),
            Key::Power => Action::None,
        }
    }
}

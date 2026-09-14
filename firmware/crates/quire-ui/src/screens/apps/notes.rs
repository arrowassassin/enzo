//! 79 notes: text files in /notes, typed on the phone.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::{draw_text, Frame, TextStyle};

use crate::keyboard::KeyboardScreen;
use crate::text::{line_h, page_indicator, paginate};
use crate::theme::*;
use crate::widgets::{self, empty_state, rail, row, running_head, ListNav, RowState};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

/// Notes folder.
pub const NOTES_DIR: &str = "/notes";

/// The notes list and viewer.
pub struct Notes {
    files: Vec<String>,
    nav: ListNav,
    open: Option<(String, String)>,
    page: usize,
    loaded: bool,
}

impl Notes {
    /// New.
    pub fn new() -> Self {
        Notes { files: Vec::new(), nav: ListNav::new(0, 9), open: None, page: 0, loaded: false }
    }
    fn load<E: Env>(&mut self, cx: &Ctx<E>) {
        let fs = cx.env.fs();
        self.files = fs
            .read_dir(NOTES_DIR)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| !e.is_dir && e.name.ends_with(".txt"))
            .map(|e| e.name)
            .collect();
        self.files.sort();
        self.files.reverse();
        self.nav.set_n(self.files.len());
        self.loaded = true;
    }
}

impl Default for Notes {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Notes {
    fn name(&self) -> &'static str {
        "79-notes"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.loaded {
            self.load(cx);
        }
        let w = f.width() as i32;
        if let Some((name, text)) = &self.open {
            let fb = quire_fonts::ui::body();
            let per = ((f.height() as i32 - RAIL_H - widgets::CONTENT_TOP - 8) / line_h(fb)).max(4) as usize;
            let pages = paginate(fb, text, w - 2 * widgets::INSET, per);
            running_head(f, name.trim_end_matches(".txt"), Some(&page_indicator(self.page, pages.len())));
            let mut y = widgets::CONTENT_TOP;
            for l in pages.get(self.page).into_iter().flatten() {
                draw_text(f, fb, widgets::INSET, y + fb.ascent(), l, TextStyle::INK);
                y += line_h(fb);
            }
            rail(f, ["", "Back", "Edit", "Next"], None);
            return Refresh::Gc;
        }
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, "Notes", Some(&page_indicator(self.nav.page(), self.nav.pages())));
        if self.files.is_empty() {
            empty_state(f, 240, "No notes yet", "Right starts one; type it on your phone.");
        }
        let mut y = widgets::CONTENT_TOP;
        for i in self.nav.visible() {
            row(
                f,
                y,
                row_h,
                self.files[i].trim_end_matches(".txt"),
                None,
                None,
                if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
            );
            y += row_h;
        }
        rail(f, ["", "Back", "Open", "New"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if let Some((name, text)) = &self.open {
            return match ev.key {
                Key::Back => {
                    self.open = None;
                    Action::Redraw
                }
                Key::Right => {
                    self.page += 1;
                    Action::Redraw
                }
                Key::Left => {
                    self.page = self.page.saturating_sub(1);
                    Action::Redraw
                }
                Key::Confirm => Action::Push(KeyboardScreen::new(name.trim_end_matches(".txt"), text, "").boxed()),
                _ => Action::None,
            };
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        if ev.is(Key::Right) {
            let now = cx.env.now();
            let day = quire_library::time::day_of(now);
            let name = alloc::format!(
                "{}-{}.txt",
                quire_library::time::fmt_date_year(day).replace(' ', "-"),
                quire_library::time::fmt_clock(now, true).replace(':', "")
            );
            self.open = Some((name, String::new()));
            return Action::Push(KeyboardScreen::new("New note", "", "Type on your phone").boxed());
        }
        if ev.is(Key::Confirm) {
            if let Some(name) = self.files.get(self.nav.focus).cloned() {
                let text = cx
                    .env
                    .fs()
                    .read_to_vec(&quire_fs::join(NOTES_DIR, &name))
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default();
                self.open = Some((name, text));
                self.page = 0;
            }
            return Action::Redraw;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let (Result_::Text(t), Some((name, _))) = (r, self.open.clone()) {
            let fs = cx.env.fs();
            let _ = fs.mkdir_all(NOTES_DIR);
            let _ = fs.write_atomic(&quire_fs::join(NOTES_DIR, &name), t.as_bytes());
            self.open = Some((name, t));
            self.loaded = false;
        }
        Action::Redraw
    }
}

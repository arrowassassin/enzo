//! 13 folders: the SD browser with a breadcrumb in mono.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::{DirEntry, Fs};
use quire_gfx::{draw_text, Frame, TextStyle};

use crate::text::{ellipsis, page_indicator};
use crate::theme::*;
use crate::widgets::{self, empty_state, rail, row, running_head, ListNav, RowState};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Screen};

/// The folder browser.
pub struct Folders {
    path: String,
    entries: Vec<DirEntry>,
    nav: ListNav,
    loaded: bool,
}

impl Folders {
    /// New, at the card root.
    pub fn new() -> Self {
        Folders { path: String::from("/"), entries: Vec::new(), nav: ListNav::new(0, 8), loaded: false }
    }
    fn load<E: Env>(&mut self, cx: &Ctx<E>) {
        self.entries = cx.env.fs().read_dir(&self.path).unwrap_or_default();
        self.entries.retain(|e| !e.name.starts_with('.') && e.name != "System Volume Information");
        self.entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        self.nav.set_n(self.entries.len());
        self.nav.focus = 0;
        self.loaded = true;
    }
}

impl Default for Folders {
    fn default() -> Self {
        Self::new()
    }
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        alloc::format!("{}.{} MB", bytes / (1024 * 1024), (bytes % (1024 * 1024)) * 10 / (1024 * 1024))
    } else {
        alloc::format!("{} KB", bytes.div_ceil(1024))
    }
}

impl<E: Env> Screen<E> for Folders {
    fn name(&self) -> &'static str {
        "13-folders"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.loaded {
            self.load(cx);
        }
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP + 32, f.height() as i32 - RAIL_H, row_h);
        running_head(f, "Folders", Some(&page_indicator(self.nav.page(), self.nav.pages())));
        let mono = quire_fonts::ui::mono();
        let crumb = if self.path == "/" { String::from("/") } else { alloc::format!("{}/", self.path) };
        draw_text(
            f,
            mono,
            widgets::INSET,
            widgets::CONTENT_TOP + mono.ascent(),
            &ellipsis(mono, &crumb, f.width() as i32 - 2 * widgets::INSET),
            TextStyle::INK,
        );
        let mut y = widgets::CONTENT_TOP + 32;
        let has_parent = self.path != "/";
        let offset = if has_parent { 1 } else { 0 };
        let n = self.entries.len() + offset;
        self.nav.set_n(n);
        if n == 0 {
            empty_state(f, y + 100, "Empty folder", "Drop books here from your phone.");
        }
        for i in self.nav.visible() {
            let focused = i == self.nav.focus;
            let state = if focused { RowState::Focused } else { RowState::Normal };
            if has_parent && i == 0 {
                row(f, y, row_h, "../", None, None, state);
            } else {
                let e = &self.entries[i - offset];
                if e.is_dir {
                    let count = cx.env.fs().read_dir(&quire_fs::join(&self.path, &e.name)).map(|v| v.len()).unwrap_or(0);
                    row(f, y, row_h, &alloc::format!("{}/", e.name), None, Some(&alloc::format!("{count} files")), state);
                } else {
                    row(f, y, row_h, &e.name, None, Some(&size_text(e.size)), state);
                }
            }
            y += row_h;
        }
        rail(f, ["", "Back", "Open", ""], None);
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
            let has_parent = self.path != "/";
            if has_parent && self.nav.focus == 0 {
                self.path = String::from(quire_fs::parent(&self.path));
                if self.path.is_empty() {
                    self.path = String::from("/");
                }
                self.load(cx);
                return Action::Redraw;
            }
            let idx = self.nav.focus - if has_parent { 1 } else { 0 };
            let Some(e) = self.entries.get(idx).cloned() else { return Action::None };
            let full = quire_fs::join(&self.path, &e.name);
            if e.is_dir {
                self.path = full;
                self.load(cx);
                return Action::Redraw;
            }
            // A file: open it if it is a book (scanning the card picks it up).
            if let Some(b) = cx.lib.by_path(&full) {
                return Action::Open(b.id);
            }
            let _ = quire_library::scan(cx.env.fs(), cx.lib, cx.env.now());
            if let Some(b) = cx.lib.by_path(&full).map(|b| b.id) {
                cx.env.request(crate::SysRequest::IngestNow);
                return Action::Push(alloc::boxed::Box::new(super::bookinfo::BookInfo::new(b)));
            }
            return Action::None;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
}

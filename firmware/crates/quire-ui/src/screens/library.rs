//! 11 library: tabs Recent · All · Authors · Series · Collections · Folders; grid or list;
//! long-Confirm compass with Open · Info · Finished · Collection, side Delete / Move.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::{Frame, Ink, Rect};
use quire_library::time::fmt_duration;
use quire_library::{cache, BookEntry, BookId, IngestState, Status};

use crate::screens::reading::draw_compass;
use crate::text::page_indicator;
use crate::theme::*;
use crate::widgets::{self, cover_cell, empty_state, rail, row_thumb, running_head, tabs, ListNav, RowState};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen, SysRequest};

const TABS: [&str; 6] = ["Recent", "All", "Authors", "Series", "Collections", "Folders"];

/// The library screen.
pub struct LibraryScreen {
    tab: usize,
    nav: ListNav,
    /// Focus is on the tab line.
    on_tabs: bool,
    /// For Authors/Series/Collections: the group opened, if any.
    group: Option<String>,
    thumbs: Vec<(BookId, quire_gfx::Bitmap)>,
}

/// A row of the current tab: a book, or a group header row.
enum Item {
    Book(BookId),
    Group(String, usize),
}

impl LibraryScreen {
    /// New, on the last-used tab.
    pub fn new() -> Self {
        LibraryScreen { tab: 0, nav: ListNav::new(0, 6), on_tabs: false, group: None, thumbs: Vec::new() }
    }

    fn items<E: Env>(&self, cx: &Ctx<E>) -> Vec<Item> {
        let lib = &*cx.lib;
        match (self.tab, &self.group) {
            (0, _) => lib.shelf().iter().map(|b| Item::Book(b.id)).collect(),
            (1, _) => lib.by_title().iter().map(|b| Item::Book(b.id)).collect(),
            (2, None) => lib.authors().into_iter().map(|(n, v)| Item::Group(n, v.len())).collect(),
            (2, Some(g)) => lib
                .authors()
                .into_iter()
                .find(|(n, _)| n == g)
                .map(|(_, v)| v.iter().map(|b| Item::Book(b.id)).collect())
                .unwrap_or_default(),
            (3, None) => lib.series().into_iter().map(|(n, v)| Item::Group(n, v.len())).collect(),
            (3, Some(g)) => lib
                .series()
                .into_iter()
                .find(|(n, _)| n == g)
                .map(|(_, v)| v.iter().map(|b| Item::Book(b.id)).collect())
                .unwrap_or_default(),
            (4, None) => lib.collections.iter().map(|c| Item::Group(c.name.clone(), lib.in_collection(c.id).len())).collect(),
            (4, Some(g)) => lib
                .collections
                .iter()
                .find(|c| c.name == *g)
                .map(|c| lib.in_collection(c.id).iter().map(|b| Item::Book(b.id)).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn ensure_thumbs<E: Env>(&mut self, cx: &mut Ctx<E>, ids: &[BookId]) {
        self.thumbs.retain(|(id, _)| ids.contains(id));
        for id in ids {
            if self.thumbs.iter().any(|(i, _)| i == id) {
                continue;
            }
            if let Some(bm) = cache::load_thumb(cx.env.fs(), *id) {
                self.thumbs.push((*id, bm));
            }
        }
    }

    fn grid<E: Env>(&self, cx: &Ctx<E>) -> bool {
        cx.settings.library_grid && matches!(self.tab, 0 | 1) || (self.group.is_some() && cx.settings.library_grid)
    }
}

impl Default for LibraryScreen {
    fn default() -> Self {
        Self::new()
    }
}

fn value_for<E: Env>(cx: &Ctx<E>, b: &BookEntry) -> String {
    match b.ingest {
        IngestState::Pending => {
            if let Some((_, d, t)) = cx.ingesting.iter().find(|(i, _, _)| *i == b.id) {
                return if *t > 0 { alloc::format!("{}%", *d as u64 * 100 / *t as u64) } else { String::from("preparing") };
            }
            String::from("waiting")
        }
        IngestState::Failed => String::from("couldn't open"),
        IngestState::Ready => {
            if b.status == Status::Finished {
                String::from("finished")
            } else {
                fmt_duration(cx.stats.time_left_secs(b))
            }
        }
    }
}

impl<E: Env> Screen<E> for LibraryScreen {
    fn name(&self) -> &'static str {
        "11-library"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if self.tab == 5 {
            // Folders is its own screen; draw a redirect-free version by delegating.
            self.tab = cx.settings.library_tab.min(4) as usize;
        }
        let items = self.items(cx);
        let grid = self.grid(cx);
        let per = if grid { 6 } else { widgets::rows_between(widgets::CONTENT_TOP + 36, f.height() as i32 - RAIL_H, ROW_THUMB_H) };
        self.nav.per_page = per;
        self.nav.set_n(items.len());
        let title = match &self.group {
            Some(g) => g.clone(),
            None => String::from("Library"),
        };
        running_head(f, &title, None);
        let y = tabs(f, widgets::CONTENT_TOP - 8, &TABS, self.tab, self.on_tabs, Some(&page_indicator(self.nav.page(), self.nav.pages())));
        if items.is_empty() {
            let (line, hint) = match self.tab {
                4 => ("No collections yet", "Long-press Confirm on a book to add it to one."),
                3 => ("No series yet", "Series come from the books' metadata."),
                _ => ("Nothing here yet", "Books you add appear here."),
            };
            empty_state(f, y + 120, line, hint);
            rail(f, ["", "Back", "Bookshop", "Drop"], None);
            return Refresh::Gc;
        }
        let ids: Vec<BookId> = self.nav.visible().filter_map(|i| if let Item::Book(id) = items[i] { Some(id) } else { None }).collect();
        self.ensure_thumbs(cx, &ids);
        if grid {
            let w = f.width() as i32;
            let gap = (w - 2 * widgets::INSET - 3 * COVER_W as i32) / 2;
            for (k, i) in self.nav.visible().enumerate() {
                let Item::Book(id) = items[i] else { continue };
                let Some(b) = cx.lib.get(id) else { continue };
                let (c, r) = (k % 3, k / 3);
                let x = widgets::INSET + c as i32 * (COVER_W as i32 + gap);
                let yy = y + 12 + r as i32 * (COVER_H as i32 + 64);
                let thumb = self.thumbs.iter().find(|(t, _)| *t == id).map(|(_, bm)| bm.as_ref());
                cover_cell(f, x, yy, thumb, &b.title, &b.author_line(), i == self.nav.focus && !self.on_tabs, Some(b.percent()));
            }
        } else {
            let mut yy = y + 4;
            for i in self.nav.visible() {
                let focused = i == self.nav.focus && !self.on_tabs;
                match &items[i] {
                    Item::Book(id) => {
                        let Some(b) = cx.lib.get(*id) else { continue };
                        let v = value_for(cx, b);
                        let thumb = self.thumbs.iter().find(|(t, _)| t == id).map(|(_, bm)| bm.as_ref());
                        let state = if focused {
                            RowState::Focused
                        } else if b.ingest != IngestState::Ready {
                            RowState::Disabled
                        } else {
                            RowState::Normal
                        };
                        row_thumb(f, yy, thumb, &b.title, &b.author_line(), Some(&v), state);
                        yy += ROW_THUMB_H;
                    }
                    Item::Group(name, n) => {
                        let v = alloc::format!("{n} books");
                        widgets::row(f, yy, ROW_THUMB_H, name, None, Some(&v), if focused { RowState::Focused } else { RowState::Normal });
                        yy += ROW_THUMB_H;
                    }
                }
            }
        }
        rail(f, ["", "Back", "Open", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        let items = self.items(cx);
        if ev.is(Key::Back) {
            if self.group.is_some() {
                self.group = None;
                self.nav.focus = 0;
                return Action::Redraw;
            }
            return Action::Pop;
        }
        if self.on_tabs {
            match ev.key {
                Key::Left => self.tab = (self.tab + TABS.len() - 1) % TABS.len(),
                Key::Right => self.tab = (self.tab + 1) % TABS.len(),
                Key::Down | Key::Confirm => {
                    if self.tab == 5 {
                        self.tab = cx.settings.library_tab.min(4) as usize;
                        return Action::Push(Box::new(super::folders::Folders::new()));
                    }
                    self.on_tabs = false;
                }
                _ => return Action::None,
            }
            if self.tab != 5 {
                cx.settings.library_tab = self.tab as u8;
            }
            self.group = None;
            self.nav.focus = 0;
            return Action::Redraw;
        }
        if ev.is_long(Key::Confirm) {
            if let Some(Item::Book(id)) = items.get(self.nav.focus) {
                return Action::Push(Box::new(BookCompass::new(*id)));
            }
            // Long-Confirm elsewhere toggles grid/list.
            cx.settings.library_grid = !cx.settings.library_grid;
            return Action::Redraw;
        }
        if ev.is(Key::Confirm) {
            return match items.get(self.nav.focus) {
                Some(Item::Book(id)) => {
                    let ready = cx.lib.get(*id).map(|b| b.ingest == IngestState::Ready).unwrap_or(false);
                    if ready {
                        Action::Open(*id)
                    } else {
                        cx.env.request(SysRequest::IngestNow);
                        Action::Push(Box::new(super::bookinfo::BookInfo::new(*id)))
                    }
                }
                Some(Item::Group(name, _)) => {
                    self.group = Some(name.clone());
                    self.nav.focus = 0;
                    Action::Redraw
                }
                None => Action::None,
            };
        }
        if ev.key == Key::Up && self.nav.focus < (if self.grid(cx) { 3 } else { 1 }) && !self.on_tabs && self.nav.page() == 0 {
            self.on_tabs = true;
            return Action::Redraw;
        }
        if self.grid(cx) {
            // Grid: Left/Right move within the row, Up/Down by rows, paging at the edges.
            let n = self.nav.n;
            if n > 0 {
                match ev.key {
                    Key::Left => self.nav.focus = (self.nav.focus + n - 1) % n,
                    Key::Right => self.nav.focus = (self.nav.focus + 1) % n,
                    Key::Up => self.nav.focus = self.nav.focus.saturating_sub(3),
                    Key::Down => self.nav.focus = (self.nav.focus + 3).min(n - 1),
                    _ => return Action::None,
                }
                return Action::Redraw;
            }
            return Action::None;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::Ingest { .. } | Event::BooksChanged => Action::Redraw,
            _ => Action::None,
        }
    }
    fn result(&mut self, _cx: &mut Ctx<E>, _r: Result_) -> Action<E> {
        Action::Redraw
    }
}

/// The book compass in the library: Open · Info · Finished · Collection; side Delete / Move.
pub struct BookCompass {
    id: BookId,
}

impl BookCompass {
    /// New.
    pub fn new(id: BookId) -> Self {
        BookCompass { id }
    }
}

impl<E: Env> Screen<E> for BookCompass {
    fn name(&self) -> &'static str {
        "11-library-compass"
    }
    fn overlay(&self) -> bool {
        true
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let Some(b) = cx.lib.get(self.id) else { return Refresh::Du };
        let page = b.pages_total.map(|(_, p)| alloc::format!("p {}", ((p as u64 * b.percent() as u64) / 100).max(1))).unwrap_or_default();
        let context = alloc::format!("{} · {}% · {page}", b.title, b.percent());
        let fin = if b.status == Status::Finished { "Unfinish" } else { "Finished" };
        let left = fmt_duration(cx.stats.time_left_secs(b));
        let colls = alloc::format!("{}", b.collections.len());
        draw_compass(
            f,
            &context,
            "hold Confirm — open at the start",
            [("Open", &left), ("Close", "—"), (fin, "mark"), ("Collection", &colls)],
            "Delete",
            "Move",
            None,
        );
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        let today = cx.today();
        match (ev.key, ev.kind) {
            (Key::Back, KeyKind::Press) => Action::Pop,
            (Key::Left, KeyKind::Press) => Action::Open(self.id),
            (Key::Confirm, KeyKind::Long) => {
                cx.lib.set_loc(self.id, quire_library::Loc::default());
                Action::Open(self.id)
            }
            (Key::Confirm, KeyKind::Press) => {
                let finished = cx.lib.get(self.id).map(|b| b.status == Status::Finished).unwrap_or(false);
                cx.lib.set_finished(self.id, !finished, today);
                Action::Pop
            }
            (Key::Right, KeyKind::Press) => Action::Replace(Box::new(CollectionPicker::new(self.id))),
            (Key::Up, KeyKind::Press) => {
                let title = cx.lib.get(self.id).map(|b| b.title.clone()).unwrap_or_default();
                Action::Push(super::Dialog::new(
                    "Delete this book?",
                    &alloc::format!("{title} and its bookmarks will be removed from the card."),
                    "Cancel",
                    "Delete",
                ))
            }
            (Key::Down, KeyKind::Press) => Action::Replace(Box::new(super::bookinfo::BookInfo::new(self.id))),
            _ => Action::None,
        }
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if r == Result_::Choice(1) {
            let path = cx.lib.get(self.id).map(|b| b.path.clone());
            if cx.reader.as_ref().map(|r| r.id) == Some(self.id) {
                *cx.reader = None;
                cx.lib.current = None;
                cx.lib.touch();
            }
            quire_library::forget_book(cx.env.fs(), cx.lib, self.id);
            if let Some(p) = path {
                let _ = cx.env.fs().remove(&p);
            }
            let _ = cx.lib.save(cx.env.fs());
            return Action::Pop;
        }
        Action::Redraw
    }
}

/// Toggle a book's collections; Right creates a new one.
pub struct CollectionPicker {
    id: BookId,
    nav: ListNav,
}

impl CollectionPicker {
    /// New.
    pub fn new(id: BookId) -> Self {
        CollectionPicker { id, nav: ListNav::new(0, 10) }
    }
}

impl<E: Env> Screen<E> for CollectionPicker {
    fn name(&self) -> &'static str {
        "11-collections"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Collections", None);
        let colls: Vec<(u16, String)> = cx.lib.collections.iter().map(|c| (c.id, c.name.clone())).collect();
        self.nav.set_n(colls.len());
        let member: Vec<u16> = cx.lib.get(self.id).map(|b| b.collections.clone()).unwrap_or_default();
        let mut y = widgets::CONTENT_TOP;
        for i in self.nav.visible() {
            let (cid, name) = &colls[i];
            let v = if member.contains(cid) { "✓" } else { "" };
            widgets::row(f, y, ROW_H, name, None, Some(v), if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
            y += ROW_H;
        }
        if colls.is_empty() {
            empty_state(f, y + 80, "No collections yet", "Right makes a new one.");
        }
        f.fill_rect(Rect::new(0, y, 1, 1), Ink::White);
        rail(f, ["", "Back", "Toggle", "New"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Right => Action::Push(crate::keyboard::KeyboardScreen::new("New collection", "", "Name").boxed()),
            Key::Confirm => {
                if let Some(c) = cx.lib.collections.get(self.nav.focus).map(|c| c.id) {
                    cx.lib.toggle_collection(self.id, c);
                }
                Action::Redraw
            }
            _ => {
                self.nav.key(ev);
                Action::Redraw
            }
        }
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let Result_::Text(t) = r {
            if !t.trim().is_empty() {
                let c = cx.lib.add_collection(t.trim());
                cx.lib.toggle_collection(self.id, c);
            }
        }
        Action::Redraw
    }
}

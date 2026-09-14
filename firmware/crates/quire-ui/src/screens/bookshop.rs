//! 35–39 the Bookshop (offline catalog, shelves, book page, search, browse, downloads),
//! 32 OPDS catalogs, 33 Calibre connect, 34 sync.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use serde::{Deserialize, Serialize};

use crate::keyboard::KeyboardScreen;
use crate::net::{DownloadState, FetchRequest, NetEvent, OpdsEntry};
use crate::text::{draw_label, ellipsis, line_h, page_indicator, paginate, wrap};
use crate::theme::*;
use crate::widgets::{self, empty_state, poster_tiles, rail, row, running_head, setting_row, stepped_bar, ListNav, RowState, SettingValue};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen, SysRequest, WifiState};

/// Three picks shown on the empty home: (title, author, hours).
pub const START_HERE: [(&str, &str, &str); 3] = [
    ("Pride and Prejudice", "Jane Austen", "6 h"),
    ("The Adventures of Sherlock Holmes", "Arthur Conan Doyle", "5 h"),
    ("Walden", "Henry David Thoreau", "8 h"),
];

/// Where the offline catalog lives on the card.
pub const CATALOG_FILE: &str = "/.quire/bookshop/catalog.bin";
/// Saved-for-later list.
pub const SAVED_FILE: &str = "/.quire/bookshop/saved.bin";

/// One catalog book.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopBook {
    /// Stable id ("pg1342", "se/…").
    pub id: String,
    /// Title.
    pub title: String,
    /// Author.
    pub author: String,
    /// Year.
    pub year: u16,
    /// Language code.
    pub lang: String,
    /// Subjects.
    pub subjects: Vec<String>,
    /// Source name ("Project Gutenberg", "Standard Ebooks", "Creative Commons").
    pub source: String,
    /// Licence line.
    pub licence: String,
    /// Download URL (EPUB).
    pub url: String,
    /// Size in bytes.
    pub size: u32,
    /// Popularity rank (lower is more popular).
    pub rank: u32,
    /// Estimated reading hours × 10.
    pub hours10: u16,
    /// Blurb.
    pub blurb: String,
    /// Collection name, if part of a curated collection.
    pub collection: Option<String>,
    /// Modern (post-1928) or Creative Commons.
    pub modern: bool,
    /// Days since epoch the edition was added.
    pub added: u16,
}

/// The catalog.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    /// Books.
    pub books: Vec<ShopBook>,
    /// When it was fetched.
    pub fetched: u32,
}

/// A built-in seed so the shop works before any catalog download.
pub fn seed() -> Catalog {
    let mk =
        |id: &str, title: &str, author: &str, year: u16, subj: &str, url: &str, size: u32, rank: u32, hours10: u16, blurb: &str| ShopBook {
            id: id.into(),
            title: title.into(),
            author: author.into(),
            year,
            lang: "en".into(),
            subjects: subj.split(',').map(|s| String::from(s.trim())).collect(),
            source: "Project Gutenberg".into(),
            licence: "Public domain".into(),
            url: url.into(),
            size,
            rank,
            hours10,
            blurb: blurb.into(),
            collection: Some("Start here".into()),
            modern: false,
            added: 0,
        };
    Catalog {
        fetched: 0,
        books: alloc::vec![
            mk("pg1342", "Pride and Prejudice", "Jane Austen", 1813, "romance, classics", "https://www.gutenberg.org/ebooks/1342.epub3.images", 720_000, 1, 60, "Elizabeth Bennet and Mr Darcy misjudge each other, and slowly learn better, in the best-loved comedy of manners in English."),
            mk("pg1661", "The Adventures of Sherlock Holmes", "Arthur Conan Doyle", 1892, "mystery, short stories", "https://www.gutenberg.org/ebooks/1661.epub3.images", 640_000, 2, 50, "Twelve cases from Baker Street, from A Scandal in Bohemia to The Copper Beeches."),
            mk("pg205", "Walden", "Henry David Thoreau", 1854, "nature, philosophy", "https://www.gutenberg.org/ebooks/205.epub3.images", 520_000, 3, 80, "Two years in a cabin by a pond, and what a deliberate life costs and gives."),
            mk("pg2701", "Moby-Dick", "Herman Melville", 1851, "adventure, classics", "https://www.gutenberg.org/ebooks/2701.epub3.images", 1_400_000, 4, 180, "Call me Ishmael. A whaling voyage, a wounded captain, and the white whale."),
            mk("pg84", "Frankenstein", "Mary Shelley", 1818, "gothic, science fiction", "https://www.gutenberg.org/ebooks/84.epub3.images", 480_000, 5, 55, "A young scientist creates a living being and abandons it."),
            mk("pg1232", "The Prince", "Niccolò Machiavelli", 1532, "politics, philosophy", "https://www.gutenberg.org/ebooks/1232.epub3.images", 260_000, 6, 30, "How princes gain and keep power, told without illusions."),
            mk("pg11", "Alice's Adventures in Wonderland", "Lewis Carroll", 1865, "fantasy, children", "https://www.gutenberg.org/ebooks/11.epub3.images", 340_000, 7, 25, "Down the rabbit hole with the Cheshire Cat, the Hatter and the Queen of Hearts."),
            mk("pg2600", "War and Peace", "Leo Tolstoy", 1869, "historical, classics", "https://www.gutenberg.org/ebooks/2600.epub3.images", 3_300_000, 8, 350, "Five families and Napoleon's invasion of Russia."),
            mk("pg145", "Middlemarch", "George Eliot", 1871, "classics", "https://www.gutenberg.org/ebooks/145.epub3.images", 1_500_000, 9, 210, "A study of provincial life: Dorothea, Lydgate, and the town that shapes them."),
            mk("pg1400", "Great Expectations", "Charles Dickens", 1861, "classics", "https://www.gutenberg.org/ebooks/1400.epub3.images", 1_000_000, 10, 130, "Pip, Estella, Magwitch and the fortune that was not what it seemed."),
            mk("pg2814", "Dubliners", "James Joyce", 1914, "short stories", "https://www.gutenberg.org/ebooks/2814.epub3.images", 380_000, 11, 45, "Fifteen stories of a city and its paralysis, ending with The Dead."),
            mk("pg174", "The Picture of Dorian Gray", "Oscar Wilde", 1890, "gothic, classics", "https://www.gutenberg.org/ebooks/174.epub3.images", 520_000, 12, 50, "A portrait ages so that its subject need not."),
            mk("pg35", "The Time Machine", "H. G. Wells", 1895, "science fiction", "https://www.gutenberg.org/ebooks/35.epub3.images", 220_000, 13, 20, "A traveller into the year 802,701 and beyond."),
            mk("pg2680", "Meditations", "Marcus Aurelius", 180, "philosophy", "https://www.gutenberg.org/ebooks/2680.epub3.images", 360_000, 14, 35, "The private notebook of a Roman emperor on how to live."),
            mk("pg43", "The Strange Case of Dr Jekyll and Mr Hyde", "Robert Louis Stevenson", 1886, "gothic, mystery", "https://www.gutenberg.org/ebooks/43.epub3.images", 200_000, 15, 15, "A lawyer investigates his friend's sinister associate."),
            mk("pg1184", "The Count of Monte Cristo", "Alexandre Dumas", 1846, "adventure, revenge", "https://www.gutenberg.org/ebooks/1184.epub3.images", 2_800_000, 16, 300, "Wrongly imprisoned, Edmond Dantès escapes with a fortune and a plan."),
            mk("pg98", "A Tale of Two Cities", "Charles Dickens", 1859, "historical, classics", "https://www.gutenberg.org/ebooks/98.epub3.images", 800_000, 17, 100, "London and Paris in the years of the Revolution."),
            mk("pg1952", "The Yellow Wallpaper", "Charlotte Perkins Gilman", 1892, "short stories", "https://www.gutenberg.org/ebooks/1952.epub3.images", 90_000, 18, 5, "A rest cure, a room, and the pattern on the wall."),
            mk("pg244", "A Study in Scarlet", "Arthur Conan Doyle", 1887, "mystery", "https://www.gutenberg.org/ebooks/244.epub3.images", 300_000, 19, 25, "Holmes and Watson meet, and a body is found in Lauriston Gardens."),
            mk("pg2148", "The Works of Edgar Allan Poe, Volume 2", "Edgar Allan Poe", 1845, "horror, short stories", "https://www.gutenberg.org/ebooks/2148.epub3.images", 700_000, 20, 60, "The Raven, The Tell-Tale Heart, and the rest of the second volume."),
        ],
    }
}

/// Load the catalog from the card, or the seed.
pub fn load_catalog<F: Fs>(fs: &F) -> Catalog {
    fs.read_to_vec(CATALOG_FILE)
        .ok()
        .and_then(|b| postcard::from_bytes::<Catalog>(&b).ok())
        .filter(|c| !c.books.is_empty())
        .unwrap_or_else(seed)
}

/// Saved-for-later ids.
pub fn load_saved<F: Fs>(fs: &F) -> Vec<String> {
    fs.read_to_vec(SAVED_FILE).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default()
}

fn save_saved<F: Fs>(fs: &F, saved: &[String]) {
    if let Ok(b) = postcard::to_allocvec(&saved.to_vec()) {
        let _ = fs.mkdir_all("/.quire/bookshop");
        let _ = fs.write_atomic(SAVED_FILE, &b);
    }
}

fn hours_text(h10: u16) -> String {
    if h10 < 10 {
        alloc::format!("{} min", h10 as u32 * 6)
    } else {
        alloc::format!("{} h", h10 / 10)
    }
}

/// Whether a catalog book is already in the library (by title and author).
fn in_library<E: Env>(cx: &Ctx<E>, b: &ShopBook) -> Option<quire_library::BookId> {
    cx.lib
        .books
        .iter()
        .find(|x| {
            !x.missing
                && x.title.eq_ignore_ascii_case(&b.title)
                && x.authors.first().map(|a| a.eq_ignore_ascii_case(&b.author)).unwrap_or(false)
        })
        .map(|x| x.id)
}

/// A shelf: name and the ids on it.
fn shelves(cat: &Catalog, lang: &str) -> Vec<(String, Vec<usize>)> {
    let lang_ok = |b: &ShopBook| b.lang == lang || lang.is_empty();
    let mut by_rank: Vec<usize> = (0..cat.books.len()).filter(|i| lang_ok(&cat.books[*i])).collect();
    by_rank.sort_by_key(|i| cat.books[*i].rank);
    let start: Vec<usize> = by_rank.iter().copied().filter(|i| cat.books[*i].collection.as_deref() == Some("Start here")).take(6).collect();
    let popular: Vec<usize> = by_rank.iter().copied().take(12).collect();
    let mut newest: Vec<usize> = by_rank.clone();
    newest.sort_by(|a, b| cat.books[*b].added.cmp(&cat.books[*a].added));
    let newest: Vec<usize> = newest.into_iter().take(12).collect();
    let modern: Vec<usize> = by_rank.iter().copied().filter(|i| cat.books[*i].modern).take(12).collect();
    let mut colls: Vec<String> = cat.books.iter().filter_map(|b| b.collection.clone()).collect();
    colls.sort();
    colls.dedup();
    let collections: Vec<usize> = by_rank.iter().copied().filter(|i| cat.books[*i].collection.is_some()).take(12).collect();
    let subjects: Vec<usize> = by_rank.iter().copied().take(12).collect();
    alloc::vec![
        (String::from("Start here"), start),
        (String::from("Popular this week"), popular),
        (String::from("New editions"), newest),
        (String::from("Modern & Creative Commons"), modern),
        (String::from("Collections"), collections),
        (String::from("By subject"), subjects),
    ]
}

// ---------------------------------------------------------------------------------------
// 35 home

/// The Bookshop home: six shelves of three covers plus More.
pub struct BookshopHome {
    cat: Option<Catalog>,
    focus: (usize, usize),
    first: bool,
}

impl BookshopHome {
    /// New.
    pub fn new() -> Self {
        BookshopHome { cat: None, focus: (0, 0), first: true }
    }
    fn ensure<E: Env>(&mut self, cx: &Ctx<E>) {
        if self.cat.is_none() {
            let c = load_catalog(cx.env.fs());
            self.first = c.fetched == 0;
            self.cat = Some(c);
        }
    }
}

impl Default for BookshopHome {
    fn default() -> Self {
        Self::new()
    }
}

/// Draw a small typographic cover cell (96 × 132) for a catalog book.
fn shop_cover(f: &mut Frame, x: i32, y: i32, b: &ShopBook, focused: bool) {
    let r = Rect::new(x, y, 96, 132);
    widgets::typographic_cover(f, r, &b.title, &b.author);
    f.stroke_rect(r, if focused { 4 } else { 1 }, Ink::Black);
}

impl<E: Env> Screen<E> for BookshopHome {
    fn name(&self) -> &'static str {
        "35-bookshop"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        self.ensure(cx);
        let cat = self.cat.clone().unwrap_or_default();
        running_head(f, "Bookshop", None);
        let w = f.width() as i32;
        let fl = quire_fonts::ui::label();
        let mut y = widgets::CONTENT_TOP - 4;
        // Search line.
        let sr = Rect::new(widgets::INSET, y, (w - 2 * widgets::INSET) as u32, 36);
        widgets::text_field(f, sr, "", "Search · type on your phone", self.focus.0 == 0);
        y += 44;
        if self.first {
            draw_text(f, fl, widgets::INSET, y + fl.ascent(), "Free, open books. No account, no DRM.", TextStyle::INK);
            y += line_h(fl) + 6;
        }
        let shelves = shelves(&cat, &cx.settings.bookshop_language);
        let shelf_h = (f.height() as i32 - RAIL_H - y) / 3;
        let per_row = 3;
        let visible_start = ((self.focus.0.saturating_sub(1)) / 3) * 3;
        for (si, (name, ids)) in shelves.iter().enumerate().skip(visible_start).take(3) {
            let sy = y + (si - visible_start) as i32 * shelf_h;
            draw_label(f, widgets::INSET, sy + fl.ascent(), name, false);
            let cy = sy + line_h(fl) + 4;
            let cell_w = 96;
            let gap = (w - 2 * widgets::INSET - per_row as i32 * cell_w - 80) / per_row as i32;
            for (k, bi) in ids.iter().take(per_row).enumerate() {
                let x = widgets::INSET + k as i32 * (cell_w + gap);
                let focused = self.focus == (si + 1, k);
                if (shelf_h - line_h(fl) - 8) < 132 {
                    // Not enough room for a full cover: compact rows.
                    let b = &cat.books[*bi];
                    draw_text(
                        f,
                        quire_fonts::ui::label(),
                        x,
                        cy + fl.ascent(),
                        &ellipsis(fl, &b.title, cell_w),
                        TextStyle { inverted: focused, ..TextStyle::INK },
                    );
                } else {
                    shop_cover(f, x, cy, &cat.books[*bi], focused);
                }
            }
            let mx = widgets::INSET + per_row as i32 * (cell_w + gap);
            let more_focused = self.focus == (si + 1, per_row);
            let mr = Rect::new(mx, cy, 72, 132.min(shelf_h - line_h(fl) - 8).max(24) as u32);
            if more_focused {
                f.fill_rect(mr, Ink::Black);
            }
            f.stroke_rect(mr, 1, Ink::Black);
            crate::text::draw_centered(
                f,
                fl,
                mr.x + 36,
                mr.y + mr.h as i32 / 2 + 6,
                "More",
                TextStyle { inverted: more_focused, ..TextStyle::INK },
            );
        }
        rail(f, ["Browse", "Back", "Open", "Search"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        self.ensure(cx);
        let cat = self.cat.clone().unwrap_or_default();
        let shelves = shelves(&cat, &cx.settings.bookshop_language);
        match ev.key {
            Key::Back => Action::Pop,
            Key::Left if self.focus.0 == 0 => Action::Push(Box::new(Browse::new())),
            Key::Right if self.focus.0 == 0 => Action::Push(Box::new(Search::new())),
            Key::Up => {
                self.focus.0 = self.focus.0.saturating_sub(1);
                Action::Redraw
            }
            Key::Down => {
                self.focus.0 = (self.focus.0 + 1).min(shelves.len());
                Action::Redraw
            }
            Key::Left => {
                self.focus.1 = self.focus.1.saturating_sub(1);
                Action::Redraw
            }
            Key::Right => {
                self.focus.1 = (self.focus.1 + 1).min(3);
                Action::Redraw
            }
            Key::Confirm => {
                if self.focus.0 == 0 {
                    return Action::Push(Box::new(Search::new()));
                }
                let (name, ids) = &shelves[self.focus.0 - 1];
                if self.focus.1 >= 3 || self.focus.1 >= ids.len() {
                    return Action::Push(Box::new(ShelfList::new(name, ids.clone())));
                }
                Action::Push(Box::new(BookPage::new(cat.books[ids[self.focus.1]].clone())))
            }
            Key::Power => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if matches!(ev, Event::Net(NetEvent::Shelves)) {
            self.cat = None;
            return Action::Redraw;
        }
        Action::None
    }
}

/// A full shelf as a list.
pub struct ShelfList {
    title: String,
    ids: Vec<usize>,
    cat: Option<Catalog>,
    nav: ListNav,
}

impl ShelfList {
    /// New.
    pub fn new(title: &str, ids: Vec<usize>) -> Self {
        let n = ids.len();
        ShelfList { title: title.into(), ids, cat: None, nav: ListNav::new(n, 10) }
    }
}

impl<E: Env> Screen<E> for ShelfList {
    fn name(&self) -> &'static str {
        "38-browse-shelf"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if self.cat.is_none() {
            self.cat = Some(load_catalog(cx.env.fs()));
        }
        let cat = self.cat.as_ref().cloned().unwrap_or_default();
        let row_h = ROW_H;
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, &self.title, Some(&page_indicator(self.nav.page(), self.nav.pages())));
        let mut y = widgets::CONTENT_TOP;
        for i in self.nav.visible() {
            let Some(b) = cat.books.get(self.ids[i]) else { continue };
            let v = if in_library(cx, b).is_some() { String::from("in library") } else { hours_text(b.hours10) };
            row(
                f,
                y,
                row_h,
                &alloc::format!("{} · {}", b.title, b.author),
                None,
                Some(&v),
                if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
            );
            y += row_h;
        }
        rail(f, ["", "Back", "Open", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        if ev.is(Key::Confirm) {
            if let Some(cat) = &self.cat {
                if let Some(b) = self.ids.get(self.nav.focus).and_then(|i| cat.books.get(*i)) {
                    return Action::Push(Box::new(BookPage::new(b.clone())));
                }
            }
            return Action::None;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
}

// ---------------------------------------------------------------------------------------
// 36 book page

/// A catalog book's page.
pub struct BookPage {
    book: ShopBook,
    page: usize,
    focus: usize,
}

impl BookPage {
    /// New.
    pub fn new(book: ShopBook) -> Self {
        BookPage { book, page: 0, focus: 0 }
    }
    fn download_state<E: Env>(&self, cx: &mut Ctx<E>) -> Option<DownloadState> {
        cx.env.net().downloads().iter().find(|d| d.url == self.book.url).map(|d| d.state.clone())
    }
}

impl<E: Env> Screen<E> for BookPage {
    fn name(&self) -> &'static str {
        "36-book"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Book", None);
        let w = f.width() as i32;
        let b = self.book.clone();
        let ft = quire_fonts::ui::title();
        let fb = quire_fonts::ui::body();
        let fl = quire_fonts::ui::label();
        let mut y = widgets::CONTENT_TOP;
        shop_cover(f, widgets::INSET, y, &b, false);
        let tx = widgets::INSET + 96 + 20;
        let tw = w - tx - widgets::INSET;
        let mut ty = y;
        for l in wrap(ft, &b.title, tw).iter().take(3) {
            draw_text(f, ft, tx, ty + ft.ascent(), l, TextStyle::INK);
            ty += line_h(ft);
        }
        draw_text(f, fb, tx, ty + fb.ascent(), &ellipsis(fb, &b.author, tw), TextStyle::INK);
        ty += line_h(fb);
        draw_text(f, fl, tx, ty + fl.ascent() + 4, &alloc::format!("{} · {}", b.year, b.lang.to_uppercase()), TextStyle::INK);
        y += 132 + 16;
        let tiles = alloc::vec![
            (hours_text(b.hours10), String::from("To read")),
            (alloc::format!("{} MB", b.size / 1_000_000), String::from("Size"))
        ];
        y = poster_tiles(f, widgets::INSET, y, w - 2 * widgets::INSET, &tiles, 2) + 12;
        draw_text(
            f,
            fl,
            widgets::INSET,
            y + fl.ascent(),
            &ellipsis(fl, &alloc::format!("{} · {}", b.source, b.licence), w - 2 * widgets::INSET),
            TextStyle::INK,
        );
        y += line_h(fl) + 10;
        // Blurb paginated.
        let per = ((f.height() as i32 - RAIL_H - y - ROW_H - 20) / line_h(fb)).max(2) as usize;
        let pages = paginate(fb, &b.blurb, w - 2 * widgets::INSET, per);
        for l in pages.get(self.page).into_iter().flatten() {
            draw_text(f, fb, widgets::INSET, y + fb.ascent(), l, TextStyle::INK);
            y += line_h(fb);
        }
        // Primary state line.
        let state = self.download_state(cx);
        let in_lib = in_library(cx, &b);
        let wifi_on = matches!(cx.env.wifi(), WifiState::Connected { .. });
        let sy = f.height() as i32 - RAIL_H - ROW_H - 8;
        let (label, action) = match (&state, in_lib, wifi_on) {
            (_, Some(_), _) => ("In your library", "Read"),
            (Some(DownloadState::Working), _, _) | (Some(DownloadState::Queued), _, _) => ("Downloading", ""),
            (Some(DownloadState::Retrying(_)), _, _) => ("Gutenberg is limiting requests, retrying", ""),
            (Some(DownloadState::Failed(_)), _, _) => ("Couldn't download", "Retry"),
            (_, None, false) => ("Connect to get this book", "Wi-Fi"),
            _ => ("Get", "Get"),
        };
        if let Some(DownloadState::Working) = &state {
            let d = cx.env.net().downloads().iter().find(|d| d.url == b.url).cloned();
            if let Some(d) = d {
                stepped_bar(
                    f,
                    Rect::new(widgets::INSET, sy + 20, (w - 2 * widgets::INSET) as u32, 16),
                    d.total.map(|t| (d.done * 1000 / t.max(1)) as u32).unwrap_or(0),
                );
            }
        } else {
            setting_row(
                f,
                sy,
                ROW_H,
                label,
                &SettingValue::Text(String::from(action)),
                if self.focus == 0 { RowState::Focused } else { RowState::Normal },
            );
        }
        let saved = load_saved(cx.env.fs()).contains(&b.id);
        rail(f, [if saved { "Saved ✓" } else { "Save" }, "Back", action, "More"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        let b = self.book.clone();
        match ev.key {
            Key::Back => Action::Pop,
            Key::Left => {
                let mut saved = load_saved(cx.env.fs());
                if let Some(i) = saved.iter().position(|s| *s == b.id) {
                    saved.remove(i);
                } else {
                    saved.push(b.id.clone());
                }
                save_saved(cx.env.fs(), &saved);
                Action::Redraw
            }
            Key::Right => {
                self.page += 1;
                Action::Redraw
            }
            Key::Down => Action::Push(Box::new(Search::with_query(&b.author))),
            Key::Confirm => {
                if let Some(id) = in_library(cx, &b) {
                    return Action::Open(id);
                }
                if !matches!(cx.env.wifi(), WifiState::Connected { .. }) {
                    return Action::Push(Box::new(super::wifi::WifiScreen::new_with_hint("Connect to get this book")));
                }
                cx.env.request(SysRequest::Fetch(FetchRequest::Book {
                    url: b.url.clone(),
                    title: b.title.clone(),
                    author: b.author.clone(),
                    size: Some(b.size as u64),
                }));
                Action::Redraw
            }
            _ => Action::None,
        }
    }
    fn event(&mut self, cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::Net(NetEvent::Downloads) | Event::BooksChanged | Event::Ingest { .. } | Event::Wifi(_) => {
                let _ = cx;
                Action::Redraw
            }
            _ => Action::None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// 37 search, 38 browse

/// Search the offline catalog as you type.
pub struct Search {
    query: String,
    cat: Option<Catalog>,
    nav: ListNav,
}

impl Search {
    /// New.
    pub fn new() -> Self {
        Search { query: String::new(), cat: None, nav: ListNav::new(0, 9) }
    }
    /// With a query.
    pub fn with_query(q: &str) -> Self {
        Search { query: q.into(), ..Self::new() }
    }
    fn results(&self) -> Vec<usize> {
        let Some(cat) = &self.cat else { return Vec::new() };
        let q = self.query.to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let mut out: Vec<usize> = cat
            .books
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                let hay = alloc::format!("{} {} {}", b.title, b.author, b.subjects.join(" ")).to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .map(|(i, _)| i)
            .collect();
        out.sort_by_key(|i| cat.books[*i].rank);
        out
    }
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Search {
    fn name(&self) -> &'static str {
        "37-search"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if self.cat.is_none() {
            self.cat = Some(load_catalog(cx.env.fs()));
        }
        running_head(f, "Search", None);
        let w = f.width() as i32;
        let y = widgets::CONTENT_TOP - 4;
        widgets::text_field(
            f,
            Rect::new(widgets::INSET, y, (w - 2 * widgets::INSET) as u32, 40),
            &self.query,
            "Title, author or subject",
            true,
        );
        let results = self.results();
        let row_h = ROW_H;
        let top = y + 52;
        self.nav.per_page = widgets::rows_between(top, f.height() as i32 - RAIL_H, row_h);
        self.nav.set_n(results.len());
        let cat = self.cat.clone().unwrap_or_default();
        let mut yy = top;
        for i in self.nav.visible() {
            let b = &cat.books[results[i]];
            let v = if in_library(cx, b).is_some() {
                String::from("in library")
            } else {
                alloc::format!("{} · {}", src_glyph(&b.source), hours_text(b.hours10))
            };
            row(f, yy, row_h, &b.title, Some(&b.author), Some(&v), if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
            yy += row_h;
        }
        if results.is_empty() && !self.query.is_empty() {
            empty_state(f, top + 80, "Nothing found", "Try the author's surname, or Browse by subject.");
        }
        rail(f, ["", "Back", "Open", "Type"], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            return Action::Pop;
        }
        if ev.is(Key::Right) || ev.is_long(Key::Confirm) {
            return Action::Push(KeyboardScreen::t9("Search", &self.query, "Title, author or subject").boxed());
        }
        if ev.is(Key::Confirm) {
            let results = self.results();
            if let (Some(cat), Some(&i)) = (&self.cat, results.get(self.nav.focus)) {
                return Action::Push(Box::new(BookPage::new(cat.books[i].clone())));
            }
            let _ = cx;
            return Action::None;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
    fn event(&mut self, cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if let Event::PhoneText(t) = ev {
            cx.phone_text.take();
            self.query = t.clone();
            self.nav.focus = 0;
            return Action::Redraw;
        }
        Action::None
    }
    fn result(&mut self, _cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let Result_::Text(t) = r {
            self.query = t;
            self.nav.focus = 0;
        }
        Action::Redraw
    }
}

fn src_glyph(source: &str) -> &'static str {
    if source.contains("Gutenberg") {
        "PG"
    } else if source.contains("Standard") {
        "SE"
    } else {
        "CC"
    }
}

/// Browse: subjects, authors A–Z, collections, languages.
pub struct Browse {
    cat: Option<Catalog>,
    /// 0 = kinds, 1 = a list of groups, 2 = books.
    level: u8,
    kind: usize,
    group: String,
    nav: ListNav,
}

impl Browse {
    /// New.
    pub fn new() -> Self {
        Browse { cat: None, level: 0, kind: 0, group: String::new(), nav: ListNav::new(4, 10) }
    }
    fn groups(&self) -> Vec<(String, usize)> {
        let Some(cat) = &self.cat else { return Vec::new() };
        let mut names: Vec<String> = match self.kind {
            0 => cat.books.iter().flat_map(|b| b.subjects.clone()).collect(),
            1 => cat.books.iter().map(|b| b.author.clone()).collect(),
            2 => cat.books.iter().filter_map(|b| b.collection.clone()).collect(),
            _ => cat.books.iter().map(|b| b.lang.to_uppercase()).collect(),
        };
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
            .into_iter()
            .map(|n| {
                let count = self.books_in(&n).len();
                (n, count)
            })
            .collect()
    }
    fn books_in(&self, group: &str) -> Vec<usize> {
        let Some(cat) = &self.cat else { return Vec::new() };
        let mut v: Vec<usize> = cat
            .books
            .iter()
            .enumerate()
            .filter(|(_, b)| match self.kind {
                0 => b.subjects.iter().any(|s| s == group),
                1 => b.author == group,
                2 => b.collection.as_deref() == Some(group),
                _ => b.lang.eq_ignore_ascii_case(group),
            })
            .map(|(i, _)| i)
            .collect();
        v.sort_by_key(|i| cat.books[*i].rank);
        v
    }
}

impl Default for Browse {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Browse {
    fn name(&self) -> &'static str {
        "38-browse"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if self.cat.is_none() {
            self.cat = Some(load_catalog(cx.env.fs()));
        }
        let title = match self.level {
            0 => String::from("Browse"),
            1 => String::from(["Subjects", "Authors", "Collections", "Languages"][self.kind]),
            _ => self.group.clone(),
        };
        let row_h = ROW_H;
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, &title, Some(&page_indicator(self.nav.page(), self.nav.pages())));
        let mut y = widgets::CONTENT_TOP;
        match self.level {
            0 => {
                let kinds = [
                    ("Subjects", "adventure, mystery, philosophy…"),
                    ("Authors A to Z", ""),
                    ("Collections", "curated sets with a line of intro"),
                    ("Languages", ""),
                ];
                self.nav.set_n(4);
                for (i, (t, s)) in kinds.iter().enumerate() {
                    row(
                        f,
                        y,
                        row_h,
                        t,
                        if s.is_empty() { None } else { Some(s) },
                        None,
                        if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
                    );
                    y += row_h;
                }
            }
            1 => {
                let groups = self.groups();
                self.nav.set_n(groups.len());
                for i in self.nav.visible() {
                    let (n, c) = &groups[i];
                    row(
                        f,
                        y,
                        row_h,
                        n,
                        None,
                        Some(&alloc::format!("{c}")),
                        if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
                    );
                    y += row_h;
                }
            }
            _ => {
                let books = self.books_in(&self.group);
                self.nav.set_n(books.len());
                let cat = self.cat.clone().unwrap_or_default();
                for i in self.nav.visible() {
                    let b = &cat.books[books[i]];
                    row(
                        f,
                        y,
                        row_h,
                        &b.title,
                        Some(&b.author),
                        Some(&hours_text(b.hours10)),
                        if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
                    );
                    y += row_h;
                }
            }
        }
        rail(f, ["", "Back", "Open", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, _cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            if self.level == 0 {
                return Action::Pop;
            }
            self.level -= 1;
            self.nav.focus = 0;
            return Action::Redraw;
        }
        if ev.is(Key::Confirm) {
            match self.level {
                0 => {
                    self.kind = self.nav.focus;
                    self.level = 1;
                    self.nav.focus = 0;
                }
                1 => {
                    if let Some((g, _)) = self.groups().get(self.nav.focus).cloned() {
                        self.group = g;
                        self.level = 2;
                        self.nav.focus = 0;
                    }
                }
                _ => {
                    let books = self.books_in(&self.group);
                    if let (Some(cat), Some(&i)) = (&self.cat, books.get(self.nav.focus)) {
                        return Action::Push(Box::new(BookPage::new(cat.books[i].clone())));
                    }
                }
            }
            return Action::Redraw;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
}

// ---------------------------------------------------------------------------------------
// 39 downloads and saved

/// The download queue and the saved list.
pub struct Downloads {
    nav: ListNav,
    saved_tab: bool,
}

impl Downloads {
    /// New.
    pub fn new() -> Self {
        Downloads { nav: ListNav::new(0, 9), saved_tab: false }
    }
}

impl Default for Downloads {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Downloads {
    fn name(&self) -> &'static str {
        "39-downloads"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let y0 = widgets::tabs(f, widgets::CONTENT_TOP - 8, &["Downloads", "Saved"], self.saved_tab as usize, false, None);
        running_head(f, "Downloads", None);
        let row_h = ROW_H;
        self.nav.per_page = widgets::rows_between(y0 + 4, f.height() as i32 - RAIL_H, row_h);
        let mut y = y0 + 4;
        if self.saved_tab {
            let cat = load_catalog(cx.env.fs());
            let saved = load_saved(cx.env.fs());
            let books: Vec<&ShopBook> = saved.iter().filter_map(|id| cat.books.iter().find(|b| b.id == *id)).collect();
            self.nav.set_n(books.len());
            if books.is_empty() {
                empty_state(f, y + 100, "Nothing saved", "Left on a book page saves it for later.");
            }
            for i in self.nav.visible() {
                let b = books[i];
                row(
                    f,
                    y,
                    row_h,
                    &b.title,
                    Some(&b.author),
                    Some(&hours_text(b.hours10)),
                    if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
                );
                y += row_h;
            }
            rail(f, ["Queue", "Back", "Open", "Get all"], None);
        } else {
            let downloads: Vec<crate::net::Download> = cx.env.net().downloads().to_vec();
            self.nav.set_n(downloads.len());
            if downloads.is_empty() {
                empty_state(f, y + 100, "No downloads", "Books you get from the shop appear here.");
            }
            for i in self.nav.visible() {
                let d = &downloads[i];
                let v = match &d.state {
                    DownloadState::Queued => String::from("waiting"),
                    DownloadState::Working => {
                        d.total.map(|t| alloc::format!("{}%", d.done * 100 / t.max(1))).unwrap_or_else(|| String::from("…"))
                    }
                    DownloadState::Done => String::from("done"),
                    DownloadState::Failed(e) => ellipsis(quire_fonts::ui::label(), e, 160),
                    DownloadState::Retrying(s) => alloc::format!("retry in {s} s"),
                };
                row(
                    f,
                    y,
                    row_h,
                    &d.title,
                    Some(&d.author),
                    Some(&v),
                    if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
                );
                y += row_h;
            }
            rail(f, ["Saved", "Back", "Retry", "Cancel"], None);
        }
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Left => {
                self.saved_tab = !self.saved_tab;
                self.nav.focus = 0;
                Action::Redraw
            }
            Key::Confirm if self.saved_tab => {
                let cat = load_catalog(cx.env.fs());
                let saved = load_saved(cx.env.fs());
                let books: Vec<&ShopBook> = saved.iter().filter_map(|id| cat.books.iter().find(|b| b.id == *id)).collect();
                match books.get(self.nav.focus) {
                    Some(b) => Action::Push(Box::new(BookPage::new((*b).clone()))),
                    None => Action::None,
                }
            }
            Key::Confirm => {
                cx.env.request(SysRequest::Fetch(FetchRequest::Retry(self.nav.focus)));
                Action::Redraw
            }
            Key::Right if self.saved_tab => {
                let cat = load_catalog(cx.env.fs());
                let saved = load_saved(cx.env.fs());
                for id in saved {
                    if let Some(b) = cat.books.iter().find(|b| b.id == id) {
                        if in_library(cx, b).is_none() {
                            cx.env.request(SysRequest::Fetch(FetchRequest::Book {
                                url: b.url.clone(),
                                title: b.title.clone(),
                                author: b.author.clone(),
                                size: Some(b.size as u64),
                            }));
                        }
                    }
                }
                self.saved_tab = false;
                Action::Redraw
            }
            Key::Right => {
                cx.env.request(SysRequest::Fetch(FetchRequest::Cancel(self.nav.focus)));
                Action::Redraw
            }
            _ => {
                if self.nav.key(ev) {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if matches!(ev, Event::Net(NetEvent::Downloads)) {
            Action::Redraw
        } else {
            Action::None
        }
    }
}

// ---------------------------------------------------------------------------------------
// 32 OPDS

/// OPDS catalogs: saved servers, browser, download.
pub struct OpdsScreen {
    /// Current feed: (title, entries), or None at the server list.
    feed: Option<(String, Vec<OpdsEntry>)>,
    history: Vec<String>,
    loading: Option<String>,
    error: Option<String>,
    nav: ListNav,
}

impl OpdsScreen {
    /// New.
    pub fn new() -> Self {
        OpdsScreen { feed: None, history: Vec::new(), loading: None, error: None, nav: ListNav::new(0, 9) }
    }
}

impl Default for OpdsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for OpdsScreen {
    fn name(&self) -> &'static str {
        "32-opds"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let title = self.feed.as_ref().map(|f| f.0.clone()).unwrap_or_else(|| String::from("Catalogs"));
        let row_h = ROW_H;
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        running_head(f, &title, Some(&page_indicator(self.nav.page(), self.nav.pages())));
        let mut y = widgets::CONTENT_TOP;
        if let Some(l) = &self.loading {
            widgets::working_card(f, "Loading", l, 0, "fetching the catalog");
            rail(f, ["", "Back", "", ""], None);
            return Refresh::Du;
        }
        if let Some(e) = &self.error {
            let fb = quire_fonts::ui::body();
            for l in wrap(fb, &alloc::format!("Couldn't load the catalog. {e}"), f.width() as i32 - 2 * widgets::INSET) {
                draw_text(f, fb, widgets::INSET, y + fb.ascent(), &l, TextStyle::INK);
                y += line_h(fb);
            }
            y += 12;
        }
        match &self.feed {
            None => {
                let servers = cx.settings.opds.clone();
                self.nav.set_n(servers.len() + 1);
                for i in self.nav.visible() {
                    if i < servers.len() {
                        row(
                            f,
                            y,
                            row_h,
                            &servers[i].0,
                            Some(&servers[i].1),
                            None,
                            if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
                        );
                    } else {
                        row(
                            f,
                            y,
                            row_h,
                            "Add a catalog",
                            Some("URL of an OPDS feed"),
                            None,
                            if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
                        );
                    }
                    y += row_h;
                }
                rail(f, ["", "Back", "Open", "Remove"], None);
            }
            Some((_, entries)) => {
                self.nav.set_n(entries.len());
                for i in self.nav.visible() {
                    let e = &entries[i];
                    let v = if e.nav.is_some() { "›" } else { "Get" };
                    row(
                        f,
                        y,
                        row_h,
                        &e.title,
                        if e.author.is_empty() { None } else { Some(&e.author) },
                        Some(v),
                        if i == self.nav.focus { RowState::Focused } else { RowState::Normal },
                    );
                    y += row_h;
                }
                if entries.is_empty() {
                    empty_state(f, y + 80, "Empty catalog", "");
                }
                rail(f, ["", "Back", "Open", ""], None);
            }
        }
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind == KeyKind::Release {
            return Action::None;
        }
        if ev.is(Key::Back) {
            if self.feed.is_some() {
                match self.history.pop() {
                    Some(prev) => {
                        self.loading = Some(prev.clone());
                        cx.env.request(SysRequest::Fetch(FetchRequest::Opds(prev)));
                    }
                    None => self.feed = None,
                }
                self.nav.focus = 0;
                return Action::Redraw;
            }
            return Action::Pop;
        }
        if ev.is(Key::Confirm) {
            match &self.feed {
                None => {
                    let servers = cx.settings.opds.clone();
                    if self.nav.focus >= servers.len() {
                        return Action::Push(KeyboardScreen::new("Catalog URL", "https://", "https://…/opds").boxed());
                    }
                    if !matches!(cx.env.wifi(), WifiState::Connected { .. }) {
                        return Action::Push(Box::new(super::wifi::WifiScreen::new_with_hint("Catalogs need Wi-Fi")));
                    }
                    let url = servers[self.nav.focus].1.clone();
                    self.history.clear();
                    self.loading = Some(url.clone());
                    self.error = None;
                    cx.env.request(SysRequest::Fetch(FetchRequest::Opds(url)));
                }
                Some((cur, entries)) => {
                    let Some(e) = entries.get(self.nav.focus).cloned() else { return Action::None };
                    if let Some(n) = e.nav {
                        self.history.push(cur.clone());
                        self.loading = Some(n.clone());
                        cx.env.request(SysRequest::Fetch(FetchRequest::Opds(n)));
                    } else if let Some(a) = e.acquisition {
                        cx.env.request(SysRequest::Fetch(FetchRequest::Book {
                            url: a,
                            title: e.title.clone(),
                            author: e.author.clone(),
                            size: None,
                        }));
                        return Action::Push(Box::new(Downloads::new()));
                    }
                }
            }
            return Action::Redraw;
        }
        if ev.is(Key::Right) && self.feed.is_none() {
            if self.nav.focus < cx.settings.opds.len() {
                cx.settings.opds.remove(self.nav.focus);
            }
            return Action::Redraw;
        }
        if self.nav.key(ev) {
            return Action::Redraw;
        }
        Action::None
    }
    fn event(&mut self, cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if let Event::Net(NetEvent::Opds(r)) = ev {
            self.loading = None;
            match r {
                Ok((t, entries)) => {
                    let url = self.history.last().cloned().unwrap_or_default();
                    let _ = url;
                    self.feed = Some((t.clone(), entries.clone()));
                    self.error = None;
                }
                Err(e) => self.error = Some(e.clone()),
            }
            self.nav.focus = 0;
            let _ = cx;
            return Action::Redraw;
        }
        Action::None
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let Result_::Text(url) = r {
            let url = url.trim();
            if url.starts_with("http") && cx.settings.opds.len() < 16 {
                let name = url.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or("catalog").into();
                cx.settings.opds.push((name, String::from(url)));
            }
        }
        Action::Redraw
    }
}

// ---------------------------------------------------------------------------------------
// 33 Calibre connect, 34 sync

/// Calibre wireless device connection status.
pub struct CalibreScreen {
    started: bool,
}

impl CalibreScreen {
    /// New.
    pub fn new() -> Self {
        CalibreScreen { started: false }
    }
}

impl Default for CalibreScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for CalibreScreen {
    fn name(&self) -> &'static str {
        "33-calibre"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.started {
            self.started = true;
            cx.env.request(SysRequest::Calibre(true));
        }
        running_head(f, "Calibre", None);
        let w = f.width() as i32;
        let fb = quire_fonts::ui::body();
        let fl = quire_fonts::ui::label();
        let mut y = widgets::CONTENT_TOP;
        let (line, port) = match cx.env.wifi() {
            WifiState::Connected { ip, .. } => (alloc::format!("Waiting for Calibre… {ip}:{}", cx.settings.calibre_port), true),
            _ => (String::from("Calibre needs Wi-Fi. Turn it on from the Power menu."), false),
        };
        draw_text(f, fb, widgets::INSET, y + fb.ascent(), &ellipsis(fb, &line, w - 2 * widgets::INSET), TextStyle::INK);
        y += line_h(fb) + 8;
        let status = cx.env.net().calibre_status();
        draw_text(f, fl, widgets::INSET, y + fl.ascent(), &ellipsis(fl, &status, w - 2 * widgets::INSET), TextStyle::INK);
        y += line_h(fl) + 16;
        if port {
            for l in wrap(
                fb,
                "In Calibre choose Connect/share → Start wireless device connection, then send books to the device.",
                w - 2 * widgets::INSET,
            ) {
                draw_text(f, fb, widgets::INSET, y + fb.ascent(), &l, TextStyle::INK);
                y += line_h(fb);
            }
        }
        y += 12;
        let downloads: Vec<crate::net::Download> =
            cx.env.net().downloads().iter().filter(|d| d.url.starts_with("calibre:")).cloned().collect();
        for d in downloads.iter().rev().take(6) {
            let v = match &d.state {
                DownloadState::Done => String::from("added"),
                DownloadState::Working => d.total.map(|t| alloc::format!("{}%", d.done * 100 / t.max(1))).unwrap_or_default(),
                DownloadState::Failed(e) => ellipsis(fl, e, 160),
                _ => String::from("…"),
            };
            row(f, y, ROW_H, &d.title, Some(&d.author), Some(&v), RowState::Normal);
            y += ROW_H;
        }
        rail(f, ["", "Back", "", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.is(Key::Back) {
            cx.env.request(SysRequest::Calibre(false));
            return Action::Pop;
        }
        Action::None
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        match ev {
            Event::Net(_) | Event::Wifi(_) | Event::BooksChanged => Action::Redraw,
            _ => Action::None,
        }
    }
}

/// Position sync (KOReader-compatible server).
pub struct SyncScreen {
    focus: usize,
    last: Option<Result<u32, String>>,
    working: bool,
}

impl SyncScreen {
    /// New.
    pub fn new() -> Self {
        SyncScreen { focus: 0, last: None, working: false }
    }
}

impl Default for SyncScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for SyncScreen {
    fn name(&self) -> &'static str {
        "34-sync"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        running_head(f, "Sync", None);
        let s = &*cx.settings;
        let mut y = widgets::CONTENT_TOP;
        let status = match &self.last {
            None if self.working => String::from("syncing…"),
            None => String::from("never"),
            Some(Ok(n)) => alloc::format!("{n} books updated"),
            Some(Err(e)) => ellipsis(quire_fonts::ui::label(), e, 200),
        };
        let rows: [(&str, SettingValue); 4] = [
            ("Sync now", SettingValue::Text(status)),
            ("Server", SettingValue::Text(if s.sync_url.is_empty() { String::from("not set") } else { s.sync_url.clone() })),
            ("User", SettingValue::Text(s.sync_user.clone())),
            ("Key", SettingValue::Text(if s.sync_key.is_empty() { String::new() } else { String::from("••••") })),
        ];
        for (i, (t, v)) in rows.iter().enumerate() {
            setting_row(f, y, ROW_H, t, v, if self.focus == i { RowState::Focused } else { RowState::Normal });
            y += ROW_H;
        }
        y += 16;
        let fl = quire_fonts::ui::label();
        for l in wrap(fl, "Positions sync per book with a KOReader-compatible progress server. Books match by file hash. Your reading stays on your server.", f.width() as i32 - 2 * widgets::INSET) {
            draw_text(f, fl, widgets::INSET, y + fl.ascent(), &l, TextStyle::INK);
            y += line_h(fl);
        }
        rail(f, ["", "Back", "Change", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        match ev.key {
            Key::Back => Action::Pop,
            Key::Up => {
                self.focus = (self.focus + 3) % 4;
                Action::Redraw
            }
            Key::Down => {
                self.focus = (self.focus + 1) % 4;
                Action::Redraw
            }
            Key::Confirm | Key::Right => match self.focus {
                0 => {
                    if !matches!(cx.env.wifi(), WifiState::Connected { .. }) {
                        return Action::Push(Box::new(super::wifi::WifiScreen::new_with_hint("Sync needs Wi-Fi")));
                    }
                    self.working = true;
                    self.last = None;
                    cx.env.request(SysRequest::SyncNow);
                    Action::Redraw
                }
                1 => Action::Push(KeyboardScreen::new("Sync server", &cx.settings.sync_url, "https://sync.example.org").boxed()),
                2 => Action::Push(KeyboardScreen::new("Sync user", &cx.settings.sync_user, "user").boxed()),
                _ => Action::Push(KeyboardScreen::new("Sync key", "", "password").secret().boxed()),
            },
            _ => Action::None,
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if let Event::Net(NetEvent::Sync(r)) = ev {
            self.working = false;
            self.last = Some(r.clone());
            return Action::Redraw;
        }
        Action::None
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let Result_::Text(t) = r {
            match self.focus {
                1 => cx.settings.sync_url = t.trim().into(),
                2 => cx.settings.sync_user = t.trim().into(),
                3 => cx.settings.sync_key = t,
                _ => {}
            }
        }
        Action::Redraw
    }
}

//! 72 news: feeds with unread counts, articles as rows, read on the reading page.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::Fs;
use quire_gfx::Frame;
use serde::{Deserialize, Serialize};

use crate::keyboard::KeyboardScreen;
use crate::net::{FetchRequest, NetEvent};
use crate::text::page_indicator;
use crate::theme::*;
use crate::widgets::{self, empty_state, rail, row, running_head, ListNav, RowState};
use crate::{Action, Ctx, Env, Event, Key, KeyEvent, KeyKind, Refresh, Result_, Screen, SysRequest, WifiState};

/// Where the network layer stores fetched articles.
pub const ARTICLES_FILE: &str = "/.quire/news/articles.bin";

/// A fetched article.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Article {
    /// Feed URL it came from.
    pub feed: String,
    /// Feed title.
    pub feed_title: String,
    /// Article title.
    pub title: String,
    /// Link.
    pub url: String,
    /// Published (local seconds) if known.
    pub published: u32,
    /// Plain text body (already stripped of markup by the fetcher).
    pub text: String,
    /// Read flag.
    pub read: bool,
}

/// Load the article store.
pub fn load_articles<F: Fs>(fs: &F) -> Vec<Article> {
    fs.read_to_vec(ARTICLES_FILE).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default()
}

/// Save the article store.
pub fn save_articles<F: Fs>(fs: &F, a: &[Article]) {
    if let Ok(b) = postcard::to_allocvec(&a.to_vec()) {
        let _ = fs.mkdir_all("/.quire/news");
        let _ = fs.write_atomic(ARTICLES_FILE, &b);
    }
}

/// Ingest an article into the library as a hidden virtual book and return its id.
pub fn article_book<E: Env>(cx: &mut Ctx<E>, a: &Article) -> Option<quire_library::BookId> {
    let id = quire_library::BookId::of_name(&a.url);
    let fs = cx.env.fs();
    if cx.lib.get(id).map(|b| b.ingest == quire_library::IngestState::Ready).unwrap_or(false) {
        return Some(id);
    }
    let dir = quire_library::book_dir(id);
    let mut sink = quire_library::cache::CacheSink::new(fs, &dir).ok()?;
    let mut w = quire_qtx::Writer::new();
    w.push(&quire_qtx::Token::ChapterTitle { number: None, title: Some(a.title.clone()) });
    w.para(quire_qtx::ParaKind::Caption);
    w.text(&alloc::format!("{} · {}", a.feed_title, quire_library::time::fmt_date(quire_library::time::day_of(a.published))));
    for para in a.text.split("\n\n").map(|p| p.trim()).filter(|p| !p.is_empty()) {
        w.para(quire_qtx::ParaKind::Body);
        w.text(para);
    }
    use quire_doc::Sink;
    sink.metadata(&quire_doc::Metadata {
        title: a.title.clone(),
        authors: alloc::vec![a.feed_title.clone()],
        language: "en".into(),
        ..Default::default()
    })
    .ok()?;
    sink.begin_chapter(0, Some(&a.title)).ok()?;
    sink.chapter_bytes(w.as_bytes()).ok()?;
    sink.end_chapter(w.char_count()).ok()?;
    sink.toc(&[]).ok()?;
    let summary = sink.finish().ok()?;
    cx.lib.upsert(quire_library::BookEntry {
        id,
        path: alloc::format!("news:{}", a.url),
        size: a.text.len() as u64,
        format: quire_doc::Format::Html,
        title: a.title.clone(),
        authors: alloc::vec![a.feed_title.clone()],
        series: None,
        year: None,
        language: "en".into(),
        sections: summary.sections.len() as u16,
        chars: summary.chars(),
        has_cover: false,
        ingest: quire_library::IngestState::Ready,
        error: None,
        added: cx.env.now(),
        last_opened: 0,
        loc: Default::default(),
        status: quire_library::Status::Unread,
        collections: Vec::new(),
        stats: Default::default(),
        missing: false,
        pages_total: None,
    });
    Some(id)
}

/// The news screen.
pub struct News {
    articles: Vec<Article>,
    feed: Option<String>,
    nav: ListNav,
    loaded: bool,
}

impl News {
    /// New.
    pub fn new() -> Self {
        News { articles: Vec::new(), feed: None, nav: ListNav::new(0, 9), loaded: false }
    }
    fn feeds(&self) -> Vec<(String, String, usize)> {
        let mut out: Vec<(String, String, usize)> = Vec::new();
        for a in &self.articles {
            match out.iter_mut().find(|f| f.0 == a.feed) {
                Some(f) => {
                    if !a.read {
                        f.2 += 1;
                    }
                }
                None => out.push((a.feed.clone(), a.feed_title.clone(), (!a.read) as usize)),
            }
        }
        out
    }
}

impl Default for News {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for News {
    fn name(&self) -> &'static str {
        "72-news"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        if !self.loaded {
            self.articles = load_articles(cx.env.fs());
            self.loaded = true;
        }
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        let mut y = widgets::CONTENT_TOP;
        match &self.feed {
            None => {
                let feeds = self.feeds();
                let n = cx.settings.news_feeds.len();
                self.nav.set_n(feeds.len().max(n) + 1);
                running_head(f, "News", Some(&page_indicator(self.nav.page(), self.nav.pages())));
                if feeds.is_empty() && n == 0 {
                    empty_state(f, 240, "No feeds yet", "Right adds an RSS feed URL. Night jobs fetch new articles while you sleep.");
                }
                let mut rows: Vec<(String, String)> = feeds
                    .iter()
                    .map(|(_, t, unread)| (t.clone(), if *unread > 0 { alloc::format!("{unread} unread") } else { String::new() }))
                    .collect();
                for u in &cx.settings.news_feeds {
                    if !feeds.iter().any(|(fu, _, _)| fu == u) {
                        rows.push((u.clone(), String::from("not fetched yet")));
                    }
                }
                rows.push((String::from("Refresh all"), String::new()));
                self.nav.set_n(rows.len());
                for i in self.nav.visible() {
                    let (t, v) = &rows[i];
                    row(f, y, row_h, t, None, Some(v), if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
                    y += row_h;
                }
                rail(f, ["", "Back", "Open", "Add feed"], None);
            }
            Some(feed) => {
                let arts: Vec<&Article> = self.articles.iter().filter(|a| a.feed == *feed).collect();
                let title = arts.first().map(|a| a.feed_title.clone()).unwrap_or_else(|| String::from("Feed"));
                self.nav.set_n(arts.len());
                running_head(f, &title, Some(&page_indicator(self.nav.page(), self.nav.pages())));
                for i in self.nav.visible() {
                    let a = arts[i];
                    let date = quire_library::time::fmt_date(quire_library::time::day_of(a.published));
                    row(
                        f,
                        y,
                        row_h,
                        &a.title,
                        None,
                        Some(&date),
                        if i == self.nav.focus {
                            RowState::Focused
                        } else if a.read {
                            RowState::Disabled
                        } else {
                            RowState::Normal
                        },
                    );
                    y += row_h;
                }
                rail(f, ["", "Back", "Read", ""], None);
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
                self.feed = None;
                self.nav.focus = 0;
                return Action::Redraw;
            }
            return Action::Pop;
        }
        if ev.is(Key::Right) && self.feed.is_none() {
            return Action::Push(KeyboardScreen::new("Feed URL", "https://", "RSS or Atom feed").boxed());
        }
        if ev.is(Key::Confirm) {
            match &self.feed {
                None => {
                    let feeds = self.feeds();
                    let mut urls: Vec<String> = feeds.iter().map(|f| f.0.clone()).collect();
                    for u in &cx.settings.news_feeds {
                        if !urls.contains(u) {
                            urls.push(u.clone());
                        }
                    }
                    if self.nav.focus >= urls.len() {
                        if !matches!(cx.env.wifi(), WifiState::Connected { .. }) {
                            return Action::Push(alloc::boxed::Box::new(super::super::wifi::WifiScreen::new_with_hint(
                                "Fetching news needs Wi-Fi",
                            )));
                        }
                        cx.env.request(SysRequest::Fetch(FetchRequest::News));
                        return Action::Redraw;
                    }
                    self.feed = Some(urls[self.nav.focus].clone());
                    self.nav.focus = 0;
                    Action::Redraw
                }
                Some(feed) => {
                    let feed = feed.clone();
                    let idx = self.articles.iter().enumerate().filter(|(_, a)| a.feed == feed).map(|(i, _)| i).nth(self.nav.focus);
                    let Some(i) = idx else { return Action::None };
                    self.articles[i].read = true;
                    save_articles(cx.env.fs(), &self.articles);
                    let a = self.articles[i].clone();
                    match article_book(cx, &a) {
                        Some(id) => Action::Open(id),
                        None => Action::None,
                    }
                }
            }
        } else if self.nav.key(ev) {
            Action::Redraw
        } else {
            Action::None
        }
    }
    fn event(&mut self, _cx: &mut Ctx<E>, ev: &Event) -> Action<E> {
        if matches!(ev, Event::Net(NetEvent::News)) {
            self.loaded = false;
            return Action::Redraw;
        }
        Action::None
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let Result_::Text(u) = r {
            let u = u.trim();
            if u.starts_with("http") && cx.settings.news_feeds.len() < 32 && !cx.settings.news_feeds.iter().any(|x| x == u) {
                cx.settings.news_feeds.push(String::from(u));
            }
        }
        Action::Redraw
    }
}

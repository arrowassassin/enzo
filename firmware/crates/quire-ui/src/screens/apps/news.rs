//! 72 news: feeds with unread counts, articles as rows, read on the reading page.
//!
//! The store is split so no article body is ever in RAM except the one being opened:
//! `index.bin` holds one small [`ArticleMeta`] per article (feed, title, url, date,
//! read flag — about 120 bytes) and each body is its own text file under
//! `/.quire/news/`. Marking an article read rewrites only the index; the fetcher adds
//! articles with [`store_articles`], which keeps the index bounded.

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

/// Where the news store lives.
pub const NEWS_DIR: &str = "/.quire/news";
/// The article index (metadata only).
pub const INDEX_FILE: &str = "/.quire/news/index.bin";
/// Articles kept at most (the newest by date).
pub const MAX_ARTICLES: usize = 200;

/// A fetched article, as the fetcher hands it over (the body travels with it once).
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

/// An article as the index stores it: everything but the body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArticleMeta {
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
    /// Read flag.
    pub read: bool,
}

impl ArticleMeta {
    /// Where this article's body lives.
    pub fn body_path(&self) -> String {
        body_path(&self.url)
    }
}

/// Body file for an article URL.
pub fn body_path(url: &str) -> String {
    alloc::format!("{NEWS_DIR}/{:016x}.txt", quire_library::BookId::of_name(url).0)
}

/// Load the article index.
pub fn load_index<F: Fs>(fs: &F) -> Vec<ArticleMeta> {
    fs.read_to_vec(INDEX_FILE).ok().and_then(|b| postcard::from_bytes(&b).ok()).unwrap_or_default()
}

/// Save the article index.
pub fn save_index<F: Fs>(fs: &F, index: &[ArticleMeta]) {
    if let Ok(b) = postcard::to_allocvec(index) {
        let _ = fs.mkdir_all(NEWS_DIR);
        let _ = fs.write_atomic(INDEX_FILE, &b);
    }
}

/// Read an article's body.
pub fn load_body<F: Fs>(fs: &F, meta: &ArticleMeta) -> String {
    fs.read_to_vec(&meta.body_path()).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

/// Merge fetched articles into the store: new bodies are written as their own files,
/// read flags of known articles survive, and the index keeps the newest
/// [`MAX_ARTICLES`] (bodies of dropped articles are removed).
pub fn store_articles<F: Fs>(fs: &F, fetched: Vec<Article>) {
    let _ = fs.mkdir_all(NEWS_DIR);
    let mut index = load_index(fs);
    for a in fetched {
        let path = body_path(&a.url);
        match index.iter_mut().find(|m| m.url == a.url) {
            Some(m) => {
                m.title = a.title;
                m.feed_title = a.feed_title;
                if a.published != 0 {
                    m.published = a.published;
                }
            }
            None => {
                let _ = fs.write_atomic(&path, a.text.as_bytes());
                index.push(ArticleMeta {
                    feed: a.feed,
                    feed_title: a.feed_title,
                    title: a.title,
                    url: a.url,
                    published: a.published,
                    read: a.read,
                });
            }
        }
    }
    index.sort_by_key(|a| core::cmp::Reverse(a.published));
    for dropped in index.iter().skip(MAX_ARTICLES) {
        let _ = fs.remove(&dropped.body_path());
    }
    index.truncate(MAX_ARTICLES);
    save_index(fs, &index);
}

/// Ingest an article into the library as a hidden virtual book and return its id.
pub fn article_book<E: Env>(cx: &mut Ctx<E>, a: &ArticleMeta, text: &str) -> Option<quire_library::BookId> {
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
    for para in text.split("\n\n").map(|p| p.trim()).filter(|p| !p.is_empty()) {
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
        size: text.len() as u64,
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
    articles: Vec<ArticleMeta>,
    feed: Option<String>,
    nav: ListNav,
    loaded: bool,
}

/// A feed row: url, title, unread count.
struct FeedRow {
    url: String,
    title: String,
    unread: usize,
    fetched: bool,
}

impl News {
    /// New.
    pub fn new() -> Self {
        News { articles: Vec::new(), feed: None, nav: ListNav::new(0, 9), loaded: false }
    }
    /// Feeds with articles first (unread counts), then configured feeds not yet fetched.
    fn feeds(&self, configured: &[String]) -> Vec<FeedRow> {
        let mut out: Vec<FeedRow> = Vec::new();
        for a in &self.articles {
            match out.iter_mut().find(|f| f.url == a.feed) {
                Some(f) => f.unread += (!a.read) as usize,
                None => out.push(FeedRow { url: a.feed.clone(), title: a.feed_title.clone(), unread: (!a.read) as usize, fetched: true }),
            }
        }
        for u in configured {
            if !out.iter().any(|f| f.url == *u) {
                out.push(FeedRow { url: u.clone(), title: u.clone(), unread: 0, fetched: false });
            }
        }
        out
    }
    /// Indexes into `articles` of the open feed's articles.
    fn feed_articles(&self, feed: &str) -> Vec<usize> {
        self.articles.iter().enumerate().filter(|(_, a)| a.feed == feed).map(|(i, _)| i).collect()
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
            self.articles = load_index(cx.env.fs());
            self.loaded = true;
        }
        let row_h = cx.settings.row_h();
        self.nav.per_page = widgets::rows_between(widgets::CONTENT_TOP, f.height() as i32 - RAIL_H, row_h);
        let mut y = widgets::CONTENT_TOP;
        match &self.feed {
            None => {
                let feeds = self.feeds(&cx.settings.news_feeds);
                // "Refresh all" is a row only once there is a feed to refresh.
                self.nav.set_n(if feeds.is_empty() { 0 } else { feeds.len() + 1 });
                running_head(f, "News", Some(&page_indicator(self.nav.page(), self.nav.pages())));
                if feeds.is_empty() {
                    empty_state(f, 240, "No feeds yet", "Right adds an RSS feed URL. Night jobs fetch new articles while you sleep.");
                    rail(f, ["", "Back", "", "Add feed"], None);
                    return Refresh::Gc;
                }
                for i in self.nav.visible() {
                    let (t, v) = match feeds.get(i) {
                        Some(fr) if !fr.fetched => (fr.title.clone(), String::from("not fetched yet")),
                        Some(fr) => (fr.title.clone(), if fr.unread > 0 { alloc::format!("{} unread", fr.unread) } else { String::new() }),
                        None => (String::from("Refresh all"), String::new()),
                    };
                    row(f, y, row_h, &t, None, Some(&v), if i == self.nav.focus { RowState::Focused } else { RowState::Normal });
                    y += row_h;
                }
                rail(f, ["", "Back", "Open", "Add feed"], None);
            }
            Some(feed) => {
                let arts = self.feed_articles(feed);
                let title = arts.first().map(|i| self.articles[*i].feed_title.clone()).unwrap_or_else(|| String::from("Feed"));
                self.nav.set_n(arts.len());
                running_head(f, &title, Some(&page_indicator(self.nav.page(), self.nav.pages())));
                if arts.is_empty() {
                    empty_state(f, 240, "Nothing fetched yet", "Night jobs fetch new articles while you sleep.");
                }
                for i in self.nav.visible() {
                    let a = &self.articles[arts[i]];
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
                rail(f, ["", "Back", if arts.is_empty() { "" } else { "Read" }, ""], None);
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
                    let feeds = self.feeds(&cx.settings.news_feeds);
                    if feeds.is_empty() {
                        return Action::None;
                    }
                    if self.nav.focus >= feeds.len() {
                        if !matches!(cx.env.wifi(), WifiState::Connected { .. }) {
                            return Action::Push(alloc::boxed::Box::new(super::super::wifi::WifiScreen::new_with_hint(
                                "Fetching news needs Wi-Fi",
                            )));
                        }
                        cx.env.request(SysRequest::Fetch(FetchRequest::News));
                        return Action::Redraw;
                    }
                    self.feed = Some(feeds[self.nav.focus].url.clone());
                    self.nav.focus = 0;
                    Action::Redraw
                }
                Some(feed) => {
                    let Some(i) = self.feed_articles(feed).get(self.nav.focus).copied() else { return Action::None };
                    if !self.articles[i].read {
                        self.articles[i].read = true;
                        save_index(cx.env.fs(), &self.articles);
                    }
                    let meta = self.articles[i].clone();
                    let text = load_body(cx.env.fs(), &meta);
                    match article_book(cx, &meta, &text) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use quire_fs::host::HostFs;

    fn article(n: u32) -> Article {
        Article {
            feed: String::from("https://example.org/feed"),
            feed_title: String::from("Example"),
            title: alloc::format!("Article {n}"),
            url: alloc::format!("https://example.org/{n}"),
            published: 1_000_000 + n,
            text: alloc::format!("Body of article {n}.\n\nSecond paragraph."),
            read: false,
        }
    }

    #[test]
    fn bodies_live_in_their_own_files_and_the_index_stays_bounded() {
        let dir = std::env::temp_dir().join(alloc::format!("quire-news-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fs = HostFs::new(&dir);
        store_articles(&fs, (0..5).map(article).collect());
        let mut index = load_index(&fs);
        assert_eq!(index.len(), 5);
        assert_eq!(index[0].title, "Article 4", "newest first");
        assert_eq!(load_body(&fs, &index[0]), article(4).text);
        // Marking read touches only the index; the body is untouched.
        index[4].read = true;
        save_index(&fs, &index);
        let again = load_index(&fs);
        assert!(again[4].read);
        // A refetch keeps the read flag and does not duplicate.
        store_articles(&fs, (0..5).map(article).collect());
        let again = load_index(&fs);
        assert_eq!(again.len(), 5);
        assert!(again[4].read);
        // Over the bound, the oldest bodies go.
        store_articles(&fs, (5..(MAX_ARTICLES as u32 + 20)).map(article).collect());
        let index = load_index(&fs);
        assert_eq!(index.len(), MAX_ARTICLES);
        assert!(!fs.exists(&body_path(&article(0).url)), "dropped body removed");
        assert!(fs.exists(&index[0].body_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

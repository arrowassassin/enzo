//! OPDS catalogs: 1.x Atom feeds through the streaming tokenizer (navigation and
//! acquisition entries, relative links resolved against the feed URL) and OPDS 2.0 JSON
//! (`navigation` and `publications`).

use alloc::string::String;
use alloc::vec::Vec;

use quire_ui::net::OpdsEntry;

use super::html;
use super::jsonlite;
use super::url::Url;
use super::xmlscan::{attr, Token, Tokenizer};

/// Entries kept per feed page.
pub const MAX_ENTRIES: usize = 60;
/// Summary length in bytes.
const SUMMARY_MAX: usize = 240;

/// Formats worth downloading, best first.
const FORMATS: &[&str] = &["epub", "kepub", "fb2", "txt", "html", "markdown", "cbz", "zip"];

fn format_rank(mime: &str) -> usize {
    let m = mime.to_ascii_lowercase();
    if m.contains("kepub") {
        return 1;
    }
    FORMATS.iter().position(|f| m.contains(f)).unwrap_or(FORMATS.len())
}

/// A streaming OPDS 1.x (Atom) parser.
pub struct FeedParser {
    tok: Tokenizer,
    base: Option<Url>,
    /// Feed title.
    pub title: String,
    /// Entries in feed order.
    pub entries: Vec<OpdsEntry>,
    /// Element path (local names).
    path: Vec<String>,
    cur: Option<Entry>,
    text: String,
    /// The next page's link.
    pub next: Option<String>,
}

#[derive(Default)]
struct Entry {
    e: OpdsEntry,
    acq_rank: usize,
    in_author: bool,
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl FeedParser {
    /// A parser for a feed fetched from `url`.
    pub fn new(url: &str) -> Self {
        FeedParser {
            tok: Tokenizer::new(16 * 1024),
            base: Url::parse(url),
            title: String::new(),
            entries: Vec::new(),
            path: Vec::new(),
            cur: None,
            text: String::new(),
            next: None,
        }
    }

    fn resolve(&self, href: &str) -> String {
        match &self.base {
            Some(b) => b.resolve(href).map(|u| u.to_text()).unwrap_or_else(|| String::from(href)),
            None => String::from(href),
        }
    }

    /// Feed a chunk.
    pub fn push(&mut self, chunk: &[u8]) {
        let mut tok = core::mem::replace(&mut self.tok, Tokenizer::new(0));
        tok.push(chunk, &mut |t| self.token(t));
        self.tok = tok;
    }

    /// End of input.
    pub fn finish(&mut self) -> (String, Vec<OpdsEntry>) {
        let mut tok = core::mem::replace(&mut self.tok, Tokenizer::new(0));
        tok.finish(&mut |t| self.token(t));
        let title = if self.title.is_empty() { String::from("Catalog") } else { core::mem::take(&mut self.title) };
        (title, core::mem::take(&mut self.entries))
    }

    fn token(&mut self, t: Token) {
        match t {
            Token::Start { name, attrs } => {
                let name = local(name);
                self.path.push(String::from(name));
                self.text.clear();
                let in_entry = self.path.iter().any(|p| p == "entry");
                match name {
                    "entry" => self.cur = Some(Entry::default()),
                    "author" if in_entry => {
                        if let Some(c) = self.cur.as_mut() {
                            c.in_author = true;
                        }
                    }
                    "link" => {
                        let href = attr(attrs, "href").unwrap_or_default();
                        let rel = attr(attrs, "rel").unwrap_or_default();
                        let ty = attr(attrs, "type").unwrap_or_default();
                        if href.is_empty() {
                            return;
                        }
                        if !in_entry {
                            if rel == "next" {
                                self.next = Some(self.resolve(&href));
                            }
                            return;
                        }
                        let Some(c) = self.cur.as_mut() else { return };
                        let acquisition = rel.contains("opds-spec.org/acquisition");
                        let catalog = ty.contains("profile=opds-catalog") || ty.contains("application/atom+xml") || rel == "subsection";
                        if acquisition {
                            let rank = format_rank(&ty);
                            if c.e.acquisition.is_none() || rank < c.acq_rank {
                                c.acq_rank = rank;
                                c.e.acquisition = Some(href);
                            }
                        } else if catalog && c.e.nav.is_none() && !rel.contains("image") {
                            c.e.nav = Some(href);
                        }
                    }
                    _ => {}
                }
            }
            Token::Text(s) => {
                if self.text.len() < 4096 {
                    self.text.push_str(&s);
                }
            }
            Token::End { name } => {
                let name = local(name);
                let text = core::mem::take(&mut self.text);
                let depth = self.path.len();
                let in_entry = self.path.iter().any(|p| p == "entry");
                match name {
                    "title" if depth == 2 && !in_entry => self.title = html::to_text(&text, 120),
                    "title" if in_entry => {
                        if let Some(c) = self.cur.as_mut() {
                            if c.e.title.is_empty() {
                                c.e.title = html::to_text(&text, 160);
                            }
                        }
                    }
                    "name" if in_entry => {
                        if let Some(c) = self.cur.as_mut() {
                            if c.in_author && c.e.author.is_empty() {
                                c.e.author = html::to_text(&text, 80);
                            }
                        }
                    }
                    "author" => {
                        if let Some(c) = self.cur.as_mut() {
                            c.in_author = false;
                        }
                    }
                    "summary" | "content" if in_entry => {
                        if let Some(c) = self.cur.as_mut() {
                            if c.e.summary.is_empty() || name == "summary" {
                                c.e.summary = html::to_text(&text, SUMMARY_MAX);
                            }
                        }
                    }
                    "entry" => {
                        if let Some(mut c) = self.cur.take() {
                            if let Some(n) = c.e.nav.take() {
                                c.e.nav = Some(self.resolve(&n));
                            }
                            if let Some(a) = c.e.acquisition.take() {
                                c.e.acquisition = Some(self.resolve(&a));
                            }
                            if c.e.title.is_empty() {
                                c.e.title = String::from("Untitled");
                            }
                            if (c.e.nav.is_some() || c.e.acquisition.is_some()) && self.entries.len() < MAX_ENTRIES {
                                self.entries.push(c.e);
                            }
                        }
                    }
                    _ => {}
                }
                if let Some(i) = self.path.iter().rposition(|p| p == name) {
                    self.path.truncate(i);
                }
            }
        }
    }
}

/// Parse an OPDS 2.0 JSON feed.
pub fn parse_json(url: &str, json: &[u8]) -> Option<(String, Vec<OpdsEntry>)> {
    let base = Url::parse(url);
    let resolve = |href: String| base.as_ref().and_then(|b| b.resolve(&href)).map(|u| u.to_text()).unwrap_or(href);
    let v = jsonlite::parse(json)?;
    let title = v.path(&["metadata", "title"]).and_then(|t| t.as_str()).unwrap_or_else(|| String::from("Catalog"));
    let mut entries = Vec::new();
    if let Some(nav) = v.get("navigation") {
        for n in nav.items().take(MAX_ENTRIES) {
            let Some(href) = n.get("href").and_then(|h| h.as_str()) else { continue };
            entries.push(OpdsEntry {
                title: n.get("title").and_then(|t| t.as_str()).unwrap_or_else(|| String::from("Untitled")),
                nav: Some(resolve(href)),
                ..Default::default()
            });
        }
    }
    if let Some(pubs) = v.get("publications") {
        for p in pubs.items() {
            if entries.len() >= MAX_ENTRIES {
                break;
            }
            let meta = p.get("metadata");
            let title = meta.and_then(|m| m.get("title")).and_then(|t| t.as_str()).unwrap_or_else(|| String::from("Untitled"));
            let author = meta
                .and_then(|m| m.get("author"))
                .and_then(|a| {
                    a.as_str().or_else(|| a.get("name").and_then(|n| n.as_str())).or_else(|| {
                        a.items().next().and_then(|first| first.as_str().or_else(|| first.get("name").and_then(|n| n.as_str())))
                    })
                })
                .unwrap_or_default();
            let summary = meta
                .and_then(|m| m.get("description"))
                .and_then(|d| d.as_str())
                .map(|d| html::to_text(&d, SUMMARY_MAX))
                .unwrap_or_default();
            let mut best: Option<(usize, String)> = None;
            if let Some(links) = p.get("links") {
                for l in links.items() {
                    let rel = l.get("rel").and_then(|r| r.as_str()).unwrap_or_default();
                    let ty = l.get("type").and_then(|r| r.as_str()).unwrap_or_default();
                    let Some(href) = l.get("href").and_then(|h| h.as_str()) else { continue };
                    if rel.contains("acquisition") || ty.contains("epub") {
                        let rank = format_rank(&ty);
                        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                            best = Some((rank, href));
                        }
                    }
                }
            }
            if let Some((_, href)) = best {
                entries.push(OpdsEntry { title, author, nav: None, acquisition: Some(resolve(href)), summary });
            }
        }
    }
    Some((title, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog">
  <title>Calibre-Web &amp; friends</title>
  <link rel="next" href="/opds/new?page=2" type="application/atom+xml;profile=opds-catalog"/>
  <entry>
    <title>New Books</title>
    <link rel="subsection" href="/opds/new" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
    <content type="text">Recently added</content>
  </entry>
  <entry>
    <title>Pride and &lt;i&gt;Prejudice&lt;/i&gt;</title>
    <author><name>Jane Austen</name><uri>x</uri></author>
    <summary type="html">&lt;p&gt;Elizabeth Bennet.&lt;/p&gt;</summary>
    <link rel="http://opds-spec.org/image" href="/cover/1.jpg" type="image/jpeg"/>
    <link rel="http://opds-spec.org/acquisition" href="/download/1/mobi" type="application/x-mobipocket-ebook"/>
    <link rel="http://opds-spec.org/acquisition" href="/download/1/epub" type="application/epub+zip"/>
  </entry>
  <entry><title>Nothing</title></entry>
</feed>"#;

    #[test]
    fn atom_feed() {
        let mut p = FeedParser::new("https://books.example.org/opds/root.xml");
        for c in FEED.as_bytes().chunks(33) {
            p.push(c);
        }
        assert_eq!(p.next.as_deref(), Some("https://books.example.org/opds/new?page=2"));
        let (title, entries) = p.finish();
        assert_eq!(title, "Calibre-Web & friends");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "New Books");
        assert_eq!(entries[0].nav.as_deref(), Some("https://books.example.org/opds/new"));
        assert_eq!(entries[0].summary, "Recently added");
        assert_eq!(entries[1].title, "Pride and Prejudice");
        assert_eq!(entries[1].author, "Jane Austen");
        assert_eq!(entries[1].summary, "Elizabeth Bennet.");
        assert_eq!(entries[1].acquisition.as_deref(), Some("https://books.example.org/download/1/epub"));
        assert!(entries[1].nav.is_none());
    }

    #[test]
    fn json_feed() {
        let json = br#"{"metadata":{"title":"Palace"},"navigation":[{"href":"/new","title":"New"}],
          "publications":[{"metadata":{"title":"Walden","author":{"name":"Thoreau"},"description":"<b>Pond</b>"},
          "links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/w.epub","type":"application/epub+zip"}]},
          {"metadata":{"title":"No file"},"links":[]}]}"#;
        let (t, e) = parse_json("https://palace.example/opds2/", json).unwrap();
        assert_eq!(t, "Palace");
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].nav.as_deref(), Some("https://palace.example/new"));
        assert_eq!((e[1].title.as_str(), e[1].author.as_str(), e[1].summary.as_str()), ("Walden", "Thoreau", "Pond"));
        assert_eq!(e[1].acquisition.as_deref(), Some("https://palace.example/w.epub"));
    }
}

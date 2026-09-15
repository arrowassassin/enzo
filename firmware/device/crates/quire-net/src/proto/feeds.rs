//! RSS 2.0 and Atom feeds to news articles, streamed: each article is handed over as
//! its closing tag arrives, with its body already reduced to plain text, so a feed of
//! any size costs one article of RAM.

use alloc::string::String;
use alloc::vec::Vec;

use quire_ui::screens::apps::news::Article;

use super::html;
use super::xmlscan::{attr, Token, Tokenizer};

/// Body text kept per article (bytes).
pub const BODY_MAX: usize = 6 * 1024;
/// Articles taken per feed.
pub const MAX_ITEMS: usize = 15;

/// A streaming feed parser; `on_article` receives each article.
pub struct FeedParser<'f> {
    tok: Tokenizer,
    url: String,
    /// Feed title as seen so far.
    pub title: String,
    path: Vec<String>,
    text: String,
    cur: Option<Draft>,
    /// Articles emitted.
    pub count: usize,
    on_article: &'f mut dyn FnMut(Article),
}

#[derive(Default)]
struct Draft {
    title: String,
    link: String,
    published: u32,
    body: String,
    body_rank: u8,
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl<'f> FeedParser<'f> {
    /// A parser for the feed at `url`.
    pub fn new(url: &str, on_article: &'f mut dyn FnMut(Article)) -> Self {
        FeedParser {
            tok: Tokenizer::new(BODY_MAX + 2048),
            url: String::from(url),
            title: String::new(),
            path: Vec::new(),
            text: String::new(),
            cur: None,
            count: 0,
            on_article,
        }
    }

    /// Feed a chunk. Returns false once enough articles were taken (the caller may stop).
    pub fn push(&mut self, chunk: &[u8]) -> bool {
        let mut tok = core::mem::replace(&mut self.tok, Tokenizer::new(0));
        tok.push(chunk, &mut |t| self.token(t));
        self.tok = tok;
        self.count < MAX_ITEMS
    }

    /// End of input.
    pub fn finish(&mut self) {
        let mut tok = core::mem::replace(&mut self.tok, Tokenizer::new(0));
        tok.finish(&mut |t| self.token(t));
    }

    fn token(&mut self, t: Token) {
        match t {
            Token::Start { name, attrs } => {
                let name = local(name);
                self.path.push(String::from(name));
                self.text.clear();
                let in_item = self.in_item();
                match name {
                    "item" | "entry" => self.cur = Some(Draft::default()),
                    "link" if in_item => {
                        // Atom: href on the alternate link.
                        if let Some(href) = attr(attrs, "href") {
                            let rel = attr(attrs, "rel").unwrap_or_default();
                            if let Some(c) = self.cur.as_mut() {
                                if (rel.is_empty() || rel == "alternate") && c.link.is_empty() {
                                    c.link = href;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Token::Text(s) => {
                if self.text.len() < BODY_MAX * 2 {
                    self.text.push_str(&s);
                }
            }
            Token::End { name } => {
                let name = local(name);
                let text = core::mem::take(&mut self.text);
                let in_item = self.in_item();
                if in_item {
                    if let Some(c) = self.cur.as_mut() {
                        match name {
                            "title" => {
                                if c.title.is_empty() {
                                    c.title = html::to_text(&text, 200);
                                }
                            }
                            "link" => {
                                if c.link.is_empty() && !text.trim().is_empty() {
                                    c.link = String::from(text.trim());
                                }
                            }
                            "guid" | "id" => {
                                if c.link.is_empty() && text.trim().starts_with("http") {
                                    c.link = String::from(text.trim());
                                }
                            }
                            "pubDate" | "published" | "updated" | "date" => {
                                if c.published == 0 || name == "published" {
                                    c.published = parse_date(text.trim()).unwrap_or(0);
                                }
                            }
                            "encoded" | "content" | "description" | "summary" => {
                                let rank = match name {
                                    "encoded" | "content" => 3,
                                    "description" => 2,
                                    _ => 1,
                                };
                                if rank > c.body_rank {
                                    c.body_rank = rank;
                                    c.body = html::to_text(&text, BODY_MAX);
                                }
                            }
                            _ => {}
                        }
                    }
                } else if name == "title" && self.title.is_empty() && self.path.len() <= 3 {
                    self.title = html::to_text(&text, 80);
                }
                if name == "item" || name == "entry" {
                    if let Some(c) = self.cur.take() {
                        if !c.link.is_empty() && self.count < MAX_ITEMS {
                            self.count += 1;
                            let feed_title = if self.title.is_empty() { self.url.clone() } else { self.title.clone() };
                            (self.on_article)(Article {
                                feed: self.url.clone(),
                                feed_title,
                                title: if c.title.is_empty() { String::from("Untitled") } else { c.title },
                                url: c.link,
                                published: c.published,
                                text: c.body,
                                read: false,
                            });
                        }
                    }
                }
                if let Some(i) = self.path.iter().rposition(|p| p == name) {
                    self.path.truncate(i);
                }
            }
        }
    }

    fn in_item(&self) -> bool {
        self.path.iter().any(|p| p == "item" || p == "entry")
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn month_of(s: &str) -> Option<i64> {
    let m = s.get(..3)?.to_ascii_lowercase();
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|n| *n == m).map(|i| i as i64 + 1)
}

/// Parse an RFC 822 (`Mon, 15 Sep 2026 10:00:00 +0200`) or ISO 8601
/// (`2026-09-15T10:00:00Z`) date to seconds since the epoch (UTC).
pub fn parse_date(s: &str) -> Option<u32> {
    let s = s.trim();
    let (y, mo, d, h, mi, sec, off) = if s.len() >= 10 && s.as_bytes()[4] == b'-' {
        let y: i64 = s[0..4].parse().ok()?;
        let mo: i64 = s[5..7].parse().ok()?;
        let d: i64 = s[8..10].parse().ok()?;
        let rest = &s[10..];
        let (h, mi, sec, off) = if rest.len() >= 6 {
            let t = &rest[1..];
            let h: i64 = t.get(0..2)?.parse().ok()?;
            let mi: i64 = t.get(3..5)?.parse().ok()?;
            let sec: i64 = t.get(6..8).and_then(|x| x.parse().ok()).unwrap_or(0);
            let tz = &t[t.find(['Z', '+', '-']).unwrap_or(t.len())..];
            (h, mi, sec, tz_offset(tz))
        } else {
            (0, 0, 0, 0)
        };
        (y, mo, d, h, mi, sec, off)
    } else {
        let mut parts = s.split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty());
        let first = parts.next()?;
        let day_s = if first.bytes().all(|b| b.is_ascii_digit()) { first } else { parts.next()? };
        let d: i64 = day_s.parse().ok()?;
        let mo = month_of(parts.next()?)?;
        let y: i64 = parts.next()?.parse().ok()?;
        let y = if y < 100 { y + 2000 } else { y };
        let time = parts.next().unwrap_or("00:00:00");
        let mut t = time.split(':');
        let h: i64 = t.next()?.parse().ok()?;
        let mi: i64 = t.next().unwrap_or("0").parse().ok()?;
        let sec: i64 = t.next().unwrap_or("0").parse().ok()?;
        let off = tz_offset(parts.next().unwrap_or("+0000"));
        (y, mo, d, h, mi, sec, off)
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let secs = days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + sec - off;
    (secs > 0).then_some(secs as u32)
}

/// Seconds east of UTC for `+0200`, `+02:00`, `Z`, `GMT`, `EST`…
fn tz_offset(tz: &str) -> i64 {
    match tz {
        "" | "Z" | "GMT" | "UTC" | "UT" => 0,
        "EST" => -5 * 3600,
        "EDT" => -4 * 3600,
        "CST" => -6 * 3600,
        "CDT" => -5 * 3600,
        "MST" => -7 * 3600,
        "MDT" => -6 * 3600,
        "PST" => -8 * 3600,
        "PDT" => -7 * 3600,
        t => {
            let sign = if t.starts_with('-') { -1 } else { 1 };
            let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
            let (h, m) = match digits.len() {
                4 => (digits[..2].parse().unwrap_or(0), digits[2..].parse().unwrap_or(0)),
                2 => (digits.parse().unwrap_or(0), 0),
                _ => (0i64, 0i64),
            };
            sign * (h * 3600 + m * 60)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?><rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/">
<channel><title>Example News</title><link>https://ex.org</link>
<item><title>First &amp; foremost</title><link>https://ex.org/1</link><pubDate>Mon, 14 Sep 2026 10:00:00 +0200</pubDate>
<description>&lt;p&gt;Short.&lt;/p&gt;</description><content:encoded><![CDATA[<p>Long <b>body</b>.</p><p>Two.</p>]]></content:encoded></item>
<item><title>No link</title><description>x</description></item>
<item><title>Third</title><guid>https://ex.org/3</guid><pubDate>Tue, 15 Sep 2026 08:00:00 GMT</pubDate><description>Body 3</description></item>
</channel></rss>"#;

    const ATOM: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom Blog</title>
<entry><title>Post</title><link rel="alternate" href="https://blog/1"/><link rel="enclosure" href="https://blog/1.mp3"/>
<published>2026-09-15T10:00:00+02:00</published><updated>2026-09-16T10:00:00Z</updated>
<summary>Sum</summary><content type="html">&lt;p&gt;Full&lt;/p&gt;</content></entry></feed>"#;

    #[test]
    fn rss_items() {
        let mut out = Vec::new();
        let mut sink = |a: Article| out.push(a);
        let mut p = FeedParser::new("https://ex.org/feed", &mut sink);
        for c in RSS.as_bytes().chunks(50) {
            p.push(c);
        }
        p.finish();
        assert_eq!(p.title, "Example News");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "First & foremost");
        assert_eq!(out[0].url, "https://ex.org/1");
        assert_eq!(out[0].text, "Long body.\n\nTwo.");
        assert_eq!(out[0].published, parse_date("Mon, 14 Sep 2026 08:00:00 GMT").unwrap());
        assert_eq!(out[0].feed_title, "Example News");
        assert_eq!(out[1].url, "https://ex.org/3");
        assert_eq!(out[1].text, "Body 3");
    }

    #[test]
    fn atom_entries() {
        let mut out = Vec::new();
        let mut sink = |a: Article| out.push(a);
        let mut p = FeedParser::new("https://blog/feed", &mut sink);
        p.push(ATOM.as_bytes());
        p.finish();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].url, "https://blog/1");
        assert_eq!(out[0].text, "Full");
        assert_eq!(out[0].published, parse_date("2026-09-15T08:00:00Z").unwrap());
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date("Thu, 01 Jan 1970 00:00:10 GMT"), Some(10));
        assert_eq!(parse_date("1970-01-01T00:00:10Z"), Some(10));
        assert_eq!(parse_date("1970-01-01T02:00:10+02:00"), Some(10));
        assert_eq!(parse_date("01 Jan 1970 01:00:10 +0100"), Some(10));
        assert_eq!(parse_date("2026-09-15"), parse_date("Tue, 15 Sep 2026 00:00:00 +0000"));
        assert_eq!(parse_date("garbage"), None);
    }
}

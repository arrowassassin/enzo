//! URLs the fetchers see: `http(s)://host[:port]/path?query`, relative redirect targets,
//! and percent-encoding for query values.

use alloc::string::String;
use core::fmt::Write;

/// A parsed absolute HTTP(S) URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    /// Whether the scheme is `https`.
    pub https: bool,
    /// Host name (or address) as written, lower-cased.
    pub host: String,
    /// Port (default for the scheme when not given).
    pub port: u16,
    /// Path with query, always starting with `/`.
    pub path: String,
}

impl Url {
    /// Parse; `None` for anything but an absolute http(s) URL with a host.
    pub fn parse(s: &str) -> Option<Url> {
        let s = s.trim();
        let (https, rest) = s.strip_prefix("https://").map(|r| (true, r)).or_else(|| s.strip_prefix("http://").map(|r| (false, r)))?;
        let (authority, path) = match rest.find(['/', '?', '#']) {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let authority = authority.rsplit_once('@').map(|(_, h)| h).unwrap_or(authority);
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h, p.parse().ok()?),
            Some((h, "")) => (h, if https { 443 } else { 80 }),
            _ => (authority, if https { 443 } else { 80 }),
        };
        if host.is_empty() || host.contains(|c: char| c.is_whitespace()) {
            return None;
        }
        let path = path.split('#').next().unwrap_or("");
        let path = if path.is_empty() || path.starts_with('?') { alloc::format!("/{path}") } else { String::from(path) };
        Some(Url { https, host: host.to_ascii_lowercase(), port, path })
    }

    /// Whether the port is the scheme's default.
    pub fn default_port(&self) -> bool {
        self.port == if self.https { 443 } else { 80 }
    }

    /// `host` or `host:port` for the `Host` header.
    pub fn host_header(&self) -> String {
        if self.default_port() {
            self.host.clone()
        } else {
            alloc::format!("{}:{}", self.host, self.port)
        }
    }

    /// The URL as text.
    pub fn to_text(&self) -> String {
        alloc::format!("{}://{}{}", if self.https { "https" } else { "http" }, self.host_header(), self.path)
    }

    /// The directory of the path (through the last `/`), for relative references.
    fn dir(&self) -> &str {
        let p = self.path.split('?').next().unwrap_or("/");
        &p[..p.rfind('/').map(|i| i + 1).unwrap_or(1)]
    }

    /// Resolve a reference (absolute, scheme-relative, absolute-path or relative).
    pub fn resolve(&self, href: &str) -> Option<Url> {
        let href = href.trim();
        if href.starts_with("http://") || href.starts_with("https://") {
            return Url::parse(href);
        }
        if let Some(rest) = href.strip_prefix("//") {
            return Url::parse(&alloc::format!("{}://{rest}", if self.https { "https" } else { "http" }));
        }
        let mut out = self.clone();
        out.path = if href.starts_with('/') {
            String::from(href)
        } else if href.starts_with('?') {
            alloc::format!("{}{href}", self.path.split('?').next().unwrap_or("/"))
        } else {
            alloc::format!("{}{href}", self.dir())
        };
        out.path = normalize(&out.path);
        Some(out)
    }
}

/// Collapse `.` and `..` segments in an absolute path (the query is kept).
fn normalize(path: &str) -> String {
    let (p, q) = match path.find('?') {
        Some(i) => (&path[..i], &path[i..]),
        None => (path, ""),
    };
    let mut segs: alloc::vec::Vec<&str> = alloc::vec::Vec::new();
    let trailing = p.ends_with('/') || p.ends_with("/.") || p.ends_with("/..");
    for s in p.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    let mut out = String::from("/");
    out.push_str(&segs.join("/"));
    if trailing && !out.ends_with('/') {
        out.push('/');
    }
    out.push_str(q);
    out
}

/// Percent-encode a query value or path segment (RFC 3986 unreserved kept).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The last path segment, percent-decoded, or empty.
pub fn file_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or("");
    quire_fs::percent_decode(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let u = Url::parse("https://www.gutenberg.org/ebooks/1342.epub3.images").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.path.as_str()), (true, "www.gutenberg.org", 443, "/ebooks/1342.epub3.images"));
        let u = Url::parse("http://Calibre.local:8080").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.path.as_str()), (false, "calibre.local", 8080, "/"));
        assert_eq!(u.host_header(), "calibre.local:8080");
        assert_eq!(Url::parse("https://a.b/c?d=1#frag").unwrap().path, "/c?d=1");
        assert_eq!(Url::parse("https://a.b?x=1").unwrap().path, "/?x=1");
        assert!(Url::parse("ftp://x").is_none());
        assert!(Url::parse("https://").is_none());
        assert_eq!(Url::parse("https://user:pw@h.io/p").unwrap().host, "h.io");
    }

    #[test]
    fn resolves() {
        let base = Url::parse("https://example.org/opds/root.xml?x=1").unwrap();
        assert_eq!(base.resolve("new.xml").unwrap().to_text(), "https://example.org/opds/new.xml");
        assert_eq!(base.resolve("/books/1.epub").unwrap().to_text(), "https://example.org/books/1.epub");
        assert_eq!(base.resolve("../a/./b/../c").unwrap().to_text(), "https://example.org/a/c");
        assert_eq!(base.resolve("//cdn.example.org/x").unwrap().to_text(), "https://cdn.example.org/x");
        assert_eq!(base.resolve("http://other/y").unwrap().to_text(), "http://other/y");
        assert_eq!(base.resolve("?page=2").unwrap().to_text(), "https://example.org/opds/root.xml?page=2");
    }

    #[test]
    fn encodes() {
        assert_eq!(percent_encode("São Paulo/x"), "S%C3%A3o%20Paulo%2Fx");
        assert_eq!(file_name("https://h/a/pg1342%20b.epub?x=1"), "pg1342 b.epub");
    }
}

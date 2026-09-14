//! Markdown to QTX.
//!
//! An in-crate CommonMark parser (plus GFM tables, strikethrough and footnotes) for
//! `no_std` + `alloc`. The block parser walks the source line by line and hands every
//! leaf block to the emitter the moment it closes, so only one block's inline tree is
//! ever held; the emitter maps blocks and inlines onto QTX exactly as the earlier
//! pulldown-cmark based converter did.
//!
//! Two passes run over the source: the first collects link reference definitions,
//! footnote labels and which lists are loose (all of which can only be known once the
//! whole document has been seen), the second emits tokens. Both are streaming.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_qtx::{style, ParaKind, Token, Writer};

use crate::{DocError, Metadata, Sink, TocEntry};

/// Result of a Markdown conversion: QTX bytes, char count, first heading, TOC (title, depth).
pub type MdOutput = (Vec<u8>, u32, Option<String>, Vec<(String, u8)>);

/// Convert Markdown text to QTX, returning bytes, char count, first heading and TOC.
pub fn to_qtx(src: &str) -> MdOutput {
    let mut defs = Defs::default();
    {
        let mut pre = Prepass { defs: &mut defs };
        Blocks::new(src, &mut pre).run();
    }
    defs.loose.sort_unstable();
    let mut em = Emitter::new(&defs);
    Blocks::new(src, &mut em).run();
    em.finish()
}

/// Ingest a Markdown file as a single chapter.
pub fn ingest<R: ReadAt>(file: &R, name: &str, sink: &mut dyn Sink) -> Result<(), DocError> {
    let len = file.len() as usize;
    if len > 4 * 1024 * 1024 {
        return Err(DocError::TooLarge("markdown over 4 MB"));
    }
    let data = file.read_range(0, len)?;
    let text = crate::txt::decode_text(&data);
    let (bytes, chars, heading, toc) = to_qtx(&text);
    let title = heading.unwrap_or_else(|| crate::title_from_name(name));
    sink.metadata(&Metadata { title: title.clone(), ..Default::default() })?;
    sink.begin_chapter(0, Some(&title))?;
    sink.chapter_bytes(&bytes)?;
    sink.end_chapter(chars)?;
    let entries: Vec<TocEntry> = if toc.is_empty() {
        alloc::vec![TocEntry { title, chapter: 0, anchor: None, depth: 0 }]
    } else {
        toc.into_iter().map(|(t, l)| TocEntry { title: t, chapter: 0, anchor: None, depth: l.saturating_sub(1) }).collect()
    };
    sink.toc(&entries)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Document-wide definitions gathered by the first pass.
// ---------------------------------------------------------------------------------------

#[derive(Default)]
struct Defs {
    /// Link reference definitions, normalised label → destination (first wins).
    refs: BTreeMap<String, String>,
    /// Footnote definition labels, normalised.
    footnotes: BTreeSet<String>,
    /// Source offsets of the first marker of every loose list, sorted.
    loose: Vec<u32>,
}

struct Prepass<'d> {
    defs: &'d mut Defs,
}

impl BlockSink for Prepass<'_> {
    fn wants_text(&self) -> bool {
        false
    }
    fn link_def(&mut self, label: String, url: String) {
        self.defs.refs.entry(label).or_insert(url);
    }
    fn footnote_label(&mut self, label: &str) {
        self.defs.footnotes.insert(normalize_label(label));
    }
    fn list_loose(&mut self, off: u32) {
        self.defs.loose.push(off);
    }
}

// ---------------------------------------------------------------------------------------
// Character helpers.
// ---------------------------------------------------------------------------------------

fn is_sp(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn is_blank(b: &[u8]) -> bool {
    b.iter().all(|&c| is_sp(c))
}

fn skip_sp(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && is_sp(b[i]) {
        i += 1;
    }
    i
}

fn as_str(b: &[u8]) -> &str {
    // Every slice handed here was cut at an ASCII byte, so this cannot fail.
    core::str::from_utf8(b).unwrap_or("")
}

/// Unicode punctuation as CommonMark defines it (P* and S* categories), approximated
/// with the ranges that matter for prose.
fn is_punct(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_punctuation();
    }
    matches!(c as u32,
        0xA1..=0xA9 | 0xAB..=0xB4 | 0xB6..=0xB9 | 0xBB..=0xBF | 0xD7 | 0xF7
        | 0x2010..=0x2027 | 0x2030..=0x205E | 0x2070 | 0x207A..=0x207E | 0x208A..=0x208E
        | 0x20A0..=0x20C0 | 0x2100..=0x2101 | 0x2103..=0x2106 | 0x2108..=0x2109 | 0x2114
        | 0x2116..=0x2118 | 0x211E..=0x2123 | 0x2125 | 0x2127 | 0x2129 | 0x212E
        | 0x213A..=0x213B | 0x2140..=0x2144 | 0x214A..=0x214D | 0x214F | 0x218A..=0x218B
        | 0x2190..=0x2426 | 0x2440..=0x244A | 0x249C..=0x24E9 | 0x2500..=0x2775
        | 0x2794..=0x2B73 | 0x2B76..=0x2B95 | 0x2B97..=0x2BFF | 0x2CE5..=0x2CEA
        | 0x2CF9..=0x2CFC | 0x2CFE..=0x2CFF | 0x2E00..=0x2E5D | 0x3001..=0x3004
        | 0x3008..=0x3020 | 0x3030 | 0x303D | 0x30A0 | 0x30FB | 0xFE10..=0xFE19
        | 0xFE30..=0xFE52 | 0xFE54..=0xFE66 | 0xFE68..=0xFE6B | 0xFF01..=0xFF0F
        | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65 | 0xFFE0..=0xFFE6
        | 0xFFE8..=0xFFEE | 0x1F000..=0x1FAFF)
}

/// Decode an HTML entity at the start of `s` (which begins with `&`): (char, bytes used).
fn entity(s: &str) -> Option<(char, usize)> {
    let b = s.as_bytes();
    if b.get(1) == Some(&b'#') {
        let (hex, ds) = if matches!(b.get(2), Some(b'x' | b'X')) { (true, 3) } else { (false, 2) };
        let mut i = ds;
        while i < b.len() && i - ds < 8 && (if hex { b[i].is_ascii_hexdigit() } else { b[i].is_ascii_digit() }) {
            i += 1;
        }
        let digits = i - ds;
        if digits == 0 || digits > if hex { 6 } else { 7 } || b.get(i) != Some(&b';') {
            return None;
        }
        let v = u32::from_str_radix(&s[ds..i], if hex { 16 } else { 10 }).unwrap_or(0);
        let c = if v == 0 { '\u{FFFD}' } else { char::from_u32(v).unwrap_or('\u{FFFD}') };
        return Some((c, i + 1));
    }
    let mut i = 1;
    while i < b.len() && i < 33 && b[i].is_ascii_alphanumeric() {
        i += 1;
    }
    if i == 1 || b.get(i) != Some(&b';') {
        return None;
    }
    let name = &s[1..i];
    let c = match name {
        "AMP" => '&',
        "LT" => '<',
        "GT" => '>',
        "QUOT" => '"',
        "COPY" => '©',
        "REG" => '®',
        _ => crate::html::named_entity(name)?,
    };
    Some((c, i + 1))
}

/// Append `raw` to `out`, resolving backslash escapes and HTML entities.
fn decode_into(out: &mut String, raw: &str) {
    let b = raw.as_bytes();
    let mut i = 0;
    let mut start = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() && b[i + 1].is_ascii_punctuation() => {
                out.push_str(&raw[start..i]);
                start = i + 1;
                i += 2;
            }
            b'&' => {
                if let Some((c, n)) = entity(&raw[i..]) {
                    out.push_str(&raw[start..i]);
                    out.push(c);
                    i += n;
                    start = i;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out.push_str(&raw[start..]);
}

/// Link label normalisation: trim, collapse whitespace, case fold.
fn normalize_label(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// A label is at most 999 bytes, has a non-blank character and no unescaped brackets.
fn valid_label(s: &str) -> bool {
    if s.len() > 999 || s.trim().is_empty() {
        return false;
    }
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'[' | b']' => return false,
            _ => i += 1,
        }
    }
    true
}

/// Given `b[from..]` is label content after `[`, return the index of its closing `]`.
fn scan_label_end(b: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() && i - from <= 999 {
        match b[i] {
            b'\\' => i += 2,
            b'[' => return None,
            b']' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

// ---------------------------------------------------------------------------------------
// Line scanner: column-aware whitespace handling for block structure (tabs stop at 4).
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Ls<'a> {
    b: &'a [u8],
    ix: usize,
    tab_start: usize,
    /// Columns of a partially consumed tab still logically before `ix`.
    rem: usize,
}

impl<'a> Ls<'a> {
    fn new(b: &'a [u8]) -> Self {
        Ls { b, ix: 0, tab_start: 0, rem: 0 }
    }
    /// Consume up to `n` columns of whitespace; returns the columns still wanted.
    fn space_inner(&mut self, mut n: usize) -> usize {
        let take = self.rem.min(n);
        self.rem -= take;
        n -= take;
        while n > 0 && self.ix < self.b.len() {
            match self.b[self.ix] {
                b' ' => {
                    self.ix += 1;
                    n -= 1;
                }
                b'\t' => {
                    let sp = 4 - (self.ix - self.tab_start) % 4;
                    self.ix += 1;
                    self.tab_start = self.ix;
                    let t = sp.min(n);
                    n -= t;
                    self.rem = sp - t;
                }
                _ => break,
            }
        }
        n
    }
    fn scan_space(&mut self, n: usize) -> bool {
        self.space_inner(n) == 0
    }
    fn scan_space_upto(&mut self, n: usize) -> usize {
        n - self.space_inner(n)
    }
    fn scan_all_space(&mut self) {
        self.rem = 0;
        self.ix = skip_sp(self.b, self.ix);
    }
    fn at_eol(&self) -> bool {
        self.ix >= self.b.len()
    }
    fn blank(&self) -> bool {
        is_blank(self.rest())
    }
    fn rest(&self) -> &'a [u8] {
        &self.b[self.ix..]
    }
    fn scan_ch(&mut self, c: u8) -> bool {
        if self.ix < self.b.len() && self.b[self.ix] == c {
            self.ix += 1;
            true
        } else {
            false
        }
    }
    fn scan_quote(&mut self) -> bool {
        if self.scan_ch(b'>') {
            let _ = self.scan_space(1);
            true
        } else {
            false
        }
    }
    /// A list marker at the current position: (marker char or delimiter, start, content
    /// indent measured from the container start, `indent` columns already consumed).
    fn scan_list_marker(&mut self, indent: usize) -> Option<(u8, u64, usize)> {
        let save = *self;
        if let Some(&c) = self.b.get(self.ix) {
            if c == b'-' || c == b'+' || c == b'*' {
                if scan_hrule(self.rest()) {
                    return None;
                }
                self.ix += 1;
                if self.scan_space(1) || self.at_eol() {
                    return self.finish_marker(c, 0, indent + 2);
                }
            } else if c.is_ascii_digit() {
                let start_ix = self.ix;
                let mut ix = self.ix + 1;
                let mut val = u64::from(c - b'0');
                while ix < self.b.len() && ix - start_ix < 10 {
                    let c = self.b[ix];
                    ix += 1;
                    if c.is_ascii_digit() {
                        val = val * 10 + u64::from(c - b'0');
                    } else if c == b')' || c == b'.' {
                        self.ix = ix;
                        if self.scan_space(1) || self.at_eol() {
                            return self.finish_marker(c, val, indent + 1 + ix - start_ix);
                        }
                        break;
                    } else {
                        break;
                    }
                }
            }
        }
        *self = save;
        None
    }
    fn finish_marker(&mut self, c: u8, start: u64, mut indent: usize) -> Option<(u8, u64, usize)> {
        let save = *self;
        if self.blank() {
            return Some((c, start, indent));
        }
        let post = self.scan_space_upto(4);
        if post < 4 {
            indent += post;
        } else {
            *self = save;
        }
        Some((c, start, indent))
    }
}

/// End of the line starting at `pos`: (content end, next line start).
fn line_end(b: &[u8], pos: usize) -> (usize, usize) {
    let mut i = pos;
    while i < b.len() {
        match b[i] {
            b'\n' => return (i, i + 1),
            b'\r' => return (i, if b.get(i + 1) == Some(&b'\n') { i + 2 } else { i + 1 }),
            _ => i += 1,
        }
    }
    (i, i)
}

// ---------------------------------------------------------------------------------------
// Block-level scanners over a line's remaining bytes.
// ---------------------------------------------------------------------------------------

fn scan_hrule(r: &[u8]) -> bool {
    let Some(&c) = r.first() else {
        return false;
    };
    if !matches!(c, b'*' | b'-' | b'_') {
        return false;
    }
    let mut n = 0;
    for &x in r {
        if x == c {
            n += 1;
        } else if !is_sp(x) {
            return false;
        }
    }
    n >= 3
}

/// ATX heading: (level, index of the content).
fn scan_atx(r: &[u8]) -> Option<(u8, usize)> {
    let n = r.iter().take_while(|&&c| c == b'#').count();
    if n == 0 || n > 6 || !r.get(n).is_none_or(|&c| is_sp(c)) {
        return None;
    }
    Some((n as u8, n))
}

/// Heading text after the opening hashes: trimmed, closing sequence removed.
fn atx_content(r: &[u8]) -> &[u8] {
    let s = skip_sp(r, 0);
    let mut e = r.len();
    while e > s && is_sp(r[e - 1]) {
        e -= 1;
    }
    let mut h = e;
    while h > s && r[h - 1] == b'#' {
        h -= 1;
    }
    if h < e && (h == s || is_sp(r[h - 1])) {
        e = h;
        while e > s && is_sp(r[e - 1]) {
            e -= 1;
        }
    }
    &r[s..e]
}

/// Opening code fence: (fence char, length).
fn scan_fence(r: &[u8]) -> Option<(u8, usize)> {
    let &c = r.first()?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let n = r.iter().take_while(|&&x| x == c).count();
    if n < 3 || (c == b'`' && r[n..].contains(&b'`')) {
        return None;
    }
    Some((c, n))
}

fn scan_close_fence(r: &[u8], c: u8, n: usize) -> bool {
    let k = r.iter().take_while(|&&x| x == c).count();
    k >= n && is_blank(&r[k..])
}

fn scan_setext(r: &[u8]) -> Option<u8> {
    let &c = r.first()?;
    let level = match c {
        b'=' => 1,
        b'-' => 2,
        _ => return None,
    };
    let n = r.iter().take_while(|&&x| x == c).count();
    is_blank(&r[n..]).then_some(level)
}

/// Byte-level list marker probe for paragraph interruption: (char, index, blank after).
fn list_probe(r: &[u8]) -> Option<(u8, u64, bool)> {
    let &c = r.first()?;
    let (w, ch, idx) = if matches!(c, b'-' | b'+' | b'*') {
        (1, c, 0)
    } else if c.is_ascii_digit() {
        let n = r.iter().take(9).take_while(|&&x| x.is_ascii_digit()).count();
        let d = *r.get(n)?;
        if d != b'.' && d != b')' {
            return None;
        }
        let idx = as_str(&r[..n]).parse::<u64>().unwrap_or(0);
        (n + 1, d, idx)
    } else {
        return None;
    };
    if !r.get(w).is_none_or(|&x| is_sp(x)) {
        return None;
    }
    Some((ch, idx, is_blank(&r[w..])))
}

/// `[^label]:` footnote definition marker: (label, bytes used).
fn scan_footnote_def(r: &[u8]) -> Option<(&str, usize)> {
    if !r.starts_with(b"[^") {
        return None;
    }
    let end = scan_label_end(r, 2)?;
    if end == 2 || r.get(end + 1) != Some(&b':') {
        return None;
    }
    Some((as_str(&r[2..end]), end + 2))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HtmlEnd {
    /// Types 1–5 end on a line containing a closing pattern.
    Tag,
    Comment,
    Pi,
    Decl,
    Cdata,
    /// Types 6 and 7 end at a blank line.
    Blank,
}

const HTML_BLOCK_TAGS: [&str; 62] = [
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    hay.len() >= needle.len() && hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

fn tag_name_end(r: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < r.len() && (r[i].is_ascii_alphanumeric() || r[i] == b'-') {
        i += 1;
    }
    i
}

/// The kind of HTML block a line starting with `<` opens, if any.
fn html_block_start(r: &[u8], allow_type7: bool) -> Option<HtmlEnd> {
    if r.len() < 2 {
        return None;
    }
    let after = &r[1..];
    for t in [&b"pre"[..], b"script", b"style", b"textarea"] {
        if after.len() >= t.len() && after[..t.len()].eq_ignore_ascii_case(t) && after.get(t.len()).is_none_or(|&c| is_sp(c) || c == b'>') {
            return Some(HtmlEnd::Tag);
        }
    }
    if after.starts_with(b"!--") {
        return Some(HtmlEnd::Comment);
    }
    if after.starts_with(b"?") {
        return Some(HtmlEnd::Pi);
    }
    if after.starts_with(b"![CDATA[") {
        return Some(HtmlEnd::Cdata);
    }
    if after.len() >= 2 && after[0] == b'!' && after[1].is_ascii_alphabetic() {
        return Some(HtmlEnd::Decl);
    }
    let close = usize::from(after.first() == Some(&b'/'));
    let ne = tag_name_end(r, 1 + close);
    if ne == 1 + close || !r[1 + close].is_ascii_alphabetic() {
        return None;
    }
    let name = &r[1 + close..ne];
    let tail = &r[ne..];
    if HTML_BLOCK_TAGS.iter().any(|t| t.as_bytes().eq_ignore_ascii_case(name))
        && (tail.is_empty() || is_sp(tail[0]) || tail[0] == b'>' || tail.starts_with(b"/>"))
    {
        return Some(HtmlEnd::Blank);
    }
    if allow_type7 && !["pre", "script", "style", "textarea"].iter().any(|t| t.as_bytes().eq_ignore_ascii_case(name)) {
        let end = if close == 1 {
            let i = skip_sp(r, ne);
            (r.get(i) == Some(&b'>')).then_some(i + 1)
        } else {
            scan_open_tag_rest(r, ne)
        };
        if let Some(end) = end {
            if is_blank(&r[end..]) {
                return Some(HtmlEnd::Blank);
            }
        }
    }
    None
}

fn html_end(kind: HtmlEnd, line: &[u8]) -> bool {
    match kind {
        HtmlEnd::Tag => [&b"</pre>"[..], b"</script>", b"</style>", b"</textarea>"].iter().any(|t| contains_ci(line, t)),
        HtmlEnd::Comment => contains_ci(line, b"-->"),
        HtmlEnd::Pi => contains_ci(line, b"?>"),
        HtmlEnd::Decl => line.contains(&b'>'),
        HtmlEnd::Cdata => contains_ci(line, b"]]>"),
        HtmlEnd::Blank => false,
    }
}

/// Attributes and the closing `>` of an open tag whose name ends at `i`; returns the
/// index after `>`. Whitespace may include line endings.
fn scan_open_tag_rest(b: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let ws_start = i;
        while i < b.len() && (is_sp(b[i]) || b[i] == b'\n') {
            i += 1;
        }
        match b.get(i) {
            Some(b'>') => return Some(i + 1),
            Some(b'/') => return (b.get(i + 1) == Some(&b'>')).then_some(i + 2),
            Some(&c) if i > ws_start && (c.is_ascii_alphabetic() || c == b'_' || c == b':') => {
                i += 1;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'.' | b':' | b'-')) {
                    i += 1;
                }
                let mut j = i;
                while j < b.len() && (is_sp(b[j]) || b[j] == b'\n') {
                    j += 1;
                }
                if b.get(j) == Some(&b'=') {
                    j += 1;
                    while j < b.len() && (is_sp(b[j]) || b[j] == b'\n') {
                        j += 1;
                    }
                    match b.get(j) {
                        Some(&q) if q == b'"' || q == b'\'' => {
                            let e = b[j + 1..].iter().position(|&c| c == q)?;
                            i = j + 2 + e;
                        }
                        Some(&c) if !is_sp(c) && !matches!(c, b'"' | b'\'' | b'=' | b'<' | b'>' | b'`' | b'\n') => {
                            i = j;
                            while i < b.len() && !is_sp(b[i]) && !matches!(b[i], b'"' | b'\'' | b'=' | b'<' | b'>' | b'`' | b'\n') {
                                i += 1;
                            }
                        }
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
}

/// GFM table delimiter row: number of columns.
fn scan_table_head(r: &[u8]) -> Option<usize> {
    let mut i = skip_sp(r, 0);
    if i > 3 || i == r.len() {
        return None;
    }
    let mut cols = 0;
    let mut start_col = true;
    let mut found_pipe = false;
    let mut found_hyphen = false;
    let mut hyphen_in_col = false;
    if r[i] == b'|' {
        i += 1;
        found_pipe = true;
    }
    while i < r.len() {
        match r[i] {
            b' ' => {}
            b':' => start_col = false,
            b'-' => {
                start_col = false;
                found_hyphen = true;
                hyphen_in_col = true;
            }
            b'|' => {
                start_col = true;
                found_pipe = true;
                cols += 1;
                if !hyphen_in_col {
                    return None;
                }
                hyphen_in_col = false;
            }
            _ => return None,
        }
        i += 1;
    }
    if !start_col {
        cols += 1;
    }
    (found_pipe && found_hyphen).then_some(cols)
}

/// Unescaped pipes on a line: (count, index of the last one).
fn count_pipes(r: &[u8]) -> (usize, usize) {
    let mut pipes = 0;
    let mut last = 0;
    let mut esc = false;
    for (i, &c) in r.iter().enumerate() {
        match c {
            b'\\' => {
                esc = true;
                continue;
            }
            b'|' if !esc => {
                pipes += 1;
                last = i;
            }
            _ => {}
        }
        esc = false;
    }
    (pipes, last)
}

/// Header cell count implied by a line's pipes.
fn header_cols(r: &[u8], mut pipes: usize, last: usize) -> usize {
    let s = skip_sp(r, 0);
    if r.get(s) == Some(&b'|') {
        pipes = pipes.saturating_sub(1);
    }
    if is_blank(&r[last + 1..]) {
        pipes
    } else {
        pipes + 1
    }
}

/// Table row cells as byte ranges (trimmed), split at unescaped pipes.
fn split_cells(r: &[u8], out: &mut Vec<(usize, usize)>) {
    out.clear();
    let mut i = 0;
    loop {
        if i < r.len() && r[i] == b'|' {
            i += 1;
        }
        let start = skip_sp(r, i);
        if start >= r.len() {
            break;
        }
        let mut j = start;
        while j < r.len() {
            match r[j] {
                b'\\' => j += 2,
                b'|' => break,
                _ => j += 1,
            }
        }
        let j = j.min(r.len());
        let mut e = j;
        while e > start && is_sp(r[e - 1]) {
            e -= 1;
        }
        out.push((start, e));
        i = j;
    }
}

// ---------------------------------------------------------------------------------------
// Link reference definitions and link syntax shared by blocks and inlines.
// ---------------------------------------------------------------------------------------

/// Spaces and tabs, at most one line ending, spaces and tabs.
fn skip_ws_one_nl(b: &[u8], i: usize) -> usize {
    let mut i = skip_sp(b, i);
    if b.get(i) == Some(&b'\n') {
        i = skip_sp(b, i + 1);
    }
    i
}

/// Link destination at `i`: (decoded url, index after it). A bare destination must be
/// non-empty.
fn scan_link_dest(t: &str, i: usize) -> Option<(String, usize)> {
    let b = t.as_bytes();
    let mut url = String::new();
    if b.get(i) == Some(&b'<') {
        let mut j = i + 1;
        loop {
            match b.get(j) {
                None | Some(b'\n') | Some(b'<') => return None,
                Some(b'\\') => j += 2,
                Some(b'>') => break,
                _ => j += 1,
            }
        }
        let j = j.min(b.len());
        decode_into(&mut url, &t[i + 1..j]);
        return Some((url, j + 1));
    }
    let mut j = i;
    let mut depth = 0usize;
    while j < b.len() {
        let c = b[j];
        if c == b'\\' && j + 1 < b.len() && b[j + 1].is_ascii_punctuation() {
            j += 2;
            continue;
        }
        if c <= 0x20 || c == 0x7f {
            break;
        }
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            if depth == 0 {
                break;
            }
            depth -= 1;
        }
        j += 1;
    }
    if j == i || depth != 0 {
        return None;
    }
    decode_into(&mut url, &t[i..j]);
    Some((url, j))
}

/// Link title at `i`: index after its closing delimiter.
fn scan_link_title(b: &[u8], i: usize) -> Option<usize> {
    let close = match *b.get(i)? {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'\n' if b.get(j + 1) == Some(&b'\n') => return None,
            b'(' if close == b')' => return None,
            c if c == close => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// A link reference definition at the start of `t`: (normalised label, url, bytes used).
fn parse_ref_def(t: &str) -> Option<(String, String, usize)> {
    let b = t.as_bytes();
    if b.first() != Some(&b'[') {
        return None;
    }
    let lend = scan_label_end(b, 1)?;
    let label = &t[1..lend];
    if label.trim().is_empty() || b.get(lend + 1) != Some(&b':') {
        return None;
    }
    let i = skip_ws_one_nl(b, lend + 2);
    let (url, i) = scan_link_dest(t, i)?;
    let label = normalize_label(label);
    let after = skip_sp(b, i);
    if after == b.len() {
        return Some((label, url, after));
    }
    if b[after] == b'\n' {
        let j = skip_ws_one_nl(b, i);
        if j > after {
            if let Some(k) = scan_link_title(b, j) {
                let k2 = skip_sp(b, k);
                if k2 == b.len() || b[k2] == b'\n' {
                    return Some((label, url, (k2 + 1).min(b.len())));
                }
            }
        }
        return Some((label, url, after + 1));
    }
    if after > i {
        if let Some(k) = scan_link_title(b, after) {
            let k2 = skip_sp(b, k);
            if k2 == b.len() || b[k2] == b'\n' {
                return Some((label, url, (k2 + 1).min(b.len())));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------------------
// Block parser.
// ---------------------------------------------------------------------------------------

/// Receiver of block structure. Inline content arrives as raw Markdown text per block.
trait BlockSink {
    /// Whether paragraph text should be accumulated (the first pass only needs it when
    /// it may hold a reference definition).
    fn wants_text(&self) -> bool {
        true
    }
    fn link_def(&mut self, _label: String, _url: String) {}
    fn footnote_label(&mut self, _label: &str) {}
    fn list_loose(&mut self, _off: u32) {}
    fn is_loose(&self, _off: u32) -> bool {
        false
    }
    fn paragraph(&mut self, _text: &str, _tight: bool) {}
    fn heading(&mut self, _level: u8, _text: &str) {}
    fn quote_start(&mut self) {}
    fn quote_end(&mut self) {}
    fn list_start(&mut self, _ordered: bool, _start: u64) {}
    fn list_end(&mut self) {}
    fn item_start(&mut self) {}
    fn item_end(&mut self) {}
    fn code_start(&mut self) {}
    fn code_text(&mut self, _s: &str) {}
    fn code_end(&mut self) {}
    fn html_line(&mut self, _s: &str) {}
    fn rule(&mut self) {}
    fn table_row_start(&mut self) {}
    fn table_cell(&mut self, _s: &str) {}
    fn table_row_end(&mut self) {}
    fn footnote_start(&mut self, _label: &str) {}
    fn footnote_end(&mut self) {}
}

enum Cont {
    Quote,
    List { ch: u8, off: u32, loose: bool },
    Item { indent: usize },
    Footnote,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Leaf {
    None,
    Para,
    Fenced { ch: u8, n: usize, indent: usize },
    Indented,
    Html(HtmlEnd),
    Table { cols: usize },
}

const MAX_AUTOCOMPLETED_CELLS: usize = 1 << 18;

struct Blocks<'s, 'k, S: BlockSink> {
    b: &'s [u8],
    sink: &'k mut S,
    conts: Vec<Cont>,
    leaf: Leaf,
    /// Current paragraph, lines joined by `\n` (only when `para_keep`).
    para: String,
    para_keep: bool,
    /// Blank lines held back inside an indented code block (trailing ones are dropped).
    held: String,
    scratch: String,
    cells: Vec<(usize, usize)>,
    last_blank: bool,
    /// The current list item started with a blank line and has no content yet.
    begin_item: bool,
    missing_cells: usize,
    /// Lines starting before this offset are skipped (a table's delimiter row).
    skip_to: usize,
}

impl<'s, 'k, S: BlockSink> Blocks<'s, 'k, S> {
    fn new(src: &'s str, sink: &'k mut S) -> Self {
        Blocks {
            b: src.as_bytes(),
            sink,
            conts: Vec::new(),
            leaf: Leaf::None,
            para: String::new(),
            para_keep: true,
            held: String::new(),
            scratch: String::new(),
            cells: Vec::new(),
            last_blank: false,
            begin_item: false,
            missing_cells: 0,
            skip_to: 0,
        }
    }

    fn run(&mut self) {
        let mut pos = 0;
        while pos < self.b.len() {
            let (end, next) = line_end(self.b, pos);
            if pos >= self.skip_to {
                self.line(pos, end, next);
            }
            pos = next;
        }
        self.close_leaf();
        while !self.conts.is_empty() {
            self.pop_cont();
        }
    }

    /// Match the open containers against a line; returns how many matched.
    fn match_conts(&self, ls: &mut Ls<'s>) -> usize {
        let mut i = 0;
        for c in &self.conts {
            let save = *ls;
            let ok = match c {
                Cont::Quote => {
                    let _ = ls.scan_space(3);
                    ls.scan_quote()
                }
                Cont::Item { indent } => ls.scan_space(*indent) || ls.at_eol(),
                Cont::Footnote => ls.scan_space(4) || ls.at_eol(),
                Cont::List { .. } => true,
            };
            if !ok {
                *ls = save;
                break;
            }
            i += 1;
        }
        i
    }

    fn line(&mut self, off: usize, end: usize, next: usize) {
        let line = &self.b[off..end];
        let eol = next > end;
        let mut ls = Ls::new(line);
        let matched = self.match_conts(&mut ls);
        let all = matched == self.conts.len();

        match self.leaf {
            Leaf::Fenced { ch, n, indent } => {
                if all {
                    let _ = ls.scan_space(indent);
                    let mut cl = ls;
                    if !cl.scan_space(4 - indent) && scan_close_fence(cl.rest(), ch, n) {
                        self.leaf = Leaf::None;
                        self.sink.code_end();
                        return;
                    }
                    self.code_line(ls, eol);
                    return;
                }
                self.close_leaf();
            }
            Leaf::Indented => {
                if all && (ls.scan_space(4) || ls.at_eol()) {
                    if ls.blank() {
                        for _ in 0..ls.rem {
                            self.held.push(' ');
                        }
                        self.held.push_str(as_str(ls.rest()));
                        if eol {
                            self.held.push('\n');
                        }
                        return;
                    }
                    if !self.held.is_empty() {
                        let held = core::mem::take(&mut self.held);
                        self.sink.code_text(&held);
                        self.held = held;
                        self.held.clear();
                    }
                    self.code_line(ls, eol);
                    return;
                }
                self.close_leaf();
            }
            Leaf::Html(kind) => {
                if all {
                    if kind == HtmlEnd::Blank {
                        if !ls.blank() {
                            self.html_line(ls.rest(), eol);
                            return;
                        }
                    } else {
                        self.html_line(ls.rest(), eol);
                        if html_end(kind, ls.rest()) {
                            self.leaf = Leaf::None;
                        }
                        return;
                    }
                }
                self.close_leaf();
            }
            Leaf::Table { cols } => {
                if all {
                    let mut t = ls;
                    t.scan_all_space();
                    if !t.blank() && !self.interrupt(t.rest(), true, true) && self.table_row(t.rest(), cols) {
                        return;
                    }
                }
                self.close_leaf();
            }
            Leaf::Para => {
                let mut p = ls;
                let deep = p.scan_space(4);
                let r = p.rest();
                let mut cont = !ls.blank();
                if cont && !deep {
                    if all {
                        if let Some(level) = scan_setext(r) {
                            if self.close_para_setext(level) {
                                return;
                            }
                            cont = false;
                        }
                    }
                    if cont && (self.interrupt(r, all, false) || (r.starts_with(b"|") && self.table_ahead(r, next).is_some())) {
                        cont = false;
                    }
                }
                if cont {
                    self.para_line(r);
                    return;
                }
                self.close_para();
            }
            Leaf::None => {}
        }

        while self.conts.len() > matched {
            self.pop_cont();
        }

        // New containers.
        loop {
            let save = ls;
            let outer = ls.scan_space_upto(4);
            if outer >= 4 {
                ls = save;
                break;
            }
            let r = ls.rest();
            if let Some((label, used)) = scan_footnote_def(r) {
                self.finish_list();
                if matches!(self.conts.last(), Some(Cont::Footnote)) {
                    self.pop_cont();
                }
                self.sink.footnote_label(label);
                self.sink.footnote_start(label);
                self.conts.push(Cont::Footnote);
                let after = skip_sp(r, used);
                ls = Ls::new(&r[after..]);
                continue;
            }
            let marker_off = (off + ls.ix) as u32;
            if let Some((ch, start, indent)) = ls.scan_list_marker(outer) {
                self.continue_list(marker_off, ch, start);
                self.conts.push(Cont::Item { indent });
                self.sink.item_start();
                if ls.blank() {
                    self.begin_item = true;
                    return;
                }
                continue;
            }
            if ls.scan_quote() {
                self.finish_list();
                self.conts.push(Cont::Quote);
                self.sink.quote_start();
                continue;
            }
            ls = save;
            break;
        }

        if ls.blank() {
            match self.conts.last_mut() {
                Some(Cont::Quote) => {}
                Some(Cont::Item { indent }) if self.begin_item => {
                    self.last_blank = true;
                    *indent = 0;
                }
                _ => self.last_blank = true,
            }
            return;
        }

        // Leaf start.
        let mut l2 = ls;
        let indent = l2.scan_space_upto(4);
        if indent == 4 {
            self.finish_list();
            self.leaf = Leaf::Indented;
            self.held.clear();
            self.sink.code_start();
            self.code_line(l2, eol);
            return;
        }
        let r = l2.rest();
        if r.first() == Some(&b'<') {
            if let Some(kind) = html_block_start(r, true) {
                self.finish_list();
                self.html_line(r, eol);
                if kind == HtmlEnd::Blank || !html_end(kind, r) {
                    self.leaf = Leaf::Html(kind);
                }
                return;
            }
        }
        if scan_hrule(r) {
            self.finish_list();
            self.sink.rule();
            return;
        }
        if let Some((level, cs)) = scan_atx(r) {
            self.finish_list();
            self.sink.heading(level, as_str(atx_content(&r[cs..])));
            return;
        }
        if let Some((ch, n)) = scan_fence(r) {
            self.finish_list();
            self.leaf = Leaf::Fenced { ch, n, indent };
            self.sink.code_start();
            return;
        }
        self.finish_list();
        if count_pipes(r).0 > 0 {
            if let Some((cols, after)) = self.table_ahead(r, next) {
                self.leaf = Leaf::Table { cols };
                self.missing_cells = 0;
                self.skip_to = after;
                self.table_row(r, cols);
                return;
            }
        }
        self.leaf = Leaf::Para;
        self.para.clear();
        self.para_keep = self.sink.wants_text() || r.starts_with(b"[");
        self.para_line(r);
    }

    /// Can this line (after its containers) interrupt a paragraph?
    fn interrupt(&self, r: &[u8], all: bool, in_table: bool) -> bool {
        if r.is_empty() || scan_hrule(r) || scan_atx(r).is_some() || scan_fence(r).is_some() || r[0] == b'>' {
            return true;
        }
        if let Some((ch, idx, blank_after)) = list_probe(r) {
            if !all || in_table || ((ch != b'.' && ch != b')') || idx == 1) && !blank_after {
                return true;
            }
        }
        if r[0] == b'<' && html_block_start(r, false).is_some() {
            return true;
        }
        scan_footnote_def(r).is_some()
    }

    /// If the line after `next` is a delimiter row matching this header line's columns:
    /// (columns, offset after the delimiter row).
    fn table_ahead(&self, header: &[u8], next: usize) -> Option<(usize, usize)> {
        if next >= self.b.len() {
            return None;
        }
        let (e2, n2) = line_end(self.b, next);
        let mut l = Ls::new(&self.b[next..e2]);
        if self.match_conts(&mut l) != self.conts.len() {
            return None;
        }
        let cols = scan_table_head(l.rest())?;
        let (pipes, last) = count_pipes(header);
        (header_cols(header, pipes, last) == cols).then_some((cols, n2))
    }

    fn table_row(&mut self, r: &[u8], cols: usize) -> bool {
        let mut cells = core::mem::take(&mut self.cells);
        split_cells(r, &mut cells);
        let missing = cols.saturating_sub(cells.len());
        if cells.is_empty() || self.missing_cells + missing > MAX_AUTOCOMPLETED_CELLS {
            self.cells = cells;
            return false;
        }
        self.missing_cells += missing;
        self.sink.table_row_start();
        for &(s, e) in cells.iter().take(cols) {
            self.sink.table_cell(as_str(&r[s..e]));
        }
        for _ in 0..missing {
            self.sink.table_cell("");
        }
        self.sink.table_row_end();
        self.cells = cells;
        true
    }

    fn code_line(&mut self, ls: Ls<'s>, eol: bool) {
        self.scratch.clear();
        for _ in 0..ls.rem {
            self.scratch.push(' ');
        }
        self.scratch.push_str(as_str(ls.rest()));
        if eol {
            self.scratch.push('\n');
        }
        let s = core::mem::take(&mut self.scratch);
        self.sink.code_text(&s);
        self.scratch = s;
    }

    fn html_line(&mut self, r: &[u8], eol: bool) {
        self.scratch.clear();
        self.scratch.push_str(as_str(r));
        if eol {
            self.scratch.push('\n');
        }
        let s = core::mem::take(&mut self.scratch);
        self.sink.html_line(&s);
        self.scratch = s;
    }

    fn para_line(&mut self, r: &[u8]) {
        let r = &r[skip_sp(r, 0)..];
        if self.para_keep {
            if !self.para.is_empty() {
                self.para.push('\n');
            }
            self.para.push_str(as_str(r));
        }
    }

    /// Strip leading link reference definitions from the paragraph; returns where the
    /// remaining text starts.
    fn strip_defs(&mut self) -> usize {
        if !self.para_keep {
            return 0;
        }
        let mut i = 0;
        while self.para[i..].starts_with('[') {
            match parse_ref_def(&self.para[i..]) {
                Some((label, url, used)) => {
                    self.sink.link_def(label, url);
                    i += used;
                }
                None => break,
            }
        }
        i
    }

    fn tight(&self) -> bool {
        let n = self.conts.len();
        n >= 2 && matches!(self.conts[n - 1], Cont::Item { .. }) && matches!(self.conts[n - 2], Cont::List { loose: false, .. })
    }

    fn close_para(&mut self) {
        if self.leaf != Leaf::Para {
            return;
        }
        self.leaf = Leaf::None;
        let start = self.strip_defs();
        let tight = self.tight();
        let para = core::mem::take(&mut self.para);
        if !para[start..].trim().is_empty() {
            self.sink.paragraph(&para[start..], tight);
        }
        self.para = para;
        self.para.clear();
    }

    /// Close the paragraph as a setext heading; false when nothing but definitions were
    /// in it (the underline is then an ordinary line).
    fn close_para_setext(&mut self, level: u8) -> bool {
        self.leaf = Leaf::None;
        let start = self.strip_defs();
        let para = core::mem::take(&mut self.para);
        let ok = !para[start..].trim().is_empty();
        if ok {
            self.sink.heading(level, &para[start..]);
        }
        self.para = para;
        self.para.clear();
        ok
    }

    fn close_leaf(&mut self) {
        match self.leaf {
            Leaf::None => {}
            Leaf::Para => self.close_para(),
            Leaf::Fenced { .. } | Leaf::Indented => {
                self.held.clear();
                self.sink.code_end();
            }
            Leaf::Html(_) | Leaf::Table { .. } => {}
        }
        self.leaf = Leaf::None;
    }

    fn pop_cont(&mut self) {
        match self.conts.pop() {
            Some(Cont::Quote) => self.sink.quote_end(),
            Some(Cont::List { .. }) => self.sink.list_end(),
            Some(Cont::Item { .. }) => self.sink.item_end(),
            Some(Cont::Footnote) => self.sink.footnote_end(),
            None => {}
        }
    }

    fn mark_loose(&mut self, at: usize) {
        if let Cont::List { loose, off, .. } = &mut self.conts[at] {
            if !*loose {
                *loose = true;
                let off = *off;
                self.sink.list_loose(off);
            }
        }
    }

    /// An item that began with a blank line and saw another one is empty and closes.
    fn finish_empty_item(&mut self) {
        if self.begin_item {
            if self.last_blank && matches!(self.conts.last(), Some(Cont::Item { .. })) {
                self.pop_cont();
            }
            self.begin_item = false;
        }
    }

    /// Called when a non-list block starts: closes a list that ended, and a blank line
    /// before a block directly inside an item makes that item's list loose.
    fn finish_list(&mut self) {
        self.finish_empty_item();
        if matches!(self.conts.last(), Some(Cont::List { .. })) {
            self.pop_cont();
        }
        if self.last_blank {
            let n = self.conts.len();
            if n >= 2 && matches!(self.conts[n - 1], Cont::Item { .. }) {
                self.mark_loose(n - 2);
            }
            self.last_blank = false;
        }
    }

    fn continue_list(&mut self, off: u32, ch: u8, start: u64) {
        self.finish_empty_item();
        let n = self.conts.len();
        if let Some(Cont::List { ch: existing, .. }) = self.conts.last() {
            if *existing == ch {
                if self.last_blank {
                    self.mark_loose(n - 1);
                    self.last_blank = false;
                }
                return;
            }
        }
        self.finish_list();
        let loose = self.sink.is_loose(off);
        self.conts.push(Cont::List { ch, off, loose });
        let ordered = ch == b'.' || ch == b')';
        self.sink.list_start(ordered, if ordered { start } else { 1 });
    }
}

// ---------------------------------------------------------------------------------------
// Inline parser.
// ---------------------------------------------------------------------------------------

enum Node {
    /// Raw source range; escapes and entities are decoded on emission.
    Text(usize, usize),
    /// Code span content range and whether one space is stripped from each end.
    Code(usize, usize, bool),
    Soft,
    Hard,
    Delim(usize),
    /// Unresolved `[` or `![`.
    Open(bool),
    /// Unresolved `]`.
    Close,
    LinkOpen(String),
    LinkClose,
    ImageOpen,
    ImageClose,
    Autolink(usize, usize),
    Html(usize, usize),
    Footnote(usize, usize),
}

struct Delim {
    ch: u8,
    orig: usize,
    n: usize,
    can_open: bool,
    can_close: bool,
    /// Emphasis opened here, in match order (1 = single, 2 = double); innermost first.
    opens: Vec<u8>,
    /// Emphasis closed here, in match order; innermost first.
    closes: Vec<u8>,
    prev: i32,
    next: i32,
}

struct Bracket {
    node: usize,
    pos: usize,
    image: bool,
    active: bool,
    dlen: usize,
}

struct Inline<'s> {
    s: &'s str,
    b: &'s [u8],
    table: bool,
    nodes: Vec<Node>,
    delims: Vec<Delim>,
    dtail: i32,
    brackets: Vec<Bracket>,
}

fn parse_inline(s: &str, table: bool, defs: &Defs, out: &mut Emitter<'_>) {
    let mut p = Inline { s, b: s.as_bytes(), table, nodes: Vec::new(), delims: Vec::new(), dtail: -1, brackets: Vec::new() };
    p.tokenize(defs);
    p.process_emphasis(0);
    p.emit(out);
}

impl<'s> Inline<'s> {
    fn push_text(&mut self, s: usize, e: usize) {
        if e <= s {
            return;
        }
        if let Some(Node::Text(_, pe)) = self.nodes.last_mut() {
            if *pe == s {
                *pe = e;
                return;
            }
        }
        self.nodes.push(Node::Text(s, e));
    }

    fn prev_char(&self, ix: usize) -> Option<char> {
        self.s[..ix].chars().next_back()
    }

    fn next_char(&self, ix: usize) -> Option<char> {
        self.s[ix..].chars().next()
    }

    fn pipe_before(&self, ix: usize) -> bool {
        self.table && ix >= 1 && self.b[ix - 1] == b'|' && !(ix >= 2 && self.b[ix - 2] == b'\\')
    }

    fn can_open(&self, ix: usize, n: usize) -> bool {
        let Some(next) = self.next_char(ix + n) else { return false };
        if next.is_whitespace() {
            return false;
        }
        if ix == 0 {
            return true;
        }
        if self.pipe_before(ix) {
            return true;
        }
        if self.table && next == '|' {
            return false;
        }
        let ch = self.b[ix];
        if ch == b'*' && !is_punct(next) {
            return true;
        }
        if ch == b'~' && n > 1 {
            return true;
        }
        let prev = self.prev_char(ix).unwrap_or(' ');
        if ch == b'~' && prev == '~' && !is_punct(next) {
            return true;
        }
        prev.is_whitespace() || is_punct(prev)
    }

    fn can_close(&self, ix: usize, n: usize) -> bool {
        if ix == 0 {
            return false;
        }
        let prev = self.prev_char(ix).unwrap_or(' ');
        if prev.is_whitespace() {
            return false;
        }
        let Some(next) = self.next_char(ix + n) else { return true };
        if self.pipe_before(ix) {
            return false;
        }
        if self.table && next == '|' {
            return true;
        }
        let ch = self.b[ix];
        if (ch == b'*' || (ch == b'~' && n > 1)) && !is_punct(prev) {
            return true;
        }
        if ch == b'~' && prev == '~' {
            return true;
        }
        next.is_whitespace() || is_punct(next)
    }

    fn tokenize(&mut self, defs: &Defs) {
        let b = self.b;
        let n = b.len();
        let mut i = skip_sp(b, 0);
        let mut ts = i;
        while i < n {
            match b[i] {
                b'\\' => {
                    if i + 1 < n && b[i + 1] == b'\n' {
                        self.push_text(ts, i);
                        self.nodes.push(Node::Hard);
                        i = skip_sp(b, i + 2);
                        ts = i;
                    } else if i + 1 < n && b[i + 1].is_ascii_punctuation() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                b'\n' => {
                    self.push_text(ts, i);
                    let mut spaces = 0;
                    if let Some(Node::Text(s, e)) = self.nodes.last_mut() {
                        while *e > *s && is_sp(b[*e - 1]) {
                            *e -= 1;
                            spaces += 1;
                        }
                        if *e == *s {
                            self.nodes.pop();
                        }
                    }
                    self.nodes.push(if spaces >= 2 { Node::Hard } else { Node::Soft });
                    i = skip_sp(b, i + 1);
                    ts = i;
                }
                b'`' => {
                    let k = b[i..].iter().take_while(|&&c| c == b'`').count();
                    let mut j = i + k;
                    let mut found = None;
                    while j < n {
                        if b[j] == b'`' {
                            let m = b[j..].iter().take_while(|&&c| c == b'`').count();
                            if m == k {
                                found = Some(j);
                                break;
                            }
                            j += m;
                        } else {
                            j += 1;
                        }
                    }
                    if let Some(j) = found {
                        self.push_text(ts, i);
                        let (cs, ce) = (i + k, j);
                        let content = &b[cs..ce];
                        let strip = content.len() >= 2
                            && matches!(content[0], b' ' | b'\n')
                            && matches!(content[content.len() - 1], b' ' | b'\n')
                            && !content.iter().all(|&c| c == b' ' || c == b'\n');
                        self.nodes.push(Node::Code(cs, ce, strip));
                        i = j + k;
                        ts = i;
                    } else {
                        i += k;
                    }
                }
                c @ (b'*' | b'_' | b'~') => {
                    let k = b[i..].iter().take_while(|&&x| x == c).count();
                    let can_open = self.can_open(i, k);
                    let can_close = self.can_close(i, k);
                    if (can_open || can_close) && (c != b'~' || k <= 2) {
                        self.push_text(ts, i);
                        let idx = self.delims.len();
                        self.delims.push(Delim {
                            ch: c,
                            orig: k,
                            n: k,
                            can_open,
                            can_close,
                            opens: Vec::new(),
                            closes: Vec::new(),
                            prev: self.dtail,
                            next: -1,
                        });
                        if self.dtail >= 0 {
                            self.delims[self.dtail as usize].next = idx as i32;
                        }
                        self.dtail = idx as i32;
                        self.nodes.push(Node::Delim(idx));
                        ts = i + k;
                    }
                    i += k;
                }
                b'[' => {
                    self.push_text(ts, i);
                    self.open_bracket(i, false);
                    i += 1;
                    ts = i;
                }
                b'!' if b.get(i + 1) == Some(&b'[') => {
                    self.push_text(ts, i);
                    self.open_bracket(i, true);
                    i += 2;
                    ts = i;
                }
                b']' => {
                    self.push_text(ts, i);
                    i = self.close_bracket(i, defs);
                    ts = i;
                }
                b'<' => {
                    if let Some(end) = scan_autolink(b, i) {
                        self.push_text(ts, i);
                        self.nodes.push(Node::Autolink(i + 1, end - 1));
                        i = end;
                        ts = i;
                    } else if let Some(end) = scan_inline_html(b, i) {
                        self.push_text(ts, i);
                        self.nodes.push(Node::Html(i, end));
                        i = end;
                        ts = i;
                    } else {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }
        self.push_text(ts, n);
        if let Some(Node::Text(s, e)) = self.nodes.last_mut() {
            while *e > *s && is_sp(b[*e - 1]) {
                *e -= 1;
            }
            if *e == *s {
                self.nodes.pop();
            }
        }
    }

    fn open_bracket(&mut self, pos: usize, image: bool) {
        self.brackets.push(Bracket { node: self.nodes.len(), pos, image, active: true, dlen: self.delims.len() });
        self.nodes.push(Node::Open(image));
    }

    /// Handle `]` at `i`; returns where scanning resumes.
    fn close_bracket(&mut self, i: usize, defs: &Defs) -> usize {
        let Some(br) = self.brackets.pop() else {
            self.push_text(i, i + 1);
            return i + 1;
        };
        if !br.active {
            self.nodes.push(Node::Close);
            return i + 1;
        }
        let content_start = br.pos + if br.image { 2 } else { 1 };
        let content = &self.s[content_start..i];

        if self.b.get(i + 1) == Some(&b'(') {
            if let Some((url, end)) = self.scan_inline_link(i + 1) {
                self.make_link(&br, url);
                return end;
            }
        }
        if let Some(label) = content.strip_prefix('^') {
            if !label.is_empty() && valid_label(content) && defs.footnotes.contains(&normalize_label(label)) {
                self.discard_after(br.node, br.dlen);
                // Like pulldown-cmark, a footnote reference forgets every open bracket.
                self.brackets.clear();
                if br.image {
                    self.nodes[br.node] = Node::Text(br.pos, br.pos + 1);
                    self.nodes.push(Node::Footnote(content_start + 1, i));
                } else {
                    self.nodes[br.node] = Node::Footnote(content_start + 1, i);
                }
                return i + 1;
            }
        }
        let (label, end) = if self.b.get(i + 1) == Some(&b'[') {
            match scan_label_end(self.b, i + 2) {
                Some(le) if le == i + 2 => (content, le + 1),
                Some(le) => {
                    let l = &self.s[i + 2..le];
                    if !valid_label(l) {
                        (content, i + 1)
                    } else if let Some(url) = defs.refs.get(&normalize_label(l)) {
                        let url = url.clone();
                        self.make_link(&br, url);
                        return le + 1;
                    } else {
                        self.nodes.push(Node::Close);
                        return i + 1;
                    }
                }
                None => (content, i + 1),
            }
        } else {
            (content, i + 1)
        };
        if valid_label(label) {
            if let Some(url) = defs.refs.get(&normalize_label(label)) {
                let url = url.clone();
                self.make_link(&br, url);
                return end;
            }
        }
        self.nodes.push(Node::Close);
        i + 1
    }

    /// Drop every node, delimiter and bracket created after `node` (a footnote label).
    fn discard_after(&mut self, node: usize, dlen: usize) {
        self.nodes.truncate(node + 1);
        self.delims.truncate(dlen);
        while self.brackets.last().is_some_and(|x| x.node > node) {
            self.brackets.pop();
        }
        let mut t = self.dtail;
        while t >= dlen as i32 {
            t = self.delims.get(t as usize).map_or(-1, |d| d.prev);
        }
        self.dtail = t;
        if t >= 0 {
            self.delims[t as usize].next = -1;
        }
    }

    fn make_link(&mut self, br: &Bracket, url: String) {
        self.process_emphasis(br.dlen);
        if br.image {
            self.nodes[br.node] = Node::ImageOpen;
            self.nodes.push(Node::ImageClose);
        } else {
            self.nodes[br.node] = Node::LinkOpen(url);
            self.nodes.push(Node::LinkClose);
            for e in &mut self.brackets {
                if !e.image {
                    e.active = false;
                }
            }
        }
    }

    /// `(` dest [title] `)` at `i`: (url, index after `)`).
    fn scan_inline_link(&self, i: usize) -> Option<(String, usize)> {
        let b = self.b;
        let mut k = skip_ws_one_nl(b, i + 1);
        let mut url = String::new();
        if b.get(k) == Some(&b')') {
            return Some((url, k + 1));
        }
        let before = k;
        if let Some((u, k2)) = scan_link_dest(self.s, k) {
            url = u;
            k = k2;
        }
        let after = skip_ws_one_nl(b, k);
        if after > k || k == before {
            if let Some(k2) = scan_link_title(b, after) {
                k = skip_ws_one_nl(b, k2);
            } else {
                k = after;
            }
        }
        (b.get(k) == Some(&b')')).then_some((url, k + 1))
    }

    fn unlink(&mut self, i: usize) {
        let (p, nx) = (self.delims[i].prev, self.delims[i].next);
        if p >= 0 {
            self.delims[p as usize].next = nx;
        }
        if nx >= 0 {
            self.delims[nx as usize].prev = p;
        } else {
            self.dtail = p;
        }
    }

    /// The CommonMark "process emphasis" algorithm over delimiters at index ≥ `bottom`.
    fn process_emphasis(&mut self, bottom: usize) {
        // First active delimiter at or above `bottom`.
        let mut cur = -1i32;
        let mut t = self.dtail;
        while t >= bottom as i32 {
            cur = t;
            t = self.delims[t as usize].prev;
        }
        // Openers-bottom per (char, closer can open, run length mod 3); tildes share one
        // slot whatever their run length, as pulldown-cmark does.
        let key = |ch: u8, can_open: bool, orig: usize| -> usize {
            match ch {
                b'*' => usize::from(can_open) * 3 + orig % 3,
                b'_' => 6 + usize::from(can_open) * 3 + orig % 3,
                _ => 12,
            }
        };
        let mut lower = [bottom as i32; 13];
        while cur >= 0 {
            let ci = cur as usize;
            let (ch, can_open, can_close, orig) = {
                let d = &self.delims[ci];
                (d.ch, d.can_open, d.can_close, d.orig)
            };
            if !can_close {
                cur = self.delims[ci].next;
                continue;
            }
            let k = key(ch, can_open, orig);
            let mut j = self.delims[ci].prev;
            let mut found = -1;
            while j >= lower[k] && j >= bottom as i32 {
                let o = &self.delims[j as usize];
                if o.ch == ch && o.can_open {
                    let odd = (can_open || o.can_close)
                        && (o.orig + orig).is_multiple_of(3)
                        && !(o.orig.is_multiple_of(3) && orig.is_multiple_of(3));
                    let tilde_ok = ch != b'~' || o.orig == orig;
                    if !odd && tilde_ok {
                        found = j;
                        break;
                    }
                }
                j = o.prev;
            }
            if found >= 0 {
                let oi = found as usize;
                let use_n = if ch == b'~' {
                    orig
                } else if self.delims[oi].n >= 2 && self.delims[ci].n >= 2 {
                    2
                } else {
                    1
                };
                let use_n = use_n.min(self.delims[oi].n).min(self.delims[ci].n).max(1);
                self.delims[oi].opens.push(use_n as u8);
                self.delims[ci].closes.push(use_n as u8);
                self.delims[oi].n -= use_n;
                self.delims[ci].n -= use_n;
                self.delims[oi].next = cur;
                self.delims[ci].prev = found;
                for l in &mut lower {
                    if *l > found {
                        *l = found;
                    }
                }
                if self.delims[oi].n == 0 {
                    self.unlink(oi);
                }
                if self.delims[ci].n == 0 {
                    let nx = self.delims[ci].next;
                    self.unlink(ci);
                    cur = nx;
                }
            } else {
                lower[k] = cur;
                let nx = self.delims[ci].next;
                if !can_open {
                    self.unlink(ci);
                }
                cur = nx;
            }
        }
        // Everything above `bottom` leaves the stack.
        let mut t = self.dtail;
        while t >= bottom as i32 {
            let p = self.delims[t as usize].prev;
            self.unlink(t as usize);
            t = p;
        }
    }

    fn emit(&self, out: &mut Emitter<'_>) {
        let mut buf = String::new();
        for node in &self.nodes {
            match node {
                Node::Text(s, e) => {
                    buf.clear();
                    decode_into(&mut buf, &self.s[*s..*e]);
                    out.text(&buf);
                }
                Node::Code(s, e, strip) => {
                    buf.clear();
                    let raw = &self.s[*s..*e];
                    let raw = if *strip { &raw[1..raw.len() - 1] } else { raw };
                    let mut it = raw.chars().peekable();
                    while let Some(c) = it.next() {
                        if c == '\n' {
                            buf.push(' ');
                        } else if c == '\\' && self.table && it.peek() == Some(&'|') {
                            buf.push('|');
                            it.next();
                        } else {
                            buf.push(c);
                        }
                    }
                    out.code(&buf);
                }
                Node::Soft => out.soft(),
                Node::Hard => out.hard(),
                Node::Delim(d) => {
                    let d = &self.delims[*d];
                    let flag = |k: u8| {
                        if d.ch == b'~' {
                            style::STRIKE
                        } else if k == 2 {
                            style::BOLD
                        } else {
                            style::ITALIC
                        }
                    };
                    for &k in &d.closes {
                        out.style_off(flag(k));
                    }
                    if d.n > 0 {
                        buf.clear();
                        for _ in 0..d.n {
                            buf.push(d.ch as char);
                        }
                        out.text(&buf);
                    }
                    for &k in d.opens.iter().rev() {
                        out.style_on(flag(k));
                    }
                }
                Node::Open(image) => out.text(if *image { "![" } else { "[" }),
                Node::Close => out.text("]"),
                Node::LinkOpen(url) => out.link_start(url),
                Node::LinkClose => out.link_end(),
                Node::ImageOpen => out.image_start(),
                Node::ImageClose => {}
                Node::Autolink(s, e) => {
                    let t = &self.s[*s..*e];
                    out.link_start(t);
                    out.text(t);
                    out.link_end();
                }
                Node::Html(s, e) => out.inline_html(&self.s[*s..*e]),
                Node::Footnote(s, e) => out.footnote_ref(&self.s[*s..*e]),
            }
        }
    }
}

/// `<scheme:...>` or `<email>` at `i`: index after `>`.
fn scan_autolink(b: &[u8], i: usize) -> Option<usize> {
    let s = i + 1;
    // URI autolink.
    if b.get(s).is_some_and(|c| c.is_ascii_alphabetic()) {
        let mut j = s + 1;
        while j < b.len() && j - s < 32 && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'+' | b'.' | b'-')) {
            j += 1;
        }
        if j - s >= 2 && b.get(j) == Some(&b':') {
            let mut k = j + 1;
            while k < b.len() && b[k] > 0x20 && b[k] != b'<' && b[k] != b'>' && b[k] != 0x7f {
                k += 1;
            }
            if b.get(k) == Some(&b'>') {
                return Some(k + 1);
            }
        }
    }
    // Email autolink.
    let mut j = s;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&b[j])) {
        j += 1;
    }
    if j == s || b.get(j) != Some(&b'@') {
        return None;
    }
    j += 1;
    loop {
        let ls = j;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-') && j - ls < 63 {
            j += 1;
        }
        if j == ls || b[ls] == b'-' || b[j - 1] == b'-' {
            return None;
        }
        match b.get(j) {
            Some(b'.') => j += 1,
            Some(b'>') => return Some(j + 1),
            _ => return None,
        }
    }
}

fn find_sub(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if from > b.len() {
        return None;
    }
    b[from..].windows(pat.len()).position(|w| w == pat).map(|p| from + p + pat.len())
}

/// Raw inline HTML at `i` (which is `<`): index after it.
fn scan_inline_html(b: &[u8], i: usize) -> Option<usize> {
    let r = &b[i..];
    if r.starts_with(b"<!--") {
        return find_sub(b, i + 4, b"-->");
    }
    if r.starts_with(b"<?") {
        return find_sub(b, i + 2, b"?>");
    }
    if r.starts_with(b"<![CDATA[") {
        return find_sub(b, i + 9, b"]]>");
    }
    if r.len() > 2 && r[1] == b'!' && r[2].is_ascii_alphabetic() {
        return find_sub(b, i + 2, b">");
    }
    if r.starts_with(b"</") {
        if !r.get(2).is_some_and(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let ne = tag_name_end(b, i + 2);
        let mut j = ne;
        while j < b.len() && (is_sp(b[j]) || b[j] == b'\n') {
            j += 1;
        }
        return (b.get(j) == Some(&b'>')).then_some(j + 1);
    }
    if !r.get(1).is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    scan_open_tag_rest(b, tag_name_end(b, i + 1))
}

// ---------------------------------------------------------------------------------------
// Emitter: block and inline events onto QTX.
// ---------------------------------------------------------------------------------------

struct Emitter<'d> {
    defs: &'d Defs,
    w: Writer,
    pending: String,
    styleflags: u8,
    in_para: bool,
    list_stack: Vec<(bool, u16)>,
    first_heading: Option<String>,
    heading_buf: Option<(u8, String)>,
    toc: Vec<(String, u8)>,
    image_ids: u16,
    quote_depth: u32,
    /// A leading `#`/`##` heading is the chapter opening: captured, not written as a
    /// heading paragraph, and emitted as `ChapterTitle` when it ends.
    opening: Option<(u8, String)>,
    any_block: bool,
}

impl<'d> Emitter<'d> {
    fn new(defs: &'d Defs) -> Self {
        Emitter {
            defs,
            w: Writer::new(),
            pending: String::new(),
            styleflags: 0,
            in_para: false,
            list_stack: Vec::new(),
            first_heading: None,
            heading_buf: None,
            toc: Vec::new(),
            image_ids: 0,
            quote_depth: 0,
            opening: None,
            any_block: false,
        }
    }

    fn finish(mut self) -> MdOutput {
        if self.in_para {
            self.push(&Token::End);
        }
        self.flush();
        let chars = self.w.char_count();
        (self.w.finish(), chars, self.first_heading, self.toc)
    }

    fn flush(&mut self) {
        if !self.pending.is_empty() {
            self.w.text(&self.pending);
            self.pending.clear();
        }
    }

    fn push(&mut self, t: &Token) {
        self.flush();
        self.w.push(t);
    }

    fn style(&mut self, flags: u8) {
        self.flush();
        self.w.style(flags);
    }

    fn start(&mut self, k: ParaKind) {
        if self.in_para {
            self.push(&Token::End);
        }
        self.flush();
        self.w.para(k);
        if self.styleflags != 0 {
            self.w.style(self.styleflags);
        }
        self.in_para = true;
    }

    fn end_para(&mut self) {
        if self.in_para {
            self.push(&Token::End);
            self.in_para = false;
        }
    }

    fn inline(&mut self, text: &str, table: bool) {
        let defs = self.defs;
        parse_inline(text, table, defs, self);
    }

    fn strip_html(&mut self, raw: &str) {
        let (bytes, _, _) = crate::html::to_qtx(raw);
        for t in quire_qtx::Reader::new(&bytes) {
            if let Token::Text(s) = t {
                if !self.in_para {
                    self.start(ParaKind::Body);
                }
                self.pending.push_str(&s);
            }
        }
    }

    // --- inline events -------------------------------------------------------------

    fn text(&mut self, s: &str) {
        if let Some((_, buf)) = self.opening.as_mut() {
            buf.push_str(s);
            return;
        }
        if !self.in_para {
            self.any_block = true;
            self.start(ParaKind::Body);
        }
        self.pending.push_str(s);
        if let Some((_, h)) = self.heading_buf.as_mut() {
            h.push_str(s);
        }
    }

    fn code(&mut self, s: &str) {
        if let Some((_, buf)) = self.opening.as_mut() {
            buf.push_str(s);
            return;
        }
        if !self.in_para {
            self.start(ParaKind::Body);
        }
        self.style(self.styleflags | style::MONO);
        self.pending.push_str(s);
        self.style(self.styleflags);
    }

    fn soft(&mut self) {
        if let Some((_, buf)) = self.opening.as_mut() {
            buf.push(' ');
        } else if self.in_para {
            self.pending.push(' ');
        }
    }

    fn hard(&mut self) {
        if let Some((_, buf)) = self.opening.as_mut() {
            buf.push(' ');
        } else if self.in_para {
            self.push(&Token::Break);
        }
    }

    fn style_on(&mut self, f: u8) {
        if self.opening.is_some() {
            return;
        }
        self.styleflags |= f;
        if self.in_para {
            self.style(self.styleflags);
        }
    }

    fn style_off(&mut self, f: u8) {
        if self.opening.is_some() {
            return;
        }
        self.styleflags &= !f;
        if self.in_para {
            self.style(self.styleflags);
        }
    }

    fn link_start(&mut self, url: &str) {
        if self.opening.is_none() && self.in_para {
            self.push(&Token::Link(String::from(url)));
        }
    }

    fn link_end(&mut self) {
        if self.opening.is_none() && self.in_para {
            self.push(&Token::LinkEnd);
        }
    }

    fn image_start(&mut self) {
        if self.opening.is_some() {
            return;
        }
        self.any_block = true;
        self.end_para();
        self.push(&Token::Image { id: self.image_ids, w: 0, h: 0 });
        self.image_ids = self.image_ids.wrapping_add(1);
    }

    fn footnote_ref(&mut self, label: &str) {
        if self.opening.is_none() && self.in_para {
            // Targets take the `chapter#anchor` form; Markdown is one chapter.
            self.push(&Token::Footnote(alloc::format!("0#{label}")));
        }
    }

    fn inline_html(&mut self, raw: &str) {
        if self.opening.is_none() {
            self.strip_html(raw);
        }
    }
}

impl BlockSink for Emitter<'_> {
    fn is_loose(&self, off: u32) -> bool {
        self.defs.loose.binary_search(&off).is_ok()
    }

    fn paragraph(&mut self, text: &str, tight: bool) {
        if !tight {
            self.any_block = true;
            self.start(if self.quote_depth > 0 { ParaKind::Quote } else { ParaKind::Body });
        }
        self.inline(text, false);
        if !tight {
            self.end_para();
        }
    }

    fn heading(&mut self, level: u8, text: &str) {
        let l = level.min(3);
        if !self.any_block && l <= 2 {
            self.opening = Some((l, String::new()));
            self.inline(text, false);
            let (level, buf) = self.opening.take().unwrap_or((l, String::new()));
            let text = crate::html::collapse_ws(&buf);
            self.any_block = true;
            if self.first_heading.is_none() {
                self.first_heading = Some(text.clone());
            }
            self.toc.push((text.clone(), level));
            let (number, title) = crate::html::split_chapter_heading(&text);
            self.push(&Token::ChapterTitle { number, title });
            return;
        }
        self.any_block = true;
        self.start(ParaKind::Heading(l));
        self.heading_buf = Some((l, String::new()));
        self.inline(text, false);
        if let Some((l, t)) = self.heading_buf.take() {
            if self.first_heading.is_none() {
                self.first_heading = Some(t.clone());
            }
            self.toc.push((t, l));
        }
        self.end_para();
    }

    fn quote_start(&mut self) {
        self.quote_depth += 1;
    }

    fn quote_end(&mut self) {
        self.quote_depth = self.quote_depth.saturating_sub(1);
    }

    fn list_start(&mut self, ordered: bool, start: u64) {
        self.list_stack.push((ordered, start as u16));
    }

    fn list_end(&mut self) {
        self.list_stack.pop();
    }

    fn item_start(&mut self) {
        self.any_block = true;
        let level = self.list_stack.len().saturating_sub(1).min(255) as u8;
        let (ordered, idx) = self.list_stack.last().copied().unwrap_or((false, 1));
        self.start(ParaKind::ListItem { ordered, level, index: idx });
        if let Some(t) = self.list_stack.last_mut() {
            t.1 = t.1.saturating_add(1);
        }
    }

    fn item_end(&mut self) {
        self.end_para();
    }

    fn code_start(&mut self) {
        self.any_block = true;
        self.start(ParaKind::Code);
    }

    fn code_text(&mut self, s: &str) {
        if !self.in_para {
            self.start(ParaKind::Body);
        }
        for (i, line) in s.split('\n').enumerate() {
            if i > 0 {
                self.push(&Token::Break);
            }
            self.pending.push_str(line);
        }
    }

    fn code_end(&mut self) {
        self.end_para();
    }

    fn html_line(&mut self, s: &str) {
        self.strip_html(s);
    }

    fn rule(&mut self) {
        self.any_block = true;
        self.end_para();
        self.push(&Token::Rule);
    }

    fn table_row_start(&mut self) {
        self.any_block = true;
        self.start(ParaKind::TableRow);
    }

    fn table_cell(&mut self, s: &str) {
        self.inline(s, true);
        if self.in_para {
            self.pending.push_str(" · ");
        }
    }

    fn table_row_end(&mut self) {
        self.end_para();
    }

    fn footnote_start(&mut self, label: &str) {
        self.any_block = true;
        self.start(ParaKind::Body);
        self.push(&Token::Anchor(String::from(label)));
    }

    fn footnote_end(&mut self) {
        self.end_para();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use quire_qtx::{plain_text, Reader};

    /// Tokens with adjacent text runs merged, so assertions do not depend on chunking.
    fn toks(md: &str) -> Vec<Token> {
        let (bytes, _, _, _) = to_qtx(md);
        let mut out: Vec<Token> = Vec::new();
        for t in Reader::new(&bytes) {
            if let (Token::Text(a), Some(Token::Text(b))) = (&t, out.last_mut()) {
                b.push_str(a);
            } else {
                out.push(t);
            }
        }
        out
    }

    fn text(md: &str) -> String {
        plain_text(&to_qtx(md).0)
    }

    fn para(k: ParaKind) -> Token {
        Token::Para(k)
    }
    fn tx(s: &str) -> Token {
        Token::Text(s.into())
    }
    fn st(f: u8) -> Token {
        Token::Style(f)
    }
    fn item(ordered: bool, level: u8, index: u16) -> Token {
        para(ParaKind::ListItem { ordered, level, index })
    }
    const END: Token = Token::End;
    const BODY: ParaKind = ParaKind::Body;

    #[test]
    fn markdown_maps_to_qtx() {
        let md = "# Chapter 1: Title\n\nSome *italic* and **bold** with `code`.\n\n## Sub\n\n- a\n- b\n\n1. one\n\n> quote\n\n```\nlet x = 1;\nlet y = 2;\n```\n\n---\n";
        let (bytes, _, h, toc) = to_qtx(md);
        assert_eq!(h.as_deref(), Some("Chapter 1: Title"));
        assert_eq!(toc.len(), 2);
        let toks: Vec<Token> = Reader::new(&bytes).collect();
        assert_eq!(toks[0], Token::ChapterTitle { number: Some("1".into()), title: Some("Title".into()) });
        assert!(toks.contains(&Token::Para(ParaKind::Heading(2))));
        assert!(!toks.contains(&Token::Para(ParaKind::Heading(1))));
        assert!(toks.contains(&Token::Style(style::ITALIC)));
        assert!(toks.contains(&Token::Style(style::BOLD)));
        assert!(toks.contains(&Token::Style(style::MONO)));
        assert!(toks.contains(&Token::Para(ParaKind::ListItem { ordered: false, level: 0, index: 1 })));
        assert!(toks.contains(&Token::Para(ParaKind::ListItem { ordered: true, level: 0, index: 1 })));
        assert!(toks.contains(&Token::Para(ParaKind::Quote)));
        assert!(toks.contains(&Token::Para(ParaKind::Code)));
        assert!(toks.contains(&Token::Break));
        assert!(toks.contains(&Token::Rule));
    }

    #[test]
    fn paragraphs_soft_and_hard_breaks() {
        assert_eq!(
            toks("Hello world.\nThis is a soft break.\n\nSecond paragraph.\n"),
            [para(BODY), tx("Hello world. This is a soft break."), END, para(BODY), tx("Second paragraph."), END]
        );
        assert_eq!(
            toks("line one  \nline two\\\nline three   \nlast line\\"),
            [
                para(BODY),
                tx("line one"),
                Token::Break,
                tx("line two"),
                Token::Break,
                tx("line three"),
                Token::Break,
                tx("last line\\"),
                END
            ]
        );
        // Leading spaces of continuation lines vanish; four spaces cannot start code here.
        assert_eq!(text("   indented up to three\n    four spaces is code\n"), "indented up to three four spaces is code\n");
        assert_eq!(text("a  \n  b\nc\\\n  d\n"), "a\nb c\nd\n");
        assert_eq!(toks(""), []);
        assert_eq!(toks("   \n\n   \n"), []);
        assert_eq!(text("\n\n\nText after blanks.\n"), "Text after blanks.\n");
    }

    #[test]
    fn atx_headings() {
        let md = "# One\n## Two\n### Three\n#### Four\n##### Five\n###### Six\n####### Seven\n#5 not heading\n# Closing ##\n#\n##   Spaced   ##   \n# Trailing \\#\n";
        let (bytes, _, first, toc) = to_qtx(md);
        assert_eq!(first.as_deref(), Some("One"));
        let want: Vec<(String, u8)> = [
            ("One", 1),
            ("Two", 2),
            ("Three", 3),
            ("Four", 3),
            ("Five", 3),
            ("Six", 3),
            ("Closing", 1),
            ("", 1),
            ("Spaced", 2),
            ("Trailing #", 1),
        ]
        .iter()
        .map(|(t, l)| (t.to_string(), *l))
        .collect();
        assert_eq!(toc, want);
        let t = toks(md);
        assert_eq!(t[0], Token::ChapterTitle { number: None, title: Some("One".into()) });
        assert!(t.contains(&tx("####### Seven #5 not heading")));
        assert_eq!(plain_text(&bytes), "One\nTwo\nThree\nFour\nFive\nSix\n####### Seven #5 not heading\nClosing\nSpaced\nTrailing #\n");
        assert_eq!(toks("#\tab\n# \n###### six\n").iter().filter(|t| matches!(t, Token::Para(ParaKind::Heading(_)))).count(), 2);
    }

    #[test]
    fn opening_heading_becomes_chapter_title() {
        let (_, _, first, toc) = to_qtx("# Chapter 1: Title\n\nBody text.\n");
        assert_eq!(first.as_deref(), Some("Chapter 1: Title"));
        assert_eq!(toc, [("Chapter 1: Title".to_string(), 1)]);
        assert_eq!(
            toks("# Chapter 1: Title\n\nBody text.\n"),
            [Token::ChapterTitle { number: Some("1".into()), title: Some("Title".into()) }, para(BODY), tx("Body text."), END]
        );
        // A leading H2 opens too; later H1/H2 are ordinary headings; code and links flatten.
        let t = toks("## Chapter XII. The Sea\n\nText.\n\n# Another H1 later\n\n## Sub *emph* and `code`\n");
        assert_eq!(t[0], Token::ChapterTitle { number: Some("XII".into()), title: Some("The Sea".into()) });
        assert!(t.contains(&para(ParaKind::Heading(1))));
        assert_eq!(
            &t[t.len() - 10..t.len() - 1],
            [para(ParaKind::Heading(2)), tx("Sub "), st(2), tx("emph"), st(0), tx(" and "), st(4), tx("code"), st(0)]
        );
        assert_eq!(
            to_qtx("# Chapter XII. The Sea\n\n# Another H1 later\n\n## Sub *emph* and `code`\n").3[2],
            ("Sub emph and ".to_string(), 2)
        );
        assert_eq!(
            toks("# The *Quick* **Brown** `fox` [link](u) ![img](i)\n\nBody.\n")[0],
            Token::ChapterTitle { number: None, title: Some("The Quick Brown fox link img".into()) }
        );
        // H3, or anything after another block, is not an opening.
        assert_eq!(
            toks("### Not an opening\n\n# Later H1\n"),
            [para(ParaKind::Heading(3)), tx("Not an opening"), END, para(ParaKind::Heading(1)), tx("Later H1"), END]
        );
        assert_eq!(toks("Intro.\n\n# Heading after para\n")[3], para(ParaKind::Heading(1)));
        assert_eq!(toks("---\n# Not opening\n")[1], para(ParaKind::Heading(1)));
        let t = toks("![x](u)\n# Not opening\n");
        assert!(t.contains(&para(ParaKind::Heading(1))) && !t.iter().any(|t| matches!(t, Token::ChapterTitle { .. })));
        // Quotes, definitions and stripped HTML do not count as blocks; list items do.
        assert_eq!(toks("> # Quoted title\n\nText.\n")[0], Token::ChapterTitle { number: None, title: Some("Quoted title".into()) });
        assert_eq!(toks("[x]: /u\n\n# Opening\n")[0], Token::ChapterTitle { number: None, title: Some("Opening".into()) });
        assert_eq!(toks("<div>\nx\n</div>\n\n## Opening\n")[2], Token::ChapterTitle { number: None, title: Some("Opening".into()) });
        assert_eq!(
            toks("- # In list\n\nText.\n"),
            [item(false, 0, 1), END, para(ParaKind::Heading(1)), tx("In list"), END, para(BODY), tx("Text."), END]
        );
        assert_eq!(toks("#\n\ntext\n")[0], Token::ChapterTitle { number: None, title: None });
    }

    #[test]
    fn setext_headings() {
        let md = "Setext One\n==========\n\nSetext Two\n----------\n\nMulti\nline\nsetext\n===\n\nPara\n--- not setext\n";
        let (bytes, _, first, toc) = to_qtx(md);
        assert_eq!(first.as_deref(), Some("Setext One"));
        assert_eq!(toc, [("Setext One".to_string(), 1), ("Setext Two".to_string(), 2), ("Multilinesetext".to_string(), 1)]);
        assert_eq!(plain_text(&bytes), "Setext One\nSetext Two\nMulti line setext\nPara --- not setext\n");
        // An underline after a list item is a rule; an indented one is text.
        assert_eq!(
            toks("- item\n---\n\npara\n***\n"),
            [item(false, 0, 1), tx("item"), END, Token::Rule, para(BODY), tx("para"), END, Token::Rule]
        );
        assert_eq!(text("Foo\n    ---\n"), "Foo ---\n");
        // A paragraph holding only a definition leaves its underline as text.
        assert_eq!(text("[foo]: /url\n===\n[foo]\n"), "=== foo\n");
    }

    #[test]
    fn emphasis_and_strong() {
        assert_eq!(
            toks("*a* **b** ***c*** *d **e** f*"),
            [
                para(BODY),
                st(2),
                tx("a"),
                st(0),
                tx(" "),
                st(1),
                tx("b"),
                st(0),
                tx(" "),
                st(2),
                st(3),
                tx("c"),
                st(2),
                st(0),
                tx(" "),
                st(2),
                tx("d "),
                st(3),
                tx("e"),
                st(2),
                tx(" f"),
                st(0),
                END
            ]
        );
        assert_eq!(
            toks("_a_ __b__ ___c___"),
            [para(BODY), st(2), tx("a"), st(0), tx(" "), st(1), tx("b"), st(0), tx(" "), st(2), st(3), tx("c"), st(2), st(0), END]
        );
        // `*` works intraword, `_` does not.
        assert_eq!(
            toks("foo*bar*baz foo**bar**baz foo_bar_baz snake_case_name foo__bar__baz"),
            [
                para(BODY),
                tx("foo"),
                st(2),
                tx("bar"),
                st(0),
                tx("baz foo"),
                st(1),
                tx("bar"),
                st(0),
                tx("baz foo_bar_baz snake_case_name foo__bar__baz"),
                END
            ]
        );
        assert_eq!(toks("*a **b *c __d _e ~~f"), [para(BODY), tx("*a **b *c __d _e ~~f"), END]);
        assert_eq!(toks("**g *h* i*"), [para(BODY), tx("*"), st(2), tx("g "), st(2), tx("h"), st(0), tx(" i"), st(0), END]);
        // Rule of three.
        assert_eq!(toks("*foo**bar**baz*"), [para(BODY), st(2), tx("foo"), st(3), tx("bar"), st(2), tx("baz"), st(0), END]);
        assert_eq!(toks("*foo**bar"), [para(BODY), tx("*foo**bar"), END]);
        assert_eq!(
            toks("*foo**bar\nfoo***bar***baz"),
            [para(BODY), st(2), tx("foo"), st(3), tx("bar foo"), st(2), st(0), tx("bar***baz"), END]
        );
        assert_eq!(
            toks("*foo**bar**baz* **foo*bar*baz** *foo**bar*\nfoo***bar***baz *a**b**c*"),
            [
                para(BODY),
                st(2),
                tx("foo"),
                st(3),
                tx("bar"),
                st(2),
                tx("baz"),
                st(0),
                tx(" "),
                st(1),
                tx("foo"),
                st(3),
                tx("bar"),
                st(1),
                tx("baz"),
                st(0),
                tx(" "),
                st(2),
                tx("foo**bar"),
                st(0),
                tx(" foo"),
                st(2),
                st(3),
                tx("bar"),
                st(2),
                st(0),
                tx("baz "),
                st(2),
                tx("a"),
                st(3),
                tx("b"),
                st(2),
                tx("c"),
                st(0),
                END
            ]
        );
        assert_eq!(toks("foo***bar***baz"), [para(BODY), tx("foo"), st(2), st(3), tx("bar"), st(2), st(0), tx("baz"), END]);
        assert_eq!(toks("_foo*bar_baz*"), [para(BODY), tx("_foo"), st(2), tx("bar_baz"), st(0), END]);
        assert_eq!(
            toks("_foo*bar_baz* *foo _bar* baz_"),
            [para(BODY), st(2), tx("foo"), st(2), tx("bar_baz"), st(0), tx(" "), st(2), tx("foo _bar"), st(0), tx(" baz"), st(0), END]
        );
        assert_eq!(toks("*foo _bar* baz_"), [para(BODY), st(2), tx("foo _bar"), st(0), tx(" baz_"), END]);
        assert_eq!(toks("__foo, __bar__, baz__"), [para(BODY), st(1), tx("foo, "), st(1), tx("bar"), st(0), tx(", baz"), st(0), END]);
        assert_eq!(toks("*(**foo**)*"), [para(BODY), st(2), tx("("), st(3), tx("foo"), st(2), tx(")"), st(0), END]);
        // Punctuation and whitespace flanking.
        assert_eq!(toks("** ** a * b a ** b *a *"), [para(BODY), tx("** ** a * b a ** b *a *"), END]);
        assert_eq!(toks("a**\"foo\"**b **(a)**b"), [para(BODY), tx("a**\"foo\"**b **(a)**b"), END]);
        assert_eq!(toks(".*foo*."), [para(BODY), tx("."), st(2), tx("foo"), st(0), tx("."), END]);
        assert_eq!(
            toks("«*a*» —*b*— *c*。"),
            [para(BODY), tx("«"), st(2), tx("a"), st(0), tx("» —"), st(2), tx("b"), st(0), tx("— "), st(2), tx("c"), st(0), tx("。"), END]
        );
        assert_eq!(
            toks("a\u{a0}*b*\u{a0}c *\u{a0}d\u{a0}*"),
            [para(BODY), tx("a\u{a0}"), st(2), tx("b"), st(0), tx("\u{a0}c *\u{a0}d\u{a0}*"), END]
        );
        assert_eq!(
            toks("****a**** _____b_____"),
            [para(BODY), st(1), st(1), tx("a"), st(0), st(0), tx(" "), st(2), st(3), st(3), tx("b"), st(2), st(2), st(0), END]
        );
        assert_eq!(toks("*a `code` b*"), [para(BODY), st(2), tx("a "), st(6), tx("code"), st(2), tx(" b"), st(0), END]);
    }

    #[test]
    fn strikethrough() {
        assert_eq!(
            toks("~~a~~ ~b~ ~~c~ ~~~d~~~ foo~~bar~~baz foo~bar~baz ~~ x ~~ a~~b"),
            [
                para(BODY),
                st(128),
                tx("a"),
                st(0),
                tx(" "),
                st(128),
                tx("b"),
                st(0),
                tx(" ~~c~ ~~~d~~~ foo"),
                st(128),
                tx("bar"),
                st(0),
                tx("baz foo~bar~baz ~~ x ~~ a~~b"),
                END
            ]
        );
        assert_eq!(toks("x ~~~~c~~~~"), [para(BODY), tx("x ~~~~c~~~~"), END]);
    }

    #[test]
    fn code_spans() {
        assert_eq!(
            toks("`a` `` a`b `` ` c ` `  `"),
            [
                para(BODY),
                st(4),
                tx("a"),
                st(0),
                tx(" "),
                st(4),
                tx("a`b"),
                st(0),
                tx(" "),
                st(4),
                tx("c"),
                st(0),
                tx(" "),
                st(4),
                tx("  "),
                st(0),
                END
            ]
        );
        // Line endings become spaces; entities and escapes stay literal.
        assert_eq!(
            toks("`d\ne` `&amp;` `\\*` `*x*`"),
            [
                para(BODY),
                st(4),
                tx("d e"),
                st(0),
                tx(" "),
                st(4),
                tx("&amp;"),
                st(0),
                tx(" "),
                st(4),
                tx("\\*"),
                st(0),
                tx(" "),
                st(4),
                tx("*x*"),
                st(0),
                END
            ]
        );
        assert_eq!(toks("`` `x `` ` `` `"), [para(BODY), st(4), tx("`x"), st(0), tx(" "), st(4), tx("``"), st(0), END]);
        assert_eq!(toks("`open and *emph* here"), [para(BODY), tx("`open and "), st(2), tx("emph"), st(0), tx(" here"), END]);
        assert_eq!(toks("*a `b*` c*"), [para(BODY), st(2), tx("a "), st(6), tx("b*"), st(2), tx(" c"), st(0), END]);
    }

    #[test]
    fn links() {
        let link = |u: &str, t: &str| [Token::Link(u.into()), tx(t), Token::LinkEnd];
        let t = toks("[t](u) [t](u \"title\") [t](<u v>) [t]() [a](b\\)c) [a](b(c)) [a](u 'single') [a](u (paren))");
        assert_eq!(&t[1..4], link("u", "t"));
        assert_eq!(&t[5..8], link("u", "t"));
        assert_eq!(&t[9..12], link("u v", "t"));
        assert_eq!(&t[13..16], link("", "t"));
        assert_eq!(&t[17..20], link("b)c", "a"));
        assert_eq!(&t[21..24], link("b(c)", "a"));
        assert_eq!(&t[25..28], link("u", "a"));
        assert_eq!(&t[29..32], link("u", "a"));
        assert_eq!(
            toks("[*e* **s**](x) *[in emph](y)*"),
            [
                para(BODY),
                Token::Link("x".into()),
                st(2),
                tx("e"),
                st(0),
                tx(" "),
                st(1),
                tx("s"),
                st(0),
                Token::LinkEnd,
                tx(" "),
                st(2),
                Token::Link("y".into()),
                tx("in emph"),
                Token::LinkEnd,
                st(0),
                END
            ]
        );
        assert_eq!(toks("[a](b [c] [d](e \"f) [g](h i)"), [para(BODY), tx("[a](b [c] [d](e \"f) [g](h i)"), END]);
        assert_eq!(
            toks("[a](u \"t) [b](u \"x\" y) [c](u\n\"t\")"),
            [para(BODY), tx("[a](u \"t) [b](u \"x\" y) "), Token::Link("u".into()), tx("c"), Token::LinkEnd, END]
        );
        // Reference links: full, collapsed, shortcut; labels are case-folded and definitions
        // may follow their use; undefined ones stay text.
        let t = toks("[t][r] [R][] [r] [Missing] [t][missing]\n\n[r]: /url \"Title\"\n");
        assert_eq!(
            t,
            [
                para(BODY),
                Token::Link("/url".into()),
                tx("t"),
                Token::LinkEnd,
                tx(" "),
                Token::Link("/url".into()),
                tx("R"),
                Token::LinkEnd,
                tx(" "),
                Token::Link("/url".into()),
                tx("r"),
                Token::LinkEnd,
                tx(" [Missing] [t][missing]"),
                END
            ]
        );
        assert_eq!(text("[x]: /x\n[x] [X] [ x ]\n"), "x X  x \n");
        assert_eq!(toks("[x]: /x\n[x]")[1], Token::Link("/x".into()));
        assert_eq!(
            text("[a]: /u\n  \"title\"\n[a]\n\n[b]: /v \"bad\n[b]\n\n[c]: /w\nextra words\n[c]\n"),
            "a\n[b]: /v \"bad [b]\nextra words c\n"
        );
        assert_eq!(toks("> [q]: /q\n> [q]\n")[1], Token::Link("/q".into()));
        assert_eq!(text("text before\n[x]: /u\n[x]\n"), "text before [x]: /u [x]\n");
        // Links do not nest; images may hold links; escaped brackets are text.
        assert_eq!(toks("[a [b](c) d](e)"), [para(BODY), tx("[a "), Token::Link("c".into()), tx("b"), Token::LinkEnd, tx(" d](e)"), END]);
        assert_eq!(
            toks("\\[not a link\\](u) [a\\]b](c) [a](u\\_v) [a](u&amp;v)"),
            [
                para(BODY),
                tx("[not a link](u) "),
                Token::Link("c".into()),
                tx("a]b"),
                Token::LinkEnd,
                tx(" "),
                Token::Link("u_v".into()),
                tx("a"),
                Token::LinkEnd,
                tx(" "),
                Token::Link("u&v".into()),
                tx("a"),
                Token::LinkEnd,
                END
            ]
        );
        assert_eq!(toks("[a [b] c](u)"), [para(BODY), Token::Link("u".into()), tx("a [b] c"), Token::LinkEnd, END]);
        assert_eq!(toks("[`]`](u)"), [para(BODY), Token::Link("u".into()), st(4), tx("]"), st(0), Token::LinkEnd, END]);
        // Links inside headings appear in the TOC as text.
        assert_eq!(to_qtx("x\n\n## See [the docs](http://x) here\n").3, [("See the docs here".to_string(), 2)]);
    }

    #[test]
    fn autolinks_and_inline_html() {
        let t = toks("<http://a.b/c?d=1> <mailto:x@y.z> <x@y.z> <not an autolink> <a b> <https://x>. a<b>c");
        assert_eq!(&t[1..4], [Token::Link("http://a.b/c?d=1".into()), tx("http://a.b/c?d=1"), Token::LinkEnd]);
        assert_eq!(&t[5..8], [Token::Link("mailto:x@y.z".into()), tx("mailto:x@y.z"), Token::LinkEnd]);
        assert_eq!(&t[9..12], [Token::Link("x@y.z".into()), tx("x@y.z"), Token::LinkEnd]);
        // Tags are stripped (they are HTML), leaving their surrounding spaces.
        assert_eq!(t[12], tx("   "));
        assert_eq!(&t[t.len() - 2..], [tx(". ac"), END]);
        assert_eq!(text("<span>a</span> b <br> c <!-- comment --> d <x-tag attr=\"1\"> e < 3 f <3\n"), "a b  c  d  e < 3 f <3\n");
        assert_eq!(text("<b>bold</b> and <i>it</i> and <a href=\"x\">link</a>\n"), "bold and it and link\n");
        assert_eq!(toks("<a href=\"x\">*y*</a>"), [para(BODY), st(2), tx("y"), st(0), END]);
        assert_eq!(text("<http://x>y <a@b.c>d <http://a b> <http://> <a:b>\n"), "http://xy a@b.cd <http://a b> http:// <a:b>\n");
    }

    #[test]
    fn html_blocks_keep_their_text() {
        assert_eq!(toks("<div>\nhello\n</div>\n\nafter\n"), [para(BODY), tx("hello"), END, para(BODY), tx("after"), END]);
        assert_eq!(text("<p>para text</p>\nnext line still html\n\nafter blank\n"), "para textnext line still html\nafter blank\n");
        assert_eq!(text("<!-- a comment\nspanning lines -->\npara right after\n"), "spanning lines -->\npara right after\n");
        // Block tags interrupt a paragraph; inline-only tags do not.
        assert_eq!(toks("text before\n<div>\ninside\n</div>\n"), [para(BODY), tx("text before"), END, para(BODY), tx("inside"), END]);
        assert_eq!(text("text\n<span>x</span> inline\n"), "text x inline\n");
        assert_eq!(text("<a href=\"x\">\ntext after\n\n<a href=\"x\">text\npara\n"), "text after\ntext para\n");
        assert_eq!(text("<script>\nx < 1\n</script>\n<pre>\n  pre\n</pre>\n<?php ?>\n<!DOCTYPE html>\n<![CDATA[ x ]]>\n<table><tr><td>\nhi\n</td></tr></table>\n\n<custom>\nend\n"), "x < 1prexhiend\n");
    }

    #[test]
    fn entities_and_escapes() {
        assert_eq!(
            text("&amp; &lt; &gt; &#65; &#x41; &copy; &nonsense; &#0; &; &mdash; a&b &AMP; &amp &#X41;\n"),
            "& < > A A © &nonsense; \u{FFFD} &; — a&b & &amp A\n"
        );
        assert_eq!(toks("[a](u&amp;v&#48;)")[1], Token::Link("u&v0".into()));
        assert_eq!(to_qtx("x\n\n## A &amp; B `c`\n").3, [("A & B ".to_string(), 2)]);
        assert_eq!(
            text("\\* \\_ \\# \\\\ \\[ \\] \\` \\< \\> \\a \\1 \\-\n\\# not heading\n\\- not list\n\\> not quote\n"),
            "* _ # \\ [ ] ` < > \\a \\1 - # not heading - not list > not quote\n"
        );
        assert_eq!(text("ends with backslash\\"), "ends with backslash\\\n");
        assert_eq!(toks("\\\\*not em* \\** \\é"), [para(BODY), tx("\\"), st(2), tx("not em"), st(0), tx(" ** \\é"), END]);
        assert_eq!(text("Ünïcödé — “quotes” • 日本語 *強調*\n"), "Ünïcödé — “quotes” • 日本語 強調\n");
    }

    #[test]
    fn blockquotes() {
        assert_eq!(
            toks("> quoted\n> more\nlazy continuation\n\n> a\n>\n> b\n"),
            [
                para(ParaKind::Quote),
                tx("quoted more lazy continuation"),
                END,
                para(ParaKind::Quote),
                tx("a"),
                END,
                para(ParaKind::Quote),
                tx("b"),
                END
            ]
        );
        let t = toks("> > deep\n> back\n>> also deep\n\n> - list in quote\n> - item two\n>\n> ```\n> code in quote\n> ```\n>\n> # Heading in quote\n");
        assert_eq!(
            t,
            [
                para(ParaKind::Quote),
                tx("deep back also deep"),
                END,
                item(false, 0, 1),
                tx("list in quote"),
                END,
                item(false, 0, 2),
                tx("item two"),
                END,
                para(ParaKind::Code),
                tx("code in quote"),
                Token::Break,
                END,
                para(ParaKind::Heading(1)),
                tx("Heading in quote"),
                END
            ]
        );
        assert_eq!(text(">no space\n>     indented code in quote\n>\n>\n> end\n"), "no space indented code in quote\nend\n");
        assert_eq!(toks(">\n>\n"), []);
        assert_eq!(text("> > > > > deep\n> > > back\n"), "deep back\n");
        assert_eq!(
            toks("> - a\n>   > inner\n>   > quote\n> - b\n"),
            [item(false, 0, 1), tx("a"), END, para(ParaKind::Quote), tx("inner quote"), END, item(false, 0, 2), tx("b"), END]
        );
        // A fence closed outside the quote closes with the quote, and reopens outside.
        assert_eq!(
            toks("> ```\n> code\n```\nafter\n"),
            [para(ParaKind::Code), tx("code"), Token::Break, END, para(ParaKind::Code), tx("after"), Token::Break, END]
        );
    }

    #[test]
    fn lists_markers_numbering_and_nesting() {
        assert_eq!(
            toks("- a\n- b\n* c\n* d\n+ e\n"),
            [
                item(false, 0, 1),
                tx("a"),
                END,
                item(false, 0, 2),
                tx("b"),
                END,
                item(false, 0, 1),
                tx("c"),
                END,
                item(false, 0, 2),
                tx("d"),
                END,
                item(false, 0, 1),
                tx("e"),
                END
            ]
        );
        assert_eq!(
            toks("1. one\n2. two\n3. three\n\n3) three\n4) four\n\n007. seven\n"),
            [
                item(true, 0, 1),
                tx("one"),
                END,
                item(true, 0, 2),
                tx("two"),
                END,
                item(true, 0, 3),
                tx("three"),
                END,
                item(true, 0, 3),
                tx("three"),
                END,
                item(true, 0, 4),
                tx("four"),
                END,
                item(true, 0, 7),
                tx("seven"),
                END
            ]
        );
        // Numbering follows the start number, then counts up regardless of what is written.
        assert_eq!(
            toks("1. a\n1. b\n1. c\n\n0) zero\n1) one\n"),
            [
                item(true, 0, 1),
                tx("a"),
                END,
                item(true, 0, 2),
                tx("b"),
                END,
                item(true, 0, 3),
                tx("c"),
                END,
                item(true, 0, 0),
                tx("zero"),
                END,
                item(true, 0, 1),
                tx("one"),
                END
            ]
        );
        // A blank line before a marker of the same kind continues (and loosens) the list.
        assert_eq!(
            toks("1. a\n\n0. b\n"),
            [item(true, 0, 1), END, para(BODY), tx("a"), END, item(true, 0, 2), END, para(BODY), tx("b"), END]
        );
        assert_eq!(toks("5. five\n6. six\n")[0], item(true, 0, 5));
        assert_eq!(
            toks("- a\n  - b\n    - c\n  - d\n- e\n"),
            [
                item(false, 0, 1),
                tx("a"),
                END,
                item(false, 1, 1),
                tx("b"),
                END,
                item(false, 2, 1),
                tx("c"),
                END,
                item(false, 1, 2),
                tx("d"),
                END,
                item(false, 0, 2),
                tx("e"),
                END
            ]
        );
        assert_eq!(
            toks("1. one\n   - bullet\n   - bullet2\n2. two\n   1. sub one\n   2. sub two\n"),
            [
                item(true, 0, 1),
                tx("one"),
                END,
                item(false, 1, 1),
                tx("bullet"),
                END,
                item(false, 1, 2),
                tx("bullet2"),
                END,
                item(true, 0, 2),
                tx("two"),
                END,
                item(true, 1, 1),
                tx("sub one"),
                END,
                item(true, 1, 2),
                tx("sub two"),
                END
            ]
        );
        // Item indentation decides nesting: a marker under the content column nests.
        assert_eq!(
            toks("- a\n - b\n  - c\n   - d\n    - e\n"),
            [
                item(false, 0, 1),
                tx("a"),
                END,
                item(false, 0, 2),
                tx("b"),
                END,
                item(false, 0, 3),
                tx("c"),
                END,
                item(false, 0, 4),
                tx("d - e"),
                END
            ]
        );
        assert_eq!(
            toks("- a\n    - b\n        - c\n- d\n"),
            [
                item(false, 0, 1),
                tx("a"),
                END,
                item(false, 1, 1),
                tx("b"),
                END,
                item(false, 2, 1),
                tx("c"),
                END,
                item(false, 0, 2),
                tx("d"),
                END
            ]
        );
        assert_eq!(
            toks("-\n- a\n-\n- b\n"),
            [item(false, 0, 1), END, item(false, 0, 2), tx("a"), END, item(false, 0, 3), END, item(false, 0, 4), tx("b"), END]
        );
        assert_eq!(toks("- 1\n  - 2\n    - 3\n      - 4\n        - 5\n          - 6\n")[15], item(false, 5, 1));
        assert_eq!(toks("123456789. big\n1234567890. too big\n"), [item(true, 0, 123456789u64 as u16), tx("big 1234567890. too big"), END]);
    }

    #[test]
    fn lists_loose_tight_and_continuation() {
        // Loose lists wrap item text in Body paragraphs; tight ones do not.
        assert_eq!(
            toks("- a\n\n- b\n- c\n"),
            [
                item(false, 0, 1),
                END,
                para(BODY),
                tx("a"),
                END,
                item(false, 0, 2),
                END,
                para(BODY),
                tx("b"),
                END,
                item(false, 0, 3),
                END,
                para(BODY),
                tx("c"),
                END
            ]
        );
        assert_eq!(
            toks("- a\n\n  second para of a\n- b\n"),
            [
                item(false, 0, 1),
                END,
                para(BODY),
                tx("a"),
                END,
                para(BODY),
                tx("second para of a"),
                END,
                item(false, 0, 2),
                END,
                para(BODY),
                tx("b"),
                END
            ]
        );
        assert_eq!(
            toks("- a\n  > quote\n  ```\n  code\n  ```\n- b\n"),
            [
                item(false, 0, 1),
                tx("a"),
                END,
                para(ParaKind::Quote),
                tx("quote"),
                END,
                para(ParaKind::Code),
                tx("code"),
                Token::Break,
                END,
                item(false, 0, 2),
                tx("b"),
                END
            ]
        );
        assert_eq!(
            toks("- a\n  b\n\n  c\n- d\n"),
            [
                item(false, 0, 1),
                END,
                para(BODY),
                tx("a b"),
                END,
                para(BODY),
                tx("c"),
                END,
                item(false, 0, 2),
                END,
                para(BODY),
                tx("d"),
                END
            ]
        );
        assert_eq!(toks("- a\n\n\n- b\n").len(), 10);
        // Interruption: bullets and `1.` interrupt a paragraph, other numbers do not.
        assert_eq!(
            toks("para\n- interrupts\npara\n1. interrupts\npara\n2. does not interrupt\n"),
            [
                para(BODY),
                tx("para"),
                END,
                item(false, 0, 1),
                tx("interrupts para"),
                END,
                item(true, 0, 1),
                tx("interrupts para"),
                END,
                item(true, 0, 2),
                tx("does not interrupt"),
                END
            ]
        );
        assert_eq!(toks("- a\nlazy\n- b\n"), [item(false, 0, 1), tx("a lazy"), END, item(false, 0, 2), tx("b"), END]);
        assert_eq!(toks("- a\n- b\npara right after\n")[4], tx("b para right after"));
        assert_eq!(toks("- a\n\npara\n"), [item(false, 0, 1), tx("a"), END, para(BODY), tx("para"), END]);
        // Indented code inside items; five spaces after a marker is code.
        assert_eq!(
            toks("- a\n\n      code in item\n\n- b\n"),
            [
                item(false, 0, 1),
                END,
                para(BODY),
                tx("a"),
                END,
                para(ParaKind::Code),
                tx("code in item"),
                Token::Break,
                END,
                item(false, 0, 2),
                END,
                para(BODY),
                tx("b"),
                END
            ]
        );
        assert_eq!(
            toks("-     five spaces\n- normal\n"),
            [item(false, 0, 1), END, para(ParaKind::Code), tx("five spaces"), Token::Break, END, item(false, 0, 2), tx("normal"), END]
        );
        // An item may start with one blank line; two make it empty.
        assert_eq!(
            toks("-\n  a\n\n- b\n"),
            [item(false, 0, 1), END, para(BODY), tx("a"), END, item(false, 0, 2), END, para(BODY), tx("b"), END]
        );
        assert_eq!(toks("1.\nfoo\n\n-\n  bar\n"), [item(true, 0, 1), END, para(BODY), tx("foo"), END, item(false, 0, 1), tx("bar"), END]);
        assert_eq!(
            toks("- # h\n- ## h2\n- para\n"),
            [
                item(false, 0, 1),
                END,
                para(ParaKind::Heading(1)),
                tx("h"),
                END,
                item(false, 0, 2),
                END,
                para(ParaKind::Heading(2)),
                tx("h2"),
                END,
                item(false, 0, 3),
                tx("para"),
                END
            ]
        );
        assert_eq!(toks("- a\n- ---\n"), [item(false, 0, 1), tx("a"), END, Token::Rule]);
        // Tabs count to the next multiple of four columns.
        assert_eq!(
            toks("- a\n\t- tab nested\n\t  more\n\n\tcode with tab\n"),
            [
                item(false, 0, 1),
                END,
                para(BODY),
                tx("a"),
                END,
                item(false, 1, 1),
                tx("tab nested more"),
                END,
                para(BODY),
                tx("code with tab"),
                END
            ]
        );
    }

    #[test]
    fn code_blocks() {
        assert_eq!(
            toks("```rust\nlet x = 1;\n\nlet y = 2;\n```\n"),
            [para(ParaKind::Code), tx("let x = 1;"), Token::Break, Token::Break, tx("let y = 2;"), Token::Break, END]
        );
        assert_eq!(toks("~~~\na\n~~~\nafter\n"), [para(ParaKind::Code), tx("a"), Token::Break, END, para(BODY), tx("after"), END]);
        assert_eq!(
            toks("```\nnever closed\nstill code\n"),
            [para(ParaKind::Code), tx("never closed"), Token::Break, tx("still code"), Token::Break, END]
        );
        assert_eq!(toks("````\n```\ninside\n````\n"), [para(ParaKind::Code), tx("```"), Token::Break, tx("inside"), Token::Break, END]);
        assert_eq!(
            toks("  ```\n    indented content\n  keep\n  ```\n"),
            [para(ParaKind::Code), tx("  indented content"), Token::Break, tx("keep"), Token::Break, END]
        );
        assert_eq!(
            toks("- a\n  ```\n  code\n  ```\n- b\n"),
            [item(false, 0, 1), tx("a"), END, para(ParaKind::Code), tx("code"), Token::Break, END, item(false, 0, 2), tx("b"), END]
        );
        assert_eq!(toks("``` a`b\nnot a fence\n```\n"), [para(BODY), tx("``` a`b not a fence"), END, para(ParaKind::Code), END]);
        assert_eq!(
            toks("```\n<b> &amp; *x* `y`\n  leading\n```\n"),
            [para(ParaKind::Code), tx("<b> &amp; *x* `y`"), Token::Break, tx("  leading"), Token::Break, END]
        );
        assert_eq!(toks("```\nabc"), [para(ParaKind::Code), tx("abc"), END]);
        // Indented code: interior blank lines kept, trailing ones dropped, tabs expanded.
        assert_eq!(
            toks("    line one\n    line two\n\n    after blank\npara\n"),
            [
                para(ParaKind::Code),
                tx("line one"),
                Token::Break,
                tx("line two"),
                Token::Break,
                Token::Break,
                tx("after blank"),
                Token::Break,
                END,
                para(BODY),
                tx("para"),
                END
            ]
        );
        assert_eq!(toks("    code\n\n    \npara\n"), [para(ParaKind::Code), tx("code"), Token::Break, END, para(BODY), tx("para"), END]);
        assert_eq!(
            toks("\ttabbed\n\t  tabbed more\n"),
            [para(ParaKind::Code), tx("tabbed"), Token::Break, tx("  tabbed more"), Token::Break, END]
        );
        assert_eq!(toks("\t\tfoo\n  \tbar\n"), [para(ParaKind::Code), tx("\tfoo"), Token::Break, tx("bar"), Token::Break, END]);
        assert_eq!(text("para\n    not code\n"), "para not code\n");
    }

    #[test]
    fn thematic_breaks() {
        assert_eq!(
            toks("---\n***\n___\n- - -\n----------\n ***\n--\n*** a\n"),
            [Token::Rule, Token::Rule, Token::Rule, Token::Rule, Token::Rule, Token::Rule, para(BODY), tx("-- *** a"), END]
        );
        assert_eq!(toks("para\n* * *\nafter\n"), [para(BODY), tx("para"), END, Token::Rule, para(BODY), tx("after"), END]);
    }

    #[test]
    fn tables() {
        let row = |s: &str| [para(ParaKind::TableRow), tx(s), END];
        let mut want = Vec::new();
        want.extend(row("a · b · "));
        want.extend(row("1 · 2 · "));
        want.extend(row("3 · 4 · "));
        assert_eq!(toks("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n"), want);
        assert_eq!(text("a | b\n--|--\n1 | 2\n"), "a · b · \n1 · 2 · \n");
        assert_eq!(text("| l | c | r |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n"), "l · c · r · \n1 · 2 · 3 · \n");
        // Short rows are padded, long ones truncated, escaped pipes kept, inline styles work.
        assert_eq!(text("| a | b | c |\n|---|---|---|\n| 1 |\n| 1 | 2 | 3 | 4 |\n"), "a · b · c · \n1 ·  ·  · \n1 · 2 · 3 · \n");
        assert_eq!(
            toks("| *a* | `b` | [c](d) |\n|---|---|---|\n| ~~x~~ | y \\| z | **w** |\n"),
            [
                para(ParaKind::TableRow),
                st(2),
                tx("a"),
                st(0),
                tx(" · "),
                st(4),
                tx("b"),
                st(0),
                tx(" · "),
                Token::Link("d".into()),
                tx("c"),
                Token::LinkEnd,
                tx(" · "),
                END,
                para(ParaKind::TableRow),
                st(128),
                tx("x"),
                st(0),
                tx(" · y | z · "),
                st(1),
                tx("w"),
                st(0),
                tx(" · "),
                END
            ]
        );
        assert_eq!(text("| a | b |\n|---|---|\n|   |   |\n| x ||\n"), "a · b · \n ·  · \nx ·  · \n");
        assert_eq!(text("| a | b |\n|---|---|\n| `x|y` | z |\n| `x\\|y` | w |\n"), "a · b · \n`x · y` · \nx|y · w · \n");
        // A table ends at a blank line or another block; a mismatched delimiter row is text.
        assert_eq!(
            text("| a | b |\n|---|---|\n| 1 | 2 |\n\npara after\n\n| c | d |\n|---|---|\n| 3 | 4 |\n- list ends table\n"),
            "a · b · \n1 · 2 · \npara after\nc · d · \n3 · 4 · \nlist ends table\n"
        );
        assert_eq!(text("| a | b |\n|---|\n| 1 | 2 |\n"), "| a | b | |---| | 1 | 2 |\n");
        assert_eq!(text("| a |\n|---|\n| 1 |\n"), "a · \n1 · \n");
        assert_eq!(text("> | a | b |\n> |---|---|\n> | 1 | 2 |\n"), "a · b · \n1 · 2 · \n");
        assert_eq!(
            text("| a | b |\n|---|---|\n1 | 2\nplain text row\n> quote ends\n"),
            "a · b · \n1 · 2 · \nplain text row ·  · \nquote ends\n"
        );
        // Only a header starting with a pipe may interrupt a paragraph.
        assert_eq!(text("text before\n| a | b |\n|---|---|\n| 1 | 2 |\n"), "text before\na · b · \n1 · 2 · \n");
        assert_eq!(text("text before\na | b\n--|--\n1 | 2\n"), "text before a | b --|-- 1 | 2\n");
        assert_eq!(text("- | a | b |\n  |---|---|\n  | 1 | 2 |\n- next\n"), "a · b · \n1 · 2 · \nnext\n");
        // An image in a cell ends the row paragraph like anywhere else.
        assert_eq!(
            toks("| ![i](u) | b |\n|---|---|\n| 1 | 2 |\n"),
            [
                para(ParaKind::TableRow),
                END,
                Token::Image { id: 0, w: 0, h: 0 },
                para(BODY),
                tx("i · b · "),
                END,
                para(ParaKind::TableRow),
                tx("1 · 2 · "),
                END
            ]
        );
    }

    #[test]
    fn footnotes() {
        let t =
            toks("Text[^1] and[^two] and [^missing].\n\n[^1]: First note.\n[^two]: Second note\n    continued.\n\n    Second paragraph.\n");
        assert_eq!(
            t,
            [
                para(BODY),
                tx("Text"),
                Token::Footnote("0#1".into()),
                tx(" and"),
                Token::Footnote("0#two".into()),
                tx(" and [^missing]."),
                END,
                para(BODY),
                Token::Anchor("1".into()),
                END,
                para(BODY),
                tx("First note."),
                END,
                para(BODY),
                Token::Anchor("two".into()),
                END,
                para(BODY),
                tx("Second note continued."),
                END,
                para(BODY),
                tx("Second paragraph."),
                END
            ]
        );
        assert_eq!(toks("Text[^1] here.\n"), [para(BODY), tx("Text[^1] here."), END]);
        assert_eq!(toks("[^a]: orphan note\n"), [para(BODY), Token::Anchor("a".into()), END, para(BODY), tx("orphan note"), END]);
        assert_eq!(toks("*emph[^n]*\n\n[^n]: note\n")[..6], [para(BODY), st(2), tx("emph"), Token::Footnote("0#n".into()), st(0), END]);
        assert_eq!(
            toks("![^1] and ![^1]\n\n[^1]: x\n")[..6],
            [para(BODY), tx("!"), Token::Footnote("0#1".into()), tx(" and !"), Token::Footnote("0#1".into()), END]
        );
        // Labels match case-insensitively; a reference inside link text wins over the link.
        assert_eq!(toks("ref[^ONE]\n\n[^one]: One\n")[2], Token::Footnote("0#ONE".into()));
        assert_eq!(toks("[a[^n]](u)\n\n[^n]: note\n")[..5], [para(BODY), tx("[a"), Token::Footnote("0#n".into()), tx("](u)"), END]);
    }

    #[test]
    fn images_get_ids_and_split_paragraphs() {
        assert_eq!(
            toks("![alt](u) ![](v) start ![mid](w) end *![in emph](x)* ![*a* **b**](y)"),
            [
                para(BODY),
                END,
                Token::Image { id: 0, w: 0, h: 0 },
                para(BODY),
                tx("alt "),
                END,
                Token::Image { id: 1, w: 0, h: 0 },
                para(BODY),
                tx(" start "),
                END,
                Token::Image { id: 2, w: 0, h: 0 },
                para(BODY),
                tx("mid end "),
                st(2),
                END,
                Token::Image { id: 3, w: 0, h: 0 },
                para(BODY),
                st(2),
                tx("in emph"),
                st(0),
                tx(" "),
                END,
                Token::Image { id: 4, w: 0, h: 0 },
                para(BODY),
                st(2),
                tx("a"),
                st(0),
                tx(" "),
                st(1),
                tx("b"),
                st(0),
                END,
            ]
        );
        assert_eq!(
            toks("![alt][r] ![r]\n\n[r]: /img\n"),
            [
                para(BODY),
                END,
                Token::Image { id: 0, w: 0, h: 0 },
                para(BODY),
                tx("alt "),
                END,
                Token::Image { id: 1, w: 0, h: 0 },
                para(BODY),
                tx("r"),
                END
            ]
        );
        assert_eq!(
            toks("- ![i](u) after\n- plain\n"),
            [
                item(false, 0, 1),
                END,
                Token::Image { id: 0, w: 0, h: 0 },
                para(BODY),
                tx("i after"),
                END,
                item(false, 0, 2),
                tx("plain"),
                END
            ]
        );
        assert_eq!(
            toks("[![i](j)](k)"),
            [para(BODY), Token::Link("k".into()), END, Token::Image { id: 0, w: 0, h: 0 }, para(BODY), tx("i"), Token::LinkEnd, END]
        );
        assert_eq!(to_qtx("x\n\n## Heading ![img](u) text\n").3, [("Heading img text".to_string(), 2)]);
    }

    #[test]
    fn line_endings() {
        let md = "# Title\r\n\r\npara one\r\nline two\r\n\r\n- a\r\n- b\r\n\r\n```\r\ncode\r\n```\r\n> q\r\n";
        assert_eq!(
            toks(md),
            [
                Token::ChapterTitle { number: None, title: Some("Title".into()) },
                para(BODY),
                tx("para one line two"),
                END,
                item(false, 0, 1),
                tx("a"),
                END,
                item(false, 0, 2),
                tx("b"),
                END,
                para(ParaKind::Code),
                tx("code"),
                Token::Break,
                END,
                para(ParaKind::Quote),
                tx("q"),
                END
            ]
        );
        assert_eq!(
            toks("- a\r\n\r\n- b\r\nline  \r\nbreak\r\n"),
            [
                item(false, 0, 1),
                END,
                para(BODY),
                tx("a"),
                END,
                item(false, 0, 2),
                END,
                para(BODY),
                tx("b line"),
                Token::Break,
                tx("break"),
                END
            ]
        );
        assert_eq!(text("a\rb\r\r# H\r"), "a b\nH\n");
    }

    #[test]
    fn ingest_uses_first_heading_and_toc() {
        use crate::memsink::MemSink;
        let md = b"# Chapter 1: Title\n\nText.\n\n## Sub\n\n### Deep\n";
        let mut sink = MemSink::default();
        ingest(&&md[..], "book.md", &mut sink).unwrap();
        assert_eq!(sink.meta.title, "Chapter 1: Title");
        assert_eq!(sink.chapters.len(), 1);
        assert_eq!(sink.chapters[0].2, 12);
        let depths: Vec<u8> = sink.toc.iter().map(|t| t.depth).collect();
        assert_eq!(depths, [0, 1, 2]);
        let mut sink = MemSink::default();
        ingest(&&b"just text\n"[..], "/x/my_notes.md", &mut sink).unwrap();
        assert_eq!(sink.meta.title, "My notes");
        assert_eq!(sink.toc.len(), 1);
    }

    // --- adversarial ---------------------------------------------------------------------

    #[test]
    fn adversarial_deep_nesting_does_not_recurse() {
        let mut md = String::new();
        for _ in 0..20_000 {
            md.push_str("> ");
        }
        md.push_str("deep\n");
        assert_eq!(text(&md), "deep\n");
        let mut md = String::new();
        let mut indent = String::new();
        for i in 0..5_000 {
            md.push_str(&indent);
            md.push_str("- item\n");
            if i < 300 {
                indent.push_str("  ");
            }
        }
        let (bytes, chars, _, _) = to_qtx(&md);
        assert_eq!(chars, 4 * 5_000);
        assert!(Reader::new(&bytes).any(|t| t == item(false, 255, 1)));
        let mut md = String::new();
        for _ in 0..3_000 {
            md.push_str("*[");
        }
        md.push('a');
        for _ in 0..3_000 {
            md.push_str("]*");
        }
        assert!(text(&md).contains('a'));
    }

    #[test]
    fn adversarial_huge_flat_list_saturates_index() {
        let mut md = String::new();
        for _ in 0..100_000 {
            md.push_str("1. item\n");
        }
        let (bytes, chars, _, _) = to_qtx(&md);
        assert_eq!(chars, 4 * 100_000);
        let last = Reader::new(&bytes).filter(|t| matches!(t, Token::Para(ParaKind::ListItem { .. }))).last();
        assert_eq!(last, Some(item(true, 0, u16::MAX)));
        let mut md = String::new();
        for _ in 0..100_000 {
            md.push_str("- item\n\n");
        }
        let (_, chars, _, _) = to_qtx(&md);
        assert_eq!(chars, 4 * 100_000);
    }

    #[test]
    fn adversarial_huge_lines_and_unclosed_delimiters() {
        let big = "x".repeat(1024 * 1024);
        let (bytes, chars, _, _) = to_qtx(&big);
        assert_eq!(chars, 1024 * 1024);
        assert_eq!(Reader::new(&bytes).count(), 3);
        for pat in ["*", "_", "~", "`", "|", "<!--", "\\", "- ", "> ", "#", "1. ", "    ", "\t", "|-"] {
            let md = pat.repeat(200_000 / pat.len());
            let (bytes, _, _, _) = to_qtx(&md);
            assert!(Reader::new(&bytes).count() < 400_000);
        }
        for pat in ["[", "![", "<", "&", "]", "*a", "a*", "**a", "_a_b", "[a](", "[^", "<a ", "&#"] {
            let md = pat.repeat(200_000 / pat.len());
            let (_, chars, _, _) = to_qtx(&md);
            assert!(chars > 0, "{pat:?} lost its text");
        }
        let md = alloc::format!("{}a{}", "*".repeat(50_000), "*".repeat(50_000));
        let t = toks(&md);
        // 25k nested strong pairs, one style token to open and one to close each.
        assert_eq!(t.iter().filter(|t| matches!(t, Token::Style(_))).count(), 50_000);
        assert!(t.contains(&tx("a")));
        // Every emphasis run open at the end of a block is plain text.
        let md = "*a ".repeat(100_000);
        assert_eq!(to_qtx(&md).1 as usize, md.trim_end().chars().count());
        let md = alloc::format!("{}x", "[".repeat(100_000));
        assert_eq!(to_qtx(&md).1, 100_001);
    }

    #[test]
    fn adversarial_tables_and_definitions() {
        let mut md = String::from("| a |\n|---|\n");
        for _ in 0..50_000 {
            md.push_str("| 1 | 2 | 3 | 4 | 5 |\n");
        }
        let (_, chars, _, _) = to_qtx(&md);
        assert_eq!(chars as usize, 4 + 50_000 * 4);
        let wide = alloc::format!("{}\n{}\n| x |\n", "| h ".repeat(300_000), "|---".repeat(300_000));
        assert!(to_qtx(&wide).1 > 0);
        let mut md = String::new();
        for i in 0..50_000 {
            md.push_str(&alloc::format!("[l{i}]: /u{i}\n"));
        }
        md.push_str("\n[l49999] [L0] [nope]\n");
        assert_eq!(text(&md), "l49999 L0 [nope]\n");
        let mut md = String::new();
        for i in 0..20_000 {
            md.push_str(&alloc::format!("a[^{i}] "));
        }
        md.push('\n');
        for i in 0..20_000 {
            md.push_str(&alloc::format!("[^{i}]: n\n"));
        }
        assert!(to_qtx(&md).1 > 0);
    }

    #[test]
    fn adversarial_random_bytes_and_truncations() {
        let seed_doc = "# T\n\n> - *a* **b** [c](d) `e` ![f](g)\n>   1. h\n\n| i | j |\n|---|---|\n| k | l |\n\n```\nm\n```\n\n[^n]: o\n\ntext[^n] &amp; \\* <b>p</b>\n";
        for cut in 0..seed_doc.len() {
            if seed_doc.is_char_boundary(cut) {
                let _ = to_qtx(&seed_doc[..cut]);
                let _ = to_qtx(&seed_doc[cut..]);
            }
        }
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..200 {
            let mut s = String::new();
            for _ in 0..400 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let pick = b" \t\n\r*_~`[]()!<>#-+=|\\&:.^0123456789abc\"'";
                s.push(pick[(x % pick.len() as u64) as usize] as char);
            }
            let _ = to_qtx(&s);
        }
    }
}

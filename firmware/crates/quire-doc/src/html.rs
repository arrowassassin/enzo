//! A tolerant HTML/XHTML pull tokenizer and its conversion to QTX. Handles real-world
//! EPUB markup (unclosed `<p>`, self-closing tags, entities, CDATA, comments, `<br/>`)
//! without building a DOM or an event list: the tokenizer yields one borrowed event at a
//! time and a small element stack plus a paragraph state machine drive the writer.
//!
//! Memory: the tokenizer borrows text and attribute values from the source whenever no
//! entity decoding is needed, and the converter holds only the current paragraph's state,
//! so a chapter costs its source bytes plus its QTX output. Sources larger than the
//! caller's budget can be fed in windows (see [`Tokenizer::window`]).

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_qtx::{style, ParaKind, Reader, Token, Writer};

use crate::{DocError, Sink};

/// Largest text run (bytes) a single paragraph carries before it is split at a sentence
/// boundary into consecutive paragraphs. Keeps the layout engine's per-paragraph
/// buffers bounded on hard-wrapped or unstructured input.
pub const PARA_LIMIT: usize = 4096;

/// A borrowed tokenizer event. Text and attribute values borrow from the source unless
/// an entity had to be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok<'a> {
    /// `<name attr=...>`; `attrs` are raw (name, value) pairs, values entity-decoded.
    Open {
        /// Tag name without namespace prefix.
        name: &'a str,
        /// Attributes in source order.
        attrs: Vec<(&'a str, Cow<'a, str>)>,
        /// Ends with `/>`.
        self_closing: bool,
    },
    /// `</name>`.
    Close(&'a str),
    /// Text (entities decoded, whitespace not yet collapsed).
    Text(Cow<'a, str>),
}

/// An owned tokenizer event, for callers that collect a whole (small) document such as
/// a package document or an FB2 file. Chapter conversion uses [`Tok`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ev<'a> {
    /// `<name attr=...>`; `attrs` are raw (name, value) pairs, values entity-decoded.
    Open {
        /// Tag name without namespace prefix.
        name: &'a str,
        /// Attributes in source order.
        attrs: Vec<(&'a str, String)>,
        /// Ends with `/>`.
        self_closing: bool,
    },
    /// `</name>`.
    Close(&'a str),
    /// Text (entities decoded, whitespace not yet collapsed).
    Text(String),
}

impl<'a> From<Tok<'a>> for Ev<'a> {
    fn from(t: Tok<'a>) -> Self {
        match t {
            Tok::Open { name, attrs, self_closing } => {
                Ev::Open { name, attrs: attrs.into_iter().map(|(k, v)| (k, v.into_owned())).collect(), self_closing }
            }
            Tok::Close(n) => Ev::Close(n),
            Tok::Text(t) => Ev::Text(t.into_owned()),
        }
    }
}

/// Tokenize a whole document held in memory into owned events. Small documents only
/// (package documents, FB2); chapters go through [`Tokenizer`] directly.
pub fn tokenize(src: &str) -> Vec<Ev<'_>> {
    Tokenizer::new(src).map(Ev::from).collect()
}

/// A pull tokenizer over a string slice.
///
/// In *window* mode the tokenizer never emits a construct that might continue past the
/// end of the slice (an open tag, comment, CDATA, or the last word of a text run); it
/// stops there and [`Tokenizer::rest`] returns the unconsumed tail for the caller to
/// prepend to the next window.
pub struct Tokenizer<'a> {
    src: &'a str,
    pos: usize,
    raw: Option<&'static str>,
    partial: bool,
}

impl<'a> Tokenizer<'a> {
    /// Tokenize a complete document.
    pub fn new(src: &'a str) -> Self {
        Tokenizer { src, pos: 0, raw: None, partial: false }
    }

    /// Tokenize one window of a larger document. `raw` is the raw-text state returned by
    /// the previous window's [`Tokenizer::raw_state`] (inside `<script>`/`<style>`);
    /// `partial` is false for the final window so everything is flushed.
    pub fn window(src: &'a str, raw: Option<&'static str>, partial: bool) -> Self {
        Tokenizer { src, pos: 0, raw, partial }
    }

    /// The unconsumed tail (window mode), to be carried into the next window.
    pub fn rest(&self) -> &'a str {
        &self.src[self.pos.min(self.src.len())..]
    }

    /// Whether the tokenizer is inside `<script>`/`<style>` raw text at the end.
    pub fn raw_state(&self) -> Option<&'static str> {
        self.raw
    }

    fn hold(&mut self, at: usize) -> Option<Tok<'a>> {
        // In window mode leave the tail for the next window; otherwise drop it.
        self.pos = if self.partial { at } else { self.src.len() };
        None
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = Tok<'a>;

    fn next(&mut self) -> Option<Tok<'a>> {
        loop {
            let src = self.src;
            let b = src.as_bytes();
            if self.pos >= b.len() {
                return None;
            }
            if let Some(tag) = self.raw {
                match find_close_tag(&src[self.pos..], tag) {
                    Some(off) => {
                        self.pos += off;
                        self.raw = None;
                    }
                    None => {
                        // Raw content up to the last '<' cannot start the close tag.
                        let keep = src[self.pos + 1..].rfind('<').map(|k| self.pos + 1 + k).unwrap_or(b.len());
                        return self.hold(keep);
                    }
                }
            }
            if b[self.pos] != b'<' {
                let start = self.pos;
                let end = src[start..].find('<').map(|k| start + k).unwrap_or(b.len());
                let mut run = &src[start..end];
                if end == b.len() && self.partial {
                    // Hold back the last, possibly incomplete, word (and any entity in it).
                    match run.rfind(char::is_whitespace) {
                        Some(k) => {
                            let cut = k + run[k..].chars().next().map_or(1, char::len_utf8);
                            run = &run[..cut];
                        }
                        None if run.len() < 256 => return self.hold(start),
                        None => {}
                    }
                }
                self.pos = start + run.len();
                let t = decode_entities(run);
                if !t.is_empty() {
                    return Some(Tok::Text(t));
                }
                continue;
            }
            let rest = &src[self.pos..];
            if let Some(after) = rest.strip_prefix("<!--") {
                match after.find("-->") {
                    Some(e) => {
                        self.pos += 4 + e + 3;
                        continue;
                    }
                    None => return self.hold(self.pos),
                }
            }
            if let Some(after) = rest.strip_prefix("<![CDATA[") {
                match after.find("]]>") {
                    Some(e) => {
                        let t = &after[..e];
                        self.pos += 9 + e + 3;
                        if t.is_empty() {
                            continue;
                        }
                        return Some(Tok::Text(Cow::Borrowed(t)));
                    }
                    None => {
                        if self.partial {
                            return self.hold(self.pos);
                        }
                        self.pos = b.len();
                        return if after.is_empty() { None } else { Some(Tok::Text(Cow::Borrowed(after))) };
                    }
                }
            }
            if rest.starts_with("<!") || rest.starts_with("<?") {
                match rest.find('>') {
                    Some(e) => {
                        self.pos += e + 1;
                        continue;
                    }
                    None => return self.hold(self.pos),
                }
            }
            // A tag. Find its end, honouring quotes.
            let mut j = self.pos + 1;
            let mut quote: Option<u8> = None;
            while j < b.len() {
                match (quote, b[j]) {
                    (Some(q), c) if c == q => quote = None,
                    (None, b'"') | (None, b'\'') => quote = Some(b[j]),
                    (None, b'>') => break,
                    _ => {}
                }
                j += 1;
            }
            if j >= b.len() {
                if self.partial {
                    return self.hold(self.pos);
                }
                // Unterminated tag at the very end: keep it as text.
                let t = decode_entities(rest);
                self.pos = b.len();
                return if t.is_empty() { None } else { Some(Tok::Text(t)) };
            }
            let inner = &src[self.pos + 1..j];
            self.pos = j + 1;
            let closing = inner.starts_with('/');
            let inner = inner.trim_start_matches('/').trim();
            let self_closing = inner.ends_with('/');
            let inner = inner.trim_end_matches('/').trim_end();
            let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
            let name = &inner[..name_end];
            let name = name.rsplit(':').next().unwrap_or(name); // drop namespace prefixes
            if closing {
                return Some(Tok::Close(name));
            }
            let attrs = parse_attrs(&inner[name_end..]);
            if !self_closing {
                if name.eq_ignore_ascii_case("script") {
                    self.raw = Some("script");
                } else if name.eq_ignore_ascii_case("style") {
                    self.raw = Some("style");
                }
            }
            return Some(Tok::Open { name, attrs, self_closing });
        }
    }
}

/// Offset of `</tag` (case-insensitive) in `hay`.
fn find_close_tag(hay: &str, tag: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(k) = hay[from..].find("</") {
        let at = from + k;
        if hay[at + 2..].get(..tag.len()).is_some_and(|n| n.eq_ignore_ascii_case(tag)) {
            return Some(at);
        }
        from = at + 2;
    }
    None
}

fn parse_attrs(s: &str) -> Vec<(&str, Cow<'_, str>)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        if i == ns {
            i += 1;
            continue;
        }
        let name = &s[ns..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = Cow::Borrowed("");
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = decode_entities(&s[vs..i]);
                i += 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = decode_entities(&s[vs..i]);
            }
        }
        out.push((name, value));
    }
    out
}

/// Decode `&amp;`-style entities (named subset, decimal and hex). Borrows when there is
/// nothing to decode.
pub fn decode_entities(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(semi) = after.find(';').filter(|&n| n <= 10) {
            let ent = &after[..semi];
            let decoded = if let Some(num) = ent.strip_prefix('#') {
                let v = if let Some(h) = num.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok() } else { num.parse::<u32>().ok() };
                v.and_then(char::from_u32)
            } else {
                named_entity(ent)
            };
            match decoded {
                Some(c) => {
                    out.push(c);
                    rest = &after[semi + 1..];
                }
                None => {
                    out.push('&');
                    rest = after;
                }
            }
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn named_entity(n: &str) -> Option<char> {
    Some(match n {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{A0}',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "sbquo" => '‚',
        "bdquo" => '„',
        "laquo" => '«',
        "raquo" => '»',
        "lsaquo" => '‹',
        "rsaquo" => '›',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "middot" => '·',
        "bull" => '•',
        "sect" => '§',
        "para" => '¶',
        "dagger" => '†',
        "Dagger" => '‡',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "shy" => '\u{AD}',
        "times" => '×',
        "divide" => '÷',
        "minus" => '−',
        "plusmn" => '±',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "iexcl" => '¡',
        "iquest" => '¿',
        "agrave" => 'à',
        "aacute" => 'á',
        "acirc" => 'â',
        "auml" => 'ä',
        "aring" => 'å',
        "atilde" => 'ã',
        "ccedil" => 'ç',
        "egrave" => 'è',
        "eacute" => 'é',
        "ecirc" => 'ê',
        "euml" => 'ë',
        "igrave" => 'ì',
        "iacute" => 'í',
        "icirc" => 'î',
        "iuml" => 'ï',
        "ntilde" => 'ñ',
        "ograve" => 'ò',
        "oacute" => 'ó',
        "ocirc" => 'ô',
        "ouml" => 'ö',
        "otilde" => 'õ',
        "oslash" => 'ø',
        "ugrave" => 'ù',
        "uacute" => 'ú',
        "ucirc" => 'û',
        "uuml" => 'ü',
        "yacute" => 'ý',
        "yuml" => 'ÿ',
        "szlig" => 'ß',
        "Agrave" => 'À',
        "Aacute" => 'Á',
        "Acirc" => 'Â',
        "Auml" => 'Ä',
        "Aring" => 'Å',
        "Ccedil" => 'Ç',
        "Egrave" => 'È',
        "Eacute" => 'É',
        "Ecirc" => 'Ê',
        "Euml" => 'Ë',
        "Iacute" => 'Í',
        "Ntilde" => 'Ñ',
        "Oacute" => 'Ó',
        "Ocirc" => 'Ô',
        "Ouml" => 'Ö',
        "Oslash" => 'Ø',
        "Uacute" => 'Ú',
        "Uuml" => 'Ü',
        "aelig" => 'æ',
        "AElig" => 'Æ',
        "oelig" => 'œ',
        "OElig" => 'Œ',
        "thinsp" => '\u{2009}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "zwj" => '\u{200D}',
        "zwnj" => '\u{200C}',
        "lrm" => '\u{200E}',
        "rlm" => '\u{200F}',
        "prime" => '′',
        "Prime" => '″',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "hearts" => '♥',
        "infin" => '∞',
        "ne" => '≠',
        "le" => '≤',
        "ge" => '≥',
        _ => return None,
    })
}

/// An image the caller has already stored, so the converter can write its final id and
/// size instead of a placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// The `src` attribute exactly as it appears in the markup.
    pub src: String,
    /// Stored image id.
    pub id: u16,
    /// Stored width.
    pub w: u16,
    /// Stored height.
    pub h: u16,
}

/// Placeholder id for an image that could not be stored; the renderer draws a frame.
pub const MISSING_IMAGE: u16 = u16::MAX;

/// Maps an `href` path (fragment removed, relative to the chapter) to an ingest chapter
/// index; `None` when the file is not a chapter.
pub type LinkResolver<'r> = &'r dyn Fn(&str) -> Option<u16>;

/// A chapter opening found at the start of a chapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChapterHeading {
    /// Number as text ("1", "XII", "Twenty").
    pub number: Option<String>,
    /// Title without the number.
    pub title: Option<String>,
}

impl ChapterHeading {
    /// A display form for section lists: `"1 · Loomings"`, or whichever part exists.
    pub fn display(&self) -> Option<String> {
        match (&self.number, &self.title) {
            (Some(n), Some(t)) => Some(alloc::format!("{n} · {t}")),
            (Some(n), None) => Some(n.clone()),
            (None, Some(t)) => Some(t.clone()),
            (None, None) => None,
        }
    }
}

/// Collects `<img src>` / `<image href>` sources: the cheap first pass that lets images
/// be decoded and stored before the text is converted.
#[derive(Default, Debug)]
pub struct ImageScan {
    /// Distinct sources in document order.
    pub srcs: Vec<String>,
}

impl ImageScan {
    /// Feed one event.
    pub fn event(&mut self, t: &Tok<'_>) {
        if let Tok::Open { name, attrs, .. } = t {
            if name.eq_ignore_ascii_case("img") || name.eq_ignore_ascii_case("image") {
                if let Some(src) = image_src(attrs) {
                    if !self.srcs.iter().any(|s| s == src) {
                        self.srcs.push(src.to_string());
                    }
                }
            }
        }
    }
}

/// Image sources of a document held in memory.
pub fn scan_images(src: &str) -> Vec<String> {
    let mut s = ImageScan::default();
    for t in Tokenizer::new(src) {
        s.event(&t);
    }
    s.srcs
}

fn image_src<'b>(attrs: &'b [(&str, Cow<'_, str>)]) -> Option<&'b str> {
    let src = attr(attrs, "src").or_else(|| attr(attrs, "href")).or_else(|| attr(attrs, "xlink:href"))?;
    if src.is_empty() || src.starts_with("data:") {
        return None;
    }
    Some(src)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SkipKind {
    Other,
    PageBreak,
    Svg,
}

/// Text of a heading being captured as the chapter opening.
struct HeadingCapture {
    level: u8,
    text: String,
    number: String,
    title: String,
    /// Bucket per open inline element: 0 none, 1 number, 2 title.
    buckets: Vec<u8>,
    pending_space: bool,
}

/// A class-less `<div>` whose children might all be verse lines; its output is buffered
/// (bounded) until the verdict is known.
struct VerseCandidate {
    saved: Writer,
    depth: i32,
    children: u32,
    cur_chars: u32,
    cur_last: char,
    punct: u32,
    block_index: usize,
}

const CANDIDATE_MAX_CHILDREN: u32 = 48;
const CANDIDATE_MAX_BYTES: usize = 6 * 1024;

/// Builds QTX from tokenizer events. Reused by EPUB, Markdown (inline HTML) and
/// standalone HTML files; FB2 has its own mapping.
pub struct Converter<'r> {
    /// The stream.
    pub w: Writer,
    chars_flushed: u32,
    in_para: bool,
    para_kind: ParaKind,
    style: u8,
    style_stack: Vec<u8>,
    pending_space: bool,
    para_kind_stack: Vec<ParaKind>,
    /// For each open block container: whether it pushed onto `para_kind_stack`.
    block_stack: Vec<bool>,
    list_stack: Vec<(bool, u16)>, // (ordered, next index)
    pre_depth: u32,
    skip_depth: u32,
    skip_kind: SkipKind,
    a_stack: Vec<bool>,
    seen_text_in_para: bool,
    para_bytes: usize,
    last_char: char,
    /// Title text if a `<title>` was present.
    pub title: Option<String>,
    in_title: bool,
    /// Whether any body text was emitted.
    pub text_emitted: bool,
    /// Number of `Image` tokens written.
    pub images_emitted: u16,
    /// The chapter opening, when the first block was an `<h1>`/`<h2>`.
    pub chapter_title: Option<ChapterHeading>,
    opening_done: bool,
    heading: Option<HeadingCapture>,
    candidate: Option<VerseCandidate>,
    chapter: u16,
    resolver: Option<LinkResolver<'r>>,
    images: Vec<ImageRef>,
}

impl Default for Converter<'_> {
    fn default() -> Self {
        Self::new(0)
    }
}

impl<'r> Converter<'r> {
    /// New converter for ingest chapter `chapter` (used for same-file link targets).
    pub fn new(chapter: u16) -> Self {
        Converter {
            w: Writer::new(),
            chars_flushed: 0,
            in_para: false,
            para_kind: ParaKind::Body,
            style: 0,
            style_stack: Vec::new(),
            pending_space: false,
            para_kind_stack: Vec::new(),
            block_stack: Vec::new(),
            list_stack: Vec::new(),
            pre_depth: 0,
            skip_depth: 0,
            skip_kind: SkipKind::Other,
            a_stack: Vec::new(),
            seen_text_in_para: false,
            para_bytes: 0,
            last_char: ' ',
            title: None,
            in_title: false,
            text_emitted: false,
            images_emitted: 0,
            chapter_title: None,
            opening_done: false,
            heading: None,
            candidate: None,
            chapter,
            resolver: None,
            images: Vec::new(),
        }
    }

    /// Resolve `href` paths (without fragment) to ingest chapter indexes. Unresolved
    /// paths keep their raw href.
    pub fn with_resolver(mut self, r: LinkResolver<'r>) -> Self {
        self.resolver = Some(r);
        self
    }

    /// Images already stored for this chapter, by `src`.
    pub fn with_images(mut self, images: Vec<ImageRef>) -> Self {
        self.images = images;
        self
    }

    /// Feed one event.
    pub fn event(&mut self, t: Tok<'_>) {
        match t {
            Tok::Open { name, attrs, self_closing } => {
                self.open(name, &attrs);
                if self_closing {
                    self.close(name);
                }
            }
            Tok::Close(name) => self.close(name),
            Tok::Text(t) => self.text(&t),
        }
    }

    /// Tokenize and feed a complete document (or a self-contained fragment).
    pub fn feed_str(&mut self, src: &str) {
        for t in Tokenizer::new(src) {
            self.event(t);
        }
    }

    /// Take the bytes written so far (streaming callers hand them to the sink between
    /// windows). Any pending verse candidate is settled first.
    pub fn flush_bytes(&mut self) -> Vec<u8> {
        if self.candidate.is_some() {
            let verse = self.candidate_verdict();
            self.finish_candidate(verse);
        }
        self.chars_flushed += self.w.char_count();
        core::mem::take(&mut self.w).finish()
    }

    /// Characters of text written so far, including flushed bytes.
    pub fn chars(&self) -> u32 {
        self.chars_flushed + self.w.char_count()
    }

    /// What was found besides text.
    pub fn info(&self) -> ConverterInfo {
        ConverterInfo {
            title: self.title.clone(),
            chapter_title: self.chapter_title.clone(),
            text_emitted: self.text_emitted,
            images_emitted: self.images_emitted,
        }
    }

    /// Finish the stream: remaining bytes, the total character count, and what was found.
    pub fn finish(mut self) -> (Vec<u8>, u32, ConverterInfo) {
        self.finish_heading();
        self.end_para();
        if self.candidate.is_some() {
            let verse = self.candidate_verdict();
            self.finish_candidate(verse);
        }
        let chars = self.chars();
        let info = self.info();
        (self.w.finish(), chars, info)
    }

    fn current_kind(&self) -> ParaKind {
        self.para_kind_stack.last().copied().unwrap_or(ParaKind::Body)
    }

    fn start_para(&mut self, kind: ParaKind) {
        self.end_para();
        self.w.para(kind);
        if self.style != 0 {
            self.w.style(self.style);
        }
        self.in_para = true;
        self.para_kind = kind;
        self.pending_space = false;
        self.seen_text_in_para = false;
        self.para_bytes = 0;
        self.opening_done = true;
    }

    fn end_para(&mut self) {
        if self.in_para {
            self.in_para = false;
            self.pending_space = false;
            if self.seen_text_in_para {
                self.w.push(&Token::End);
            }
        }
    }

    fn ensure_para(&mut self) {
        if !self.in_para {
            let k = self.current_kind();
            self.start_para(k);
        }
    }

    fn set_style(&mut self, flags: u8) {
        if flags != self.style {
            self.style = flags;
            if self.in_para {
                self.w.style(flags);
            }
        }
    }

    fn push_style(&mut self, add: u8) {
        self.style_stack.push(self.style);
        let s = self.style | add;
        self.set_style(s);
    }
    fn pop_style(&mut self) {
        if let Some(s) = self.style_stack.pop() {
            self.set_style(s);
        }
    }

    fn push_kind(&mut self, kind: Option<ParaKind>) {
        if let Some(k) = kind {
            self.para_kind_stack.push(k);
        }
        self.block_stack.push(kind.is_some());
    }

    fn pop_block(&mut self) {
        if let Some(true) = self.block_stack.pop() {
            self.para_kind_stack.pop();
        }
    }

    // --- links -------------------------------------------------------------------

    fn split_href<'h>(&self, href: &'h str) -> Option<(&'h str, Option<&'h str>)> {
        let href = href.trim();
        if href.is_empty() || href.contains("://") || href.starts_with("mailto:") {
            return None;
        }
        Some(match href.split_once('#') {
            Some((p, f)) => (p, Some(f)),
            None => (href, None),
        })
    }

    /// `chapter#anchor` for internal links; the full URL for external ones; the raw href
    /// when the file is not a chapter.
    fn link_target(&self, href: &str) -> Option<String> {
        let href = href.trim();
        if href.is_empty() {
            return None;
        }
        let Some((path, frag)) = self.split_href(href) else { return Some(href.into()) };
        if path.is_empty() {
            return Some(alloc::format!("{}#{}", self.chapter, frag.unwrap_or("")));
        }
        let ch = self.resolver.and_then(|r| r(path));
        Some(match (ch, frag) {
            (Some(c), Some(f)) => alloc::format!("{c}#{f}"),
            (Some(c), None) => c.to_string(),
            (None, _) => href.into(),
        })
    }

    /// Footnote targets always take the `chapter#anchor` form; a note file that is not a
    /// chapter is assumed to be this one (standalone HTML is one chapter).
    fn footnote_target(&self, href: &str) -> String {
        let href = href.trim();
        let Some((path, frag)) = self.split_href(href) else { return href.into() };
        let ch = if path.is_empty() { Some(self.chapter) } else { self.resolver.and_then(|r| r(path)) };
        match (ch, frag) {
            (Some(c), Some(f)) => alloc::format!("{c}#{f}"),
            (Some(c), None) => c.to_string(),
            (None, Some(f)) => alloc::format!("{}#{}", self.chapter, f),
            (None, None) => href.into(),
        }
    }

    // --- chapter opening ---------------------------------------------------------

    fn heading_bucket(attrs: &[(&str, Cow<'_, str>)], inherited: u8) -> u8 {
        let et = attr(attrs, "epub:type").unwrap_or("").to_ascii_lowercase();
        let cls = attr(attrs, "class").unwrap_or("").to_ascii_lowercase();
        if et.contains("z3998:roman") || et.contains("ordinal") || cls.contains("number") || cls.split_whitespace().any(|c| c == "num") {
            1
        } else if et.split_whitespace().any(|t| t == "title") || cls.contains("title") {
            2
        } else {
            inherited
        }
    }

    fn finish_heading(&mut self) {
        let Some(cap) = self.heading.take() else { return };
        let text = collapse_ws(&cap.text);
        if text.is_empty() {
            return;
        }
        if text.chars().count() > 200 {
            // Too long for an opening: an ordinary heading paragraph.
            self.start_para(ParaKind::Heading(cap.level));
            self.emit_text(&text);
            self.end_para();
            return;
        }
        let number = trim_number(&cap.number);
        let title = trim_title(&cap.title);
        let (number, title) = match (number, title) {
            (Some(n), Some(t)) => (Some(n), Some(t)),
            (Some(n), None) => {
                let rest = text.replacen(&n, "", 1);
                (Some(n), trim_title(&strip_keyword(&rest)))
            }
            (None, Some(t)) => {
                let rest = text.replacen(&t, "", 1);
                (split_chapter_heading(&rest).0, Some(t))
            }
            (None, None) => split_chapter_heading(&text),
        };
        self.end_para();
        self.w.push(&Token::ChapterTitle { number: number.clone(), title: title.clone() });
        self.chapter_title = Some(ChapterHeading { number, title });
        self.opening_done = true;
        self.text_emitted = true;
    }

    // --- verse candidates --------------------------------------------------------

    fn candidate_verdict(&self) -> bool {
        self.candidate.as_ref().is_some_and(|c| c.children >= 3 && c.punct * 2 <= c.children)
    }

    fn finish_candidate(&mut self, verse: bool) {
        let Some(c) = self.candidate.take() else { return };
        let sub = core::mem::replace(&mut self.w, c.saved).finish();
        for t in Reader::new(&sub) {
            match t {
                Token::Para(ParaKind::Body) if verse => self.w.push(&Token::Para(ParaKind::Verse)),
                other => self.w.push(&other),
            }
        }
        if verse && self.in_para && self.para_kind == ParaKind::Body {
            self.para_kind = ParaKind::Verse;
        }
    }

    /// Settle a candidate that hit its size cap: commit to verse for the rest of the
    /// container when the evidence is strong, else give up.
    fn cap_candidate(&mut self) {
        let Some(c) = self.candidate.as_ref() else { return };
        let strong = c.children >= 8 && c.punct * 2 <= c.children;
        let block_index = c.block_index;
        self.finish_candidate(strong);
        if strong {
            if let Some(b) = self.block_stack.get_mut(block_index) {
                if !*b {
                    *b = true;
                    self.para_kind_stack.push(ParaKind::Verse);
                }
            }
        }
    }

    // --- elements ----------------------------------------------------------------

    fn open(&mut self, name: &str, attrs: &[(&str, Cow<'_, str>)]) {
        let n = name.to_ascii_lowercase();
        let n = n.as_str();
        if self.skip_depth > 0 {
            if self.skip_kind == SkipKind::Svg && n == "image" && self.heading.is_none() {
                self.image(attrs);
            }
            self.skip_depth += 1;
            return;
        }
        for (k, v) in attrs {
            if (k.eq_ignore_ascii_case("id") || (n == "a" && k.eq_ignore_ascii_case("name"))) && !v.is_empty() {
                self.w.push(&Token::Anchor(v.to_string()));
            }
        }
        if is_page_or_line_number(attrs) {
            self.skip_depth = 1;
            self.skip_kind = SkipKind::PageBreak;
            return;
        }
        if let Some(cap) = self.heading.as_mut() {
            if n == "br" {
                cap.pending_space = true;
            }
            let inherited = cap.buckets.last().copied().unwrap_or(0);
            let b = Self::heading_bucket(attrs, inherited);
            cap.buckets.push(b);
            return;
        }
        if let Some(c) = self.candidate.as_mut() {
            c.depth += 1;
            let bad = if c.depth == 1 {
                c.cur_chars = 0;
                c.cur_last = ' ';
                (n != "div" && n != "p") || is_verse_attrs(attrs)
            } else {
                n == "br" || is_block_tag(n)
            };
            if bad {
                self.finish_candidate(false);
            }
        }
        let verse = is_verse_attrs(attrs);
        match n {
            "script" | "style" | "math" | "template" | "noscript" => {
                self.skip_depth = 1;
                self.skip_kind = SkipKind::Other;
            }
            "svg" => {
                self.skip_depth = 1;
                self.skip_kind = SkipKind::Svg;
            }
            "title" => self.in_title = true,
            "p" => {
                let cls = attr(attrs, "class").unwrap_or("").to_ascii_lowercase();
                let kind = if verse {
                    ParaKind::Verse
                } else if cls.contains("caption") {
                    ParaKind::Caption
                } else if cls.contains("center") || cls.contains("centre") {
                    ParaKind::Centered
                } else {
                    self.current_kind()
                };
                self.start_para(kind);
            }
            "div" | "section" | "article" | "aside" | "header" | "footer" | "main" | "figure" | "address" | "nav" => {
                self.end_para();
                self.push_kind(verse.then_some(ParaKind::Verse));
                if n == "div"
                    && !verse
                    && self.candidate.is_none()
                    && self.opening_done
                    && self.current_kind() == ParaKind::Body
                    && self.list_stack.is_empty()
                {
                    let saved = core::mem::take(&mut self.w);
                    self.candidate = Some(VerseCandidate {
                        saved,
                        depth: 0,
                        children: 0,
                        cur_chars: 0,
                        cur_last: ' ',
                        punct: 0,
                        block_index: self.block_stack.len() - 1,
                    });
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = (n.as_bytes()[1] - b'0').min(3);
                if level <= 2 && !self.opening_done && !self.in_para && self.candidate.is_none() {
                    self.heading = Some(HeadingCapture {
                        level,
                        text: String::new(),
                        number: String::new(),
                        title: String::new(),
                        buckets: Vec::new(),
                        pending_space: false,
                    });
                } else {
                    self.start_para(ParaKind::Heading(level));
                }
            }
            "blockquote" => {
                self.end_para();
                self.push_kind(Some(if verse { ParaKind::Verse } else { ParaKind::Quote }));
            }
            "pre" => {
                self.pre_depth += 1;
                let kind = if verse || self.current_kind() == ParaKind::Verse { ParaKind::Verse } else { ParaKind::Code };
                self.start_para(kind);
            }
            "code" | "kbd" | "samp" | "tt" => self.push_style(style::MONO),
            "ul" => {
                self.end_para();
                self.list_stack.push((false, 1));
            }
            "ol" => {
                self.end_para();
                let start = attr(attrs, "start").and_then(|s| s.parse::<u16>().ok()).unwrap_or(1);
                self.list_stack.push((true, start));
            }
            "li" => {
                let level = self.list_stack.len().saturating_sub(1) as u8;
                let (ordered, idx) = self.list_stack.last().copied().unwrap_or((false, 1));
                // The layout draws the ordinal itself from `index`; the text carries none.
                self.start_para(ParaKind::ListItem { ordered, level, index: idx });
                if let Some(top) = self.list_stack.last_mut() {
                    top.1 = top.1.saturating_add(1);
                }
            }
            "br" => {
                if self.in_para {
                    self.w.push(&Token::Break);
                    self.pending_space = false;
                }
            }
            "hr" => {
                self.end_para();
                self.w.push(&Token::Rule);
                self.opening_done = true;
            }
            "img" | "image" => self.image(attrs),
            "figcaption" => self.start_para(ParaKind::Caption),
            "table" => {
                self.end_para();
                self.push_kind(Some(ParaKind::TableRow));
            }
            "tr" => self.start_para(ParaKind::TableRow),
            "td" | "th" => {
                if self.seen_text_in_para {
                    self.text_raw(" · ");
                }
                if n == "th" {
                    self.push_style(style::BOLD);
                }
            }
            "em" | "i" | "cite" | "dfn" | "var" => self.push_style(style::ITALIC),
            "strong" | "b" => self.push_style(style::BOLD),
            "u" | "ins" => self.push_style(style::UNDERLINE),
            "s" | "strike" | "del" => self.push_style(style::STRIKE),
            "sup" => self.push_style(style::SUP),
            "sub" => self.push_style(style::SUB),
            "small" => self.push_style(0),
            "span" => {
                let cls = attr(attrs, "class").unwrap_or("").to_ascii_lowercase();
                let sty = attr(attrs, "style").unwrap_or("").to_ascii_lowercase();
                let mut add = 0;
                if cls.contains("italic") || sty.contains("italic") {
                    add |= style::ITALIC;
                }
                if cls.contains("bold") || sty.contains("bold") {
                    add |= style::BOLD;
                }
                if cls.contains("smallcap") || sty.contains("small-caps") {
                    add |= style::SMALLCAPS;
                }
                self.push_style(add);
            }
            "a" => {
                let href = attr(attrs, "href").unwrap_or("");
                let cls = attr(attrs, "class").unwrap_or("").to_ascii_lowercase();
                let role = attr(attrs, "epub:type").or_else(|| attr(attrs, "role")).unwrap_or("").to_ascii_lowercase();
                let mut pushed = false;
                if !href.is_empty() {
                    if role.contains("noteref") || cls.contains("noteref") || cls.contains("footnote") {
                        let target = self.footnote_target(href);
                        self.ensure_para();
                        self.w.push(&Token::Footnote(target));
                        // The marker text is replaced by the footnote number.
                        self.skip_depth = 1;
                        self.skip_kind = SkipKind::Other;
                        return;
                    }
                    if let Some(target) = self.link_target(href) {
                        self.ensure_para();
                        self.w.push(&Token::Link(target));
                        pushed = true;
                    }
                }
                self.a_stack.push(pushed);
            }
            "dt" => self.start_para(ParaKind::Body),
            "dd" => self.start_para(ParaKind::Quote),
            _ => {}
        }
    }

    fn image(&mut self, attrs: &[(&str, Cow<'_, str>)]) {
        let Some(src) = image_src(attrs) else { return };
        let (id, w, h) = self.images.iter().find(|i| i.src == src).map(|i| (i.id, i.w, i.h)).unwrap_or((MISSING_IMAGE, 0, 0));
        self.end_para();
        self.w.push(&Token::Image { id, w, h });
        self.images_emitted = self.images_emitted.saturating_add(1);
        self.opening_done = true;
    }

    fn close(&mut self, name: &str) {
        let n = name.to_ascii_lowercase();
        let n = n.as_str();
        if self.skip_depth > 0 {
            self.skip_depth -= 1;
            if self.skip_depth == 0 && self.skip_kind == SkipKind::PageBreak && self.last_char == '-' {
                // "newly- [209]fallen": the space before a page number after a hyphen is
                // an artefact of the page break.
                self.pending_space = false;
            }
            return;
        }
        if let Some(cap) = self.heading.as_mut() {
            if matches!(n, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                self.finish_heading();
            } else {
                cap.buckets.pop();
            }
            return;
        }
        if let Some(c) = self.candidate.as_mut() {
            if c.depth == 1 {
                if c.cur_chars >= 80 {
                    self.finish_candidate(false);
                } else {
                    c.children += 1;
                    if matches!(c.cur_last, '.' | '!' | '?' | '"' | '”' | '’' | '\'') {
                        c.punct += 1;
                    }
                    c.depth -= 1;
                    if c.children >= CANDIDATE_MAX_CHILDREN || self.w.len() > CANDIDATE_MAX_BYTES {
                        self.cap_candidate();
                    }
                }
            } else if c.depth == 0 {
                let verse = self.candidate_verdict();
                self.finish_candidate(verse);
            } else {
                c.depth -= 1;
            }
        }
        match n {
            "title" => self.in_title = false,
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "figcaption" | "tr" | "dt" | "dd" => self.end_para(),
            "div" | "section" | "article" | "aside" | "header" | "footer" | "main" | "figure" | "address" | "nav" | "blockquote"
            | "table" => {
                self.end_para();
                self.pop_block();
            }
            "pre" => {
                self.pre_depth = self.pre_depth.saturating_sub(1);
                self.end_para();
            }
            "ul" | "ol" => {
                self.end_para();
                self.list_stack.pop();
            }
            "code" | "kbd" | "samp" | "tt" | "em" | "i" | "cite" | "dfn" | "var" | "strong" | "b" | "u" | "ins" | "s" | "strike"
            | "del" | "sup" | "sub" | "small" | "span" => self.pop_style(),
            "th" => self.pop_style(),
            "a" => {
                if let Some(true) = self.a_stack.pop() {
                    if self.in_para {
                        self.w.push(&Token::LinkEnd);
                    }
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, t: &str) {
        if self.skip_depth > 0 {
            return;
        }
        if self.in_title {
            let s = t.trim();
            if !s.is_empty() {
                let mut ti = self.title.take().unwrap_or_default();
                if !ti.is_empty() {
                    ti.push(' ');
                }
                ti.push_str(&collapse_ws(s));
                self.title = Some(ti);
            }
            return;
        }
        if let Some(cap) = self.heading.as_mut() {
            let leading = t.starts_with(|c: char| c.is_whitespace());
            let body = collapse_ws(t);
            if body.is_empty() {
                cap.pending_space |= !t.is_empty();
                return;
            }
            let space = (cap.pending_space || leading) && !cap.text.is_empty();
            push_word(&mut cap.text, &body, space);
            match cap.buckets.last().copied() {
                Some(1) => push_word(&mut cap.number, &body, space),
                Some(2) => push_word(&mut cap.title, &body, space),
                _ => {}
            }
            cap.pending_space = t.ends_with(|c: char| c.is_whitespace());
            return;
        }
        if self.pre_depth > 0 {
            // Preserve whitespace; split on newlines into hard breaks.
            self.ensure_para();
            let mut first = true;
            for line in t.split('\n') {
                if !first {
                    if self.para_bytes > PARA_LIMIT {
                        let k = self.para_kind;
                        self.end_para();
                        self.start_para(k);
                    } else {
                        self.w.push(&Token::Break);
                    }
                }
                first = false;
                if !line.is_empty() {
                    self.emit_text(line);
                }
            }
            return;
        }
        let leading = t.starts_with(|c: char| c.is_whitespace());
        let trailing = t.ends_with(|c: char| c.is_whitespace());
        let body = collapse_ws(t);
        if body.is_empty() {
            if self.in_para && self.seen_text_in_para && (leading || trailing) {
                self.pending_space = true;
            }
            return;
        }
        if let Some(c) = self.candidate.as_mut() {
            if c.depth == 0 {
                self.finish_candidate(false);
            } else {
                c.cur_chars += body.chars().count() as u32;
                c.cur_last = body.chars().next_back().unwrap_or(' ');
            }
        }
        self.ensure_para();
        if (self.pending_space || leading) && self.seen_text_in_para {
            let mut s = String::with_capacity(body.len() + 1);
            s.push(' ');
            s.push_str(&body);
            self.text_raw(&s);
        } else {
            self.text_raw(&body);
        }
        self.pending_space = trailing;
    }

    /// Write a run of text into the current paragraph, splitting the paragraph at a
    /// sentence boundary when it would exceed [`PARA_LIMIT`].
    fn text_raw(&mut self, mut s: &str) {
        loop {
            self.ensure_para();
            let room = PARA_LIMIT.saturating_sub(self.para_bytes);
            if s.len() <= room {
                self.emit_text(s);
                return;
            }
            let mut cut = split_point(s, room);
            if cut == 0 {
                if self.para_bytes > 0 {
                    // No boundary fits: close this paragraph and retry with a fresh one.
                    let k = self.para_kind;
                    self.end_para();
                    self.start_para(k);
                    continue;
                }
                cut = split_point(s, PARA_LIMIT);
                if cut == 0 {
                    cut = floor_char_boundary(s, PARA_LIMIT);
                }
            }
            let (head, tail) = s.split_at(cut);
            self.emit_text(head.trim_end());
            let k = self.para_kind;
            self.end_para();
            self.start_para(k);
            s = tail.trim_start();
            if s.is_empty() {
                return;
            }
        }
    }

    fn emit_text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.w.text(s);
        self.para_bytes += s.len();
        self.seen_text_in_para = true;
        self.text_emitted = true;
        self.opening_done = true;
        self.last_char = s.chars().next_back().unwrap_or(' ');
    }
}

fn push_word(buf: &mut String, body: &str, space: bool) {
    if space && !buf.is_empty() {
        buf.push(' ');
    }
    buf.push_str(body);
}

fn is_block_tag(n: &str) -> bool {
    matches!(
        n,
        "div"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "blockquote"
            | "pre"
            | "img"
            | "image"
            | "hr"
            | "section"
            | "figure"
            | "dl"
    )
}

/// Page-number and line-number markers (Gutenberg `<span class="pagenum">[169]</span>`,
/// `<span class="lnum">20</span>`, EPUB 3 `epub:type="pagebreak"`).
fn is_page_or_line_number(attrs: &[(&str, Cow<'_, str>)]) -> bool {
    let et = attr(attrs, "epub:type").unwrap_or("");
    if et.split_whitespace().any(|t| t.eq_ignore_ascii_case("pagebreak")) {
        return true;
    }
    if attr(attrs, "role").is_some_and(|r| r.eq_ignore_ascii_case("doc-pagebreak")) {
        return true;
    }
    let cls = attr(attrs, "class").unwrap_or("").to_ascii_lowercase();
    ["pagenum", "pageno", "page-number", "lnum", "linenum", "line-number"].iter().any(|k| cls.contains(k))
}

/// Verse containers: class tokens like `line`, `verse`, `poem`, `stanza`, `linegroup`,
/// or an `epub:type` of `z3998:verse|poem|stanza|song`.
fn is_verse_attrs(attrs: &[(&str, Cow<'_, str>)]) -> bool {
    let et = attr(attrs, "epub:type").unwrap_or("");
    if ["z3998:verse", "z3998:poem", "z3998:stanza", "z3998:song"].iter().any(|k| et.contains(k)) {
        return true;
    }
    let cls = attr(attrs, "class").unwrap_or("");
    cls.split_whitespace().any(|c| {
        let c = c.to_ascii_lowercase();
        (c.starts_with("line") && !c.contains("num"))
            || ["verse", "poem", "poetry", "stanza"].iter().any(|k| {
                // "verse", "poem-rw", and Gutenberg/Standard Ebooks compounds such as
                // "extract-verse-rw"; not "universe" or "reverse".
                c.starts_with(k) || c.contains(&alloc::format!("-{k}")) || c.contains(&alloc::format!("_{k}"))
            })
    })
}

fn attr<'b>(attrs: &'b [(&str, Cow<'_, str>)], name: &str) -> Option<&'b str> {
    attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_ref())
}

/// Collapse runs of whitespace to single spaces and trim.
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() && c != '\u{A0}' {
            if !space && !out.is_empty() {
                space = true;
            }
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    out
}

// --- chapter headings ----------------------------------------------------------------

const HEADING_WORDS: &[&str] = &[
    "chapter",
    "book",
    "part",
    "canto",
    "letter",
    "section",
    "volume",
    "act",
    "scene",
    "stave",
    "kapitel",
    "chapitre",
    "capitulo",
    "capítulo",
];

const NUMBER_WORDS: &[&str] = &[
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "last",
];

fn is_roman(s: &str) -> bool {
    !s.is_empty() && s.len() <= 8 && s.bytes().all(|b| matches!(b, b'I' | b'V' | b'X' | b'L' | b'C' | b'D' | b'M'))
}

fn is_number_token(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if s.bytes().all(|b| b.is_ascii_digit()) || is_roman(s) {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    lower.split('-').all(|p| NUMBER_WORDS.contains(&p)) || (lower.starts_with("the ") && NUMBER_WORDS.contains(&&lower[4..]))
}

fn is_separator(c: char) -> bool {
    matches!(c, '.' | ':' | ';' | '—' | '–' | '-' | ')' | ',' | '|' | '·')
}

fn trim_number(s: &str) -> Option<String> {
    let t = s.trim().trim_end_matches(is_separator).trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn trim_title(s: &str) -> Option<String> {
    let t = s.trim().trim_start_matches(|c: char| is_separator(c) || c.is_whitespace()).trim_end();
    let t = match t.strip_suffix('.') {
        Some(u) if !u.ends_with('.') => u,
        _ => t,
    };
    let t = t.trim_end_matches(':').trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Drop a leading "Chapter"-style keyword.
fn strip_keyword(s: &str) -> String {
    let t = s.trim();
    let word_end = t.find(|c: char| !c.is_alphabetic()).unwrap_or(t.len());
    if HEADING_WORDS.contains(&t[..word_end].to_ascii_lowercase().as_str()) {
        t[word_end..].to_string()
    } else {
        t.to_string()
    }
}

/// Split a heading such as "Chapter 1. Loomings.", "CHAPTER XII", "Chapter Twenty",
/// "XII.", "1", "I. THE BURIAL OF THE DEAD" into number and title. Either may be
/// absent; a heading with no recognisable number is all title.
pub fn split_chapter_heading(text: &str) -> (Option<String>, Option<String>) {
    let t = collapse_ws(text);
    let t = t.trim();
    if t.is_empty() {
        return (None, None);
    }
    // "Chapter <number>[ separator][ title]"
    let word_end = t.find(|c: char| !c.is_alphabetic()).unwrap_or(t.len());
    if HEADING_WORDS.contains(&t[..word_end].to_ascii_lowercase().as_str()) {
        let rest = t[word_end..].trim_start_matches(|c: char| c.is_whitespace());
        if rest.is_empty() {
            return (None, Some(t.to_string()));
        }
        // The number runs to the first separator or whitespace, with "the First" and
        // hyphenated words ("Twenty-One") allowed.
        let rest_lower = rest.to_ascii_lowercase();
        let take_the = rest_lower.starts_with("the ");
        let scan_from = if take_the { 4 } else { 0 };
        let num_end = rest[scan_from..]
            .char_indices()
            .find(|&(i, c)| {
                let hyphen_in_word = c == '-' && rest[scan_from + i + 1..].starts_with(|n: char| n.is_alphanumeric());
                c.is_whitespace() || (is_separator(c) && !hyphen_in_word)
            })
            .map(|(i, _)| i + scan_from)
            .unwrap_or(rest.len());
        let num = &rest[..num_end];
        if is_number_token(num) {
            let title = trim_title(&rest[num_end..]);
            return (Some(num.to_string()), title);
        }
        return (None, trim_title(t));
    }
    // A bare number: digits or a roman numeral followed by a separator or the end.
    let num_end = t.find(|c: char| c.is_whitespace() || is_separator(c)).unwrap_or(t.len());
    let num = &t[..num_end];
    let after = &t[num_end..];
    let separated = after.is_empty() || after.starts_with(is_separator);
    if separated && (num.bytes().all(|b| b.is_ascii_digit()) || is_roman(num)) && !num.is_empty() {
        return (Some(num.to_string()), trim_title(after));
    }
    (None, trim_title(t))
}

// --- long paragraphs -----------------------------------------------------------------

fn floor_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Where to cut `s` so the head is at most `max` bytes: after the last sentence end,
/// else after the last space; 0 when there is no such place.
fn split_point(s: &str, max: usize) -> usize {
    let max = floor_char_boundary(s, max);
    let head = &s[..max];
    let mut best_sentence = 0;
    let mut best_space = 0;
    let mut prev = ' ';
    let mut prev2 = ' ';
    for (i, c) in head.char_indices() {
        if c.is_whitespace() {
            let end = matches!(prev, '.' | '!' | '?') || (matches!(prev, '"' | '”' | '’' | '\'' | ')') && matches!(prev2, '.' | '!' | '?'));
            if end {
                best_sentence = i + c.len_utf8();
            }
            best_space = i + c.len_utf8();
        }
        prev2 = prev;
        prev = c;
    }
    if best_sentence > max / 4 {
        best_sentence
    } else {
        best_space
    }
}

/// Split a long text into pieces of at most `max` bytes at sentence boundaries, for
/// callers that build paragraphs themselves (plain text).
pub fn split_paragraph(s: &str, max: usize) -> impl Iterator<Item = &str> {
    let mut rest = s;
    core::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        if rest.len() <= max {
            let r = rest;
            rest = "";
            return Some(r);
        }
        let mut cut = split_point(rest, max);
        if cut == 0 {
            cut = floor_char_boundary(rest, max).max(1);
        }
        let (head, tail) = rest.split_at(cut);
        rest = tail.trim_start();
        Some(head.trim_end())
    })
}

// --- entry points --------------------------------------------------------------------

/// Convert an HTML document to QTX bytes, returning `(bytes, chars, info)`.
pub fn to_qtx(src: &str) -> (Vec<u8>, u32, ConverterInfo) {
    let mut c = Converter::new(0);
    c.feed_str(src);
    c.finish()
}

/// What a conversion found besides text.
#[derive(Debug, Default, Clone)]
pub struct ConverterInfo {
    /// The `<title>`.
    pub title: Option<String>,
    /// The chapter opening, if the chapter started with a heading.
    pub chapter_title: Option<ChapterHeading>,
    /// Any text at all.
    pub text_emitted: bool,
    /// Number of image tokens written.
    pub images_emitted: u16,
}

impl ConverterInfo {
    /// Whether the chapter has anything to show.
    pub fn has_content(&self) -> bool {
        self.text_emitted || self.images_emitted > 0
    }
}

/// Ingest a standalone HTML file as a one-chapter book.
pub fn ingest_file<R: ReadAt>(file: &R, name: &str, sink: &mut dyn Sink) -> Result<(), DocError> {
    let len = file.len() as usize;
    if len > 4 * 1024 * 1024 {
        return Err(DocError::TooLarge("html over 4 MB"));
    }
    let data = file.read_range(0, len)?;
    let text = crate::txt::decode_text(&data);
    let (bytes, chars, info) = to_qtx(&text);
    let meta = crate::Metadata { title: info.title.unwrap_or_else(|| crate::title_from_name(name)), ..Default::default() };
    sink.metadata(&meta)?;
    let section = info.chapter_title.as_ref().and_then(ChapterHeading::display).unwrap_or_else(|| meta.title.clone());
    sink.begin_chapter(0, Some(&section))?;
    sink.chapter_bytes(&bytes)?;
    sink.end_chapter(chars)?;
    sink.toc(&[crate::TocEntry { title: meta.title.clone(), chapter: 0, anchor: None, depth: 0 }])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_qtx::Reader;

    fn toks(html: &str) -> Vec<Token> {
        let (bytes, _, _) = to_qtx(html);
        Reader::new(&bytes).collect()
    }

    fn texts(toks: &[Token]) -> Vec<String> {
        toks.iter().filter_map(|t| if let Token::Text(s) = t { Some(s.clone()) } else { None }).collect()
    }

    #[test]
    fn entities_and_attrs() {
        assert_eq!(decode_entities("Tom &amp; Jerry &mdash; &#8220;hi&#x201D; &unknown; &lt;b&gt;"), "Tom & Jerry — “hi” &unknown; <b>");
        assert!(matches!(decode_entities("plain"), Cow::Borrowed(_)));
        let ev = tokenize(r#"<p class="x" id='a1' hidden>Hi<br/>there</p>"#);
        assert!(matches!(&ev[0], Ev::Open { name: "p", attrs, self_closing: false } if attrs.len() == 3 && attrs[1].1 == "a1"));
        assert!(matches!(&ev[2], Ev::Open { name: "br", self_closing: true, .. }));
        // The pull tokenizer borrows when nothing needs decoding.
        let mut tk = Tokenizer::new("<p class=\"x\">Hi &amp; bye</p>");
        assert!(matches!(tk.next(), Some(Tok::Open { attrs, .. }) if matches!(attrs[0].1, Cow::Borrowed("x"))));
        assert!(matches!(tk.next(), Some(Tok::Text(Cow::Owned(_)))));
        assert_eq!(tk.next(), Some(Tok::Close("p")));
        assert_eq!(tk.next(), None);
    }

    #[test]
    fn paragraphs_styles_and_whitespace() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body>\n<h3>Chapter <em>One</em></h3>\n<p>Hello,\n   <b>bold</b> world.</p>\n<p>Second   para</p><script>alert(1)</script></body></html>";
        let (bytes, chars, info) = to_qtx(html);
        assert_eq!(info.title.as_deref(), Some("T"));
        let toks: Vec<Token> = Reader::new(&bytes).collect();
        assert_eq!(texts(&toks), ["Chapter", " One", "Hello,", " bold", " world.", "Second para"]);
        assert!(toks.contains(&Token::Para(ParaKind::Heading(3))));
        assert!(toks.contains(&Token::Style(style::BOLD)));
        assert_eq!(chars as usize, "Chapter One".len() + "Hello, bold world.".len() + "Second para".len());
    }

    #[test]
    fn lists_quotes_pre_and_footnotes() {
        let html = r#"<body><ul><li>one</li><li>two <code>x</code></li></ul><ol start="3"><li>three</li></ol>
<blockquote><p>quoted</p></blockquote><pre>a
  b</pre><p>See<a epub:type="noteref" href="notes.xhtml#n1">1</a> here.</p><hr/><img src="pic.jpg"/></body>"#;
        let (bytes, _, info) = to_qtx(html);
        let toks: Vec<Token> = Reader::new(&bytes).collect();
        assert!(toks.contains(&Token::Para(ParaKind::ListItem { ordered: false, level: 0, index: 1 })));
        assert!(toks.contains(&Token::Para(ParaKind::ListItem { ordered: true, level: 0, index: 3 })));
        assert!(toks.contains(&Token::Para(ParaKind::Quote)));
        assert!(toks.contains(&Token::Para(ParaKind::Code)));
        assert!(toks.contains(&Token::Break), "pre newline becomes a break");
        assert!(toks.contains(&Token::Footnote("0#n1".into())), "footnote target is chapter#anchor: {toks:?}");
        assert!(!toks.iter().any(|t| matches!(t, Token::Text(s) if s == "1")), "noteref text replaced");
        assert!(toks.contains(&Token::Rule));
        assert!(toks.contains(&Token::Image { id: MISSING_IMAGE, w: 0, h: 0 }));
        assert_eq!(info.images_emitted, 1);
        assert_eq!(scan_images(html), ["pic.jpg"]);
    }

    #[test]
    fn unclosed_tags_and_junk_do_not_panic() {
        for junk in ["<p>unclosed <b>bold", "<<<>>>", "<p", "&", "<!-- open comment", "<![CDATA[ x", "<a href='x", "</p></p></b>", ""] {
            let (bytes, _, _) = to_qtx(junk);
            let _: Vec<Token> = Reader::new(&bytes).collect();
        }
    }

    #[test]
    fn chapter_heading_split() {
        let s = |t: &str| split_chapter_heading(t);
        assert_eq!(s("Chapter 1. Loomings."), (Some("1".into()), Some("Loomings".into())));
        assert_eq!(s("CHAPTER XII"), (Some("XII".into()), None));
        assert_eq!(s("Chapter Twenty"), (Some("Twenty".into()), None));
        assert_eq!(s("Chapter Twenty-One: The End"), (Some("Twenty-One".into()), Some("The End".into())));
        assert_eq!(s("XII."), (Some("XII".into()), None));
        assert_eq!(s("1"), (Some("1".into()), None));
        assert_eq!(s("I. THE BURIAL OF THE DEAD"), (Some("I".into()), Some("THE BURIAL OF THE DEAD".into())));
        assert_eq!(s("SECTION IV FAIRY STORIES"), (Some("IV".into()), Some("FAIRY STORIES".into())));
        assert_eq!(s("I Am Legend"), (None, Some("I Am Legend".into())));
        assert_eq!(s("12 Angry Men"), (None, Some("12 Angry Men".into())));
        assert_eq!(s("ETYMOLOGY."), (None, Some("ETYMOLOGY".into())));
        assert_eq!(s("Chapter the Last"), (Some("the Last".into()), None));
    }

    #[test]
    fn chapter_opening_token() {
        // Gutenberg / Moby-Dick style, wrapped in <header> with an anchor span.
        let t = toks(
            r#"<body><section><header><h1><span id="c1">Chapter 1. Loomings.</span></h1></header><p>Call me Ishmael.</p><h2>Later</h2></body>"#,
        );
        assert_eq!(t[0], Token::Anchor("c1".into()));
        assert_eq!(t[1], Token::ChapterTitle { number: Some("1".into()), title: Some("Loomings".into()) });
        assert!(t.contains(&Token::Para(ParaKind::Heading(2))), "subsequent headings stay headings");
        assert!(!t.contains(&Token::Para(ParaKind::Heading(1))));
        // Standard Ebooks style spans.
        let t = toks(
            r#"<body><h2 epub:type="title"><span epub:type="z3998:roman">XII</span> <span epub:type="title">The Sea</span></h2><p>x</p></body>"#,
        );
        assert_eq!(t[0], Token::ChapterTitle { number: Some("XII".into()), title: Some("The Sea".into()) });
        let t = toks(r#"<body><h2><span class="chapter-number">3</span></h2><p>x</p></body>"#);
        assert_eq!(t[0], Token::ChapterTitle { number: Some("3".into()), title: None });
        let t = toks(r#"<body><div class="chapter"><h2>Chapter <span class="chapter-number">3</span>: The Sea</h2><p>x</p></div></body>"#);
        assert_eq!(t[0], Token::ChapterTitle { number: Some("3".into()), title: Some("The Sea".into()) });
        // A heading that is not the first block stays a heading.
        let t = toks("<body><p>Preamble</p><h1>Chapter 2</h1><p>x</p></body>");
        assert!(t.contains(&Token::Para(ParaKind::Heading(1))));
        assert!(!t.iter().any(|t| matches!(t, Token::ChapterTitle { .. })));
        let (_, _, info) = to_qtx("<body><h1>Chapter 1. Loomings.</h1></body>");
        assert_eq!(info.chapter_title.unwrap().display().as_deref(), Some("1 · Loomings"));
    }

    #[test]
    fn page_and_line_numbers_are_dropped() {
        let html = r#"<body><p>Intro.</p><div class="center"><span epub:type="pagebreak" title="169" id="Page_169">169</span></div>
<p>into the newly- <span epub:type="pagebreak" title="209" id="Page_209">209</span>fallen snow</p>
<p>A line<span class="lnum">20</span> and <span class="pagenum">[170]</span>more</p></body>"#;
        let t = toks(html);
        let text = texts(&t).concat();
        assert!(!text.contains("169") && !text.contains("170") && !text.contains("20"), "{text}");
        assert!(text.contains("newly-fallen snow"), "{text}");
        assert!(text.contains("A line and more"), "{text}");
        assert!(t.contains(&Token::Anchor("Page_169".into())), "anchors kept");
    }

    #[test]
    fn verse_markup() {
        let html = r#"<body><h2>I. THE BURIAL</h2><div class="linegroup"><div>April is the cruellest month,</div><div>Lilacs out of the dead land</div></div>
<blockquote epub:type="z3998:verse"><p><span>Frisch weht der Wind</span><br/><span>Der Heimat zu</span></p></blockquote>
<div class="block-rw headline-rw"><p>NOT VERSE</p></div></body>"#;
        let t = toks(html);
        let verses = t.iter().filter(|t| matches!(t, Token::Para(ParaKind::Verse))).count();
        assert_eq!(verses, 3, "{t:?}");
        assert!(t.contains(&Token::Para(ParaKind::Body)), "headline-rw is not verse");
        // Secondary signal: a class-less div of short single-line children.
        let mut html = String::from("<body><p>Prose first.</p><div>");
        for i in 0..6 {
            html.push_str(&alloc::format!("<div>Line number {i} of the poem</div>"));
        }
        html.push_str("</div><div><p>\"Yes.\"</p><p>\"No.\"</p><p>\"Maybe.\"</p></div></body>");
        let t = toks(&html);
        let verses = t.iter().filter(|t| matches!(t, Token::Para(ParaKind::Verse))).count();
        assert_eq!(verses, 6, "{t:?}");
        assert!(texts(&t).contains(&"\"Maybe.\"".to_string()));
    }

    #[test]
    fn link_targets() {
        let map = |p: &str| -> Option<u16> { (p == "ch2.xhtml").then_some(7) };
        let mut c = Converter::new(3).with_resolver(&map);
        c.feed_str(r##"<p><a href="#here">a</a> <a href="ch2.xhtml#x">b</a> <a href="ch2.xhtml">c</a> <a href="http://x.org/p#q">d</a> <a href="notes.xhtml#n">e</a><a epub:type="noteref" href="ch2.xhtml#f1">1</a></p>"##);
        let (bytes, _, _) = c.finish();
        let t: Vec<Token> = Reader::new(&bytes).collect();
        let links: Vec<String> = t.iter().filter_map(|t| if let Token::Link(s) = t { Some(s.clone()) } else { None }).collect();
        assert_eq!(links, ["3#here", "7#x", "7", "http://x.org/p#q", "notes.xhtml#n"]);
        assert!(t.contains(&Token::Footnote("7#f1".into())));
        assert_eq!(t.iter().filter(|t| matches!(t, Token::LinkEnd)).count(), 5);
    }

    #[test]
    fn long_runs_are_split_into_paragraphs() {
        let mut html = String::from("<p>");
        for i in 0..400 {
            html.push_str(&alloc::format!("Sentence number {i} is here. "));
        }
        html.push_str("</p>");
        let t = toks(&html);
        let paras = t.iter().filter(|t| matches!(t, Token::Para(_))).count();
        assert!(paras >= 3, "{paras}");
        let mut cur = 0usize;
        for tok in &t {
            match tok {
                Token::Para(_) => cur = 0,
                Token::Text(s) => {
                    cur += s.len();
                    assert!(cur <= PARA_LIMIT + 64, "paragraph of {cur} bytes");
                    assert!(s.starts_with("Sentence"), "split at a sentence boundary: {s:?}");
                }
                _ => {}
            }
        }
        let all = texts(&t).join(" ");
        assert!(all.contains("number 399 is here."));
        let pieces: Vec<&str> = split_paragraph("aaa. bbb. ccc.", 6).collect();
        assert_eq!(pieces, ["aaa.", "bbb.", "ccc."]);
    }

    fn fixture_chapter() -> String {
        let data: &[u8] = include_bytes!("../fixtures/moby-dick.epub");
        let zip = crate::zip::Zip::open(&data).unwrap();
        let bytes = zip.read_name("OPS/chapter_001.xhtml", 1 << 20).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn windowed_feed_matches_whole_feed() {
        let src = fixture_chapter();
        let (whole, whole_chars, _) = to_qtx(&src);
        for window in [700usize, 4096, 32 * 1024] {
            let mut c = Converter::new(0);
            let mut carry = String::new();
            let mut raw = None;
            let mut pos = 0;
            while pos < src.len() {
                let mut end = (pos + window).min(src.len());
                while !src.is_char_boundary(end) {
                    end -= 1;
                }
                let mut text = core::mem::take(&mut carry);
                text.push_str(&src[pos..end]);
                pos = end;
                let mut tk = Tokenizer::window(&text, raw, pos < src.len());
                for t in tk.by_ref() {
                    c.event(t);
                }
                raw = tk.raw_state();
                carry = tk.rest().to_string();
            }
            assert!(carry.is_empty());
            let (bytes, chars, _) = c.finish();
            assert_eq!(quire_qtx::plain_text(&bytes), quire_qtx::plain_text(&whole), "window {window}");
            assert_eq!(chars, whole_chars);
        }
        // Owned and borrowed tokenizers agree.
        let owned = tokenize(&src);
        let pulled: Vec<Ev> = Tokenizer::new(&src).map(Ev::from).collect();
        assert_eq!(owned, pulled);
        assert!(quire_qtx::plain_text(&whole).starts_with("1 Loomings\nCall me Ishmael."));
    }
}

//! A tolerant HTML/XHTML tokenizer and its conversion to QTX. Handles real-world EPUB
//! markup (unclosed `<p>`, self-closing tags, entities, CDATA, comments, `<br/>`) without
//! building a DOM: a small element stack and a paragraph state machine drive the writer.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_qtx::{style, ParaKind, Token, Writer};

use crate::{DocError, Sink};

/// A tokenizer event.
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

/// Tokenize a whole document held in memory. Chapter-sized inputs only (bounded by the
/// caller); an EPUB chapter is typically 5–60 KB.
pub fn tokenize(src: &str) -> Vec<Ev<'_>> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut text_start = 0usize;
    let mut raw_until: Option<&str> = None; // inside <script>/<style>
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if let Some(tag) = raw_until {
            // Skip until the matching close tag.
            let rest = &src[i..];
            let lower = rest.get(..tag.len() + 2).map(|s| s.to_ascii_lowercase());
            if lower.as_deref() == Some(&alloc::format!("</{tag}")) {
                raw_until = None;
                text_start = i;
            } else {
                i += 1;
                continue;
            }
        }
        // Flush text before the tag.
        if i > text_start {
            let t = decode_entities(&src[text_start..i]);
            if !t.is_empty() {
                out.push(Ev::Text(t));
            }
        }
        // Comments, CDATA, doctype, processing instructions.
        if src[i..].starts_with("<!--") {
            let end = src[i + 4..].find("-->").map(|e| i + 4 + e + 3).unwrap_or(b.len());
            i = end;
            text_start = i;
            continue;
        }
        if src[i..].starts_with("<![CDATA[") {
            let end = src[i + 9..].find("]]>").map(|e| i + 9 + e).unwrap_or(b.len());
            out.push(Ev::Text(String::from(&src[i + 9..end])));
            i = (end + 3).min(b.len());
            text_start = i;
            continue;
        }
        if src[i..].starts_with("<!") || src[i..].starts_with("<?") {
            let end = src[i..].find('>').map(|e| i + e + 1).unwrap_or(b.len());
            i = end;
            text_start = i;
            continue;
        }
        // A tag. Find its end, honouring quotes.
        let mut j = i + 1;
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
            break;
        }
        let inner = &src[i + 1..j];
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/').trim();
        let self_closing = inner.ends_with('/');
        let inner = inner.trim_end_matches('/').trim_end();
        let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
        let name = &inner[..name_end];
        let name = name.rsplit(':').next().unwrap_or(name); // drop namespace prefixes
        if closing {
            out.push(Ev::Close(name));
        } else {
            let attrs = parse_attrs(&inner[name_end..]);
            let lname = name.to_ascii_lowercase();
            if !self_closing && (lname == "script" || lname == "style") {
                raw_until = Some(if lname == "script" { "script" } else { "style" });
            }
            out.push(Ev::Open { name, attrs, self_closing });
        }
        i = j + 1;
        text_start = i;
    }
    if text_start < b.len() {
        let t = decode_entities(&src[text_start..]);
        if !t.is_empty() {
            out.push(Ev::Text(t));
        }
    }
    out
}

fn parse_attrs(s: &str) -> Vec<(&str, String)> {
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
        let mut value = String::new();
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

/// Decode `&amp;`-style entities (named subset, decimal and hex).
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return String::from(s);
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
    out
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

/// Builds QTX from tokenizer events. Reused by EPUB, FB2 (through its own mapping),
/// Markdown (via HTML) and standalone HTML files.
pub struct Converter {
    /// The stream.
    pub w: Writer,
    in_para: bool,
    style: u8,
    style_stack: Vec<u8>,
    pending_space: bool,
    para_kind_stack: Vec<ParaKind>,
    list_stack: Vec<(bool, u16)>, // (ordered, next index)
    pre_depth: u32,
    skip_depth: u32,
    link_depth: u32,
    seen_text_in_para: bool,
    heading_chars: u32,
    /// Image sources encountered, in order; the caller resolves and stores them.
    pub images: Vec<(u16, String)>,
    /// Footnote-like anchors found (id → true).
    pub anchors: Vec<String>,
    /// Title text if a `<title>` was present.
    pub title: Option<String>,
    in_title: bool,
    image_ids: u16,
    /// Whether any body text was emitted.
    pub text_emitted: bool,
    chapter_heading: Option<String>,
    body_seen: bool,
}

impl Default for Converter {
    fn default() -> Self {
        Self::new()
    }
}

impl Converter {
    /// New converter.
    pub fn new() -> Self {
        Converter {
            w: Writer::new(),
            in_para: false,
            style: 0,
            style_stack: Vec::new(),
            pending_space: false,
            para_kind_stack: Vec::new(),
            list_stack: Vec::new(),
            pre_depth: 0,
            skip_depth: 0,
            link_depth: 0,
            seen_text_in_para: false,
            heading_chars: 0,
            images: Vec::new(),
            anchors: Vec::new(),
            title: None,
            in_title: false,
            image_ids: 0,
            text_emitted: false,
            chapter_heading: None,
            body_seen: false,
        }
    }

    /// Feed a whole tokenized document.
    pub fn feed(&mut self, events: &[Ev<'_>]) {
        for ev in events {
            match ev {
                Ev::Open { name, attrs, self_closing } => {
                    self.open(name, attrs);
                    if *self_closing {
                        self.close(name);
                    }
                }
                Ev::Close(name) => self.close(name),
                Ev::Text(t) => self.text(t),
            }
        }
    }

    /// Finish the stream.
    pub fn finish(mut self) -> (Vec<u8>, u32) {
        self.end_para();
        let chars = self.w.char_count();
        (self.w.finish(), chars)
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
        self.pending_space = false;
        self.seen_text_in_para = false;
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

    fn open(&mut self, name: &str, attrs: &[(&str, String)]) {
        let n = name.to_ascii_lowercase();
        let n = n.as_str();
        if self.skip_depth > 0 {
            self.skip_depth += 1;
            return;
        }
        for (k, v) in attrs {
            if k.eq_ignore_ascii_case("id") && !v.is_empty() {
                self.anchors.push(v.clone());
                if self.in_para {
                    self.w.push(&Token::Anchor(v.clone()));
                } else {
                    // Emit as its own token so the block reader tolerates it.
                    self.w.push(&Token::Anchor(v.clone()));
                }
            }
        }
        match n {
            "script" | "style" | "svg" | "math" | "template" | "noscript" => self.skip_depth = 1,
            "title" => self.in_title = true,
            "body" => self.body_seen = true,
            "p" => {
                let cls = attr(attrs, "class").unwrap_or_default().to_ascii_lowercase();
                let kind = if cls.contains("caption") {
                    ParaKind::Caption
                } else if cls.contains("center") || cls.contains("centre") {
                    ParaKind::Centered
                } else {
                    self.current_kind()
                };
                self.start_para(kind);
            }
            "div" | "section" | "article" | "aside" | "header" | "footer" | "main" | "figure" | "address" => {
                self.end_para();
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = (n.as_bytes()[1] - b'0').min(3);
                self.start_para(ParaKind::Heading(level));
                self.heading_chars = self.w.char_count();
            }
            "blockquote" => {
                self.end_para();
                self.para_kind_stack.push(ParaKind::Quote);
            }
            "pre" => {
                self.pre_depth += 1;
                self.start_para(ParaKind::Code);
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
                self.start_para(ParaKind::ListItem { ordered, level, index: idx });
                if let Some(top) = self.list_stack.last_mut() {
                    top.1 += 1;
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
            }
            "img" | "image" => {
                let src = attr(attrs, "src").or_else(|| attr(attrs, "href")).or_else(|| attr(attrs, "xlink:href")).unwrap_or_default();
                if !src.is_empty() && !src.starts_with("data:") {
                    let id = self.image_ids;
                    self.image_ids += 1;
                    self.images.push((id, src));
                    self.end_para();
                    // Dimensions are filled in by ingest after decoding; 0 means unknown.
                    self.w.push(&Token::Image { id, w: 0, h: 0 });
                }
            }
            "figcaption" => self.start_para(ParaKind::Caption),
            "table" => {
                self.end_para();
                self.para_kind_stack.push(ParaKind::TableRow);
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
                let cls = attr(attrs, "class").unwrap_or_default().to_ascii_lowercase();
                let sty = attr(attrs, "style").unwrap_or_default().to_ascii_lowercase();
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
                let href = attr(attrs, "href").unwrap_or_default();
                let cls = attr(attrs, "class").unwrap_or_default().to_ascii_lowercase();
                let role = attr(attrs, "epub:type").or_else(|| attr(attrs, "role")).unwrap_or_default().to_ascii_lowercase();
                self.link_depth += 1;
                if !href.is_empty() {
                    self.ensure_para();
                    if role.contains("noteref") || cls.contains("noteref") || cls.contains("footnote") {
                        let target = href.rsplit('#').next().unwrap_or(&href).into();
                        self.w.push(&Token::Footnote(target));
                        self.skip_depth = 1; // the marker text is replaced by the footnote number
                        return;
                    }
                    self.w.push(&Token::Link(href));
                }
            }
            "dt" => self.start_para(ParaKind::Body),
            "dd" => self.start_para(ParaKind::Quote),
            _ => {}
        }
    }

    fn close(&mut self, name: &str) {
        let n = name.to_ascii_lowercase();
        let n = n.as_str();
        if self.skip_depth > 0 {
            self.skip_depth -= 1;
            return;
        }
        match n {
            "title" => self.in_title = false,
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "figcaption" | "tr" | "dt" | "dd" => self.end_para(),
            "div" | "section" | "article" | "aside" | "header" | "footer" | "main" | "figure" | "address" => self.end_para(),
            "blockquote" => {
                self.end_para();
                self.para_kind_stack.pop();
            }
            "table" => {
                self.end_para();
                self.para_kind_stack.pop();
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
            "a" if self.link_depth > 0 => {
                self.link_depth -= 1;
                if self.in_para {
                    self.w.push(&Token::LinkEnd);
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
        if self.pre_depth > 0 {
            // Preserve whitespace; split on newlines into hard breaks.
            self.ensure_para();
            let mut first = true;
            for line in t.split('\n') {
                if !first {
                    self.w.push(&Token::Break);
                }
                first = false;
                if !line.is_empty() {
                    self.w.text(line);
                    self.seen_text_in_para = true;
                    self.text_emitted = true;
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
        self.ensure_para();
        let mut s = String::with_capacity(body.len() + 1);
        if (self.pending_space || leading) && self.seen_text_in_para {
            s.push(' ');
        }
        s.push_str(&body);
        self.text_raw(&s);
        self.pending_space = trailing;
    }

    fn text_raw(&mut self, s: &str) {
        self.ensure_para();
        self.w.text(s);
        self.seen_text_in_para = true;
        self.text_emitted = true;
        if self.chapter_heading.is_none() && !self.body_seen {
            // nothing
        }
    }
}

fn attr(attrs: &[(&str, String)], name: &str) -> Option<String> {
    attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
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

/// Convert an HTML document to QTX bytes, returning `(bytes, chars, converter)` so the
/// caller can read images, anchors and the title.
pub fn to_qtx(src: &str) -> (Vec<u8>, u32, ConverterInfo) {
    let events = tokenize(src);
    let mut c = Converter::new();
    c.feed(&events);
    let info = ConverterInfo {
        images: core::mem::take(&mut c.images),
        anchors: core::mem::take(&mut c.anchors),
        title: c.title.take(),
        text_emitted: c.text_emitted,
    };
    let (bytes, chars) = c.finish();
    (bytes, chars, info)
}

/// What a conversion found besides text.
#[derive(Debug, Default, Clone)]
pub struct ConverterInfo {
    /// Image ids and their `src` attributes.
    pub images: Vec<(u16, String)>,
    /// Element ids seen.
    pub anchors: Vec<String>,
    /// The `<title>`.
    pub title: Option<String>,
    /// Any text at all.
    pub text_emitted: bool,
}

/// Ingest a standalone HTML file as a one-chapter book.
pub fn ingest_file<R: ReadAt>(file: &R, name: &str, sink: &mut dyn Sink) -> Result<(), DocError> {
    let len = file.len() as usize;
    if len > 4 * 1024 * 1024 {
        return Err(DocError::TooLarge("html over 4 MB"));
    }
    let data = file.read_range(0, len)?;
    let text = crate::txt::decode_bytes(&data);
    let (bytes, chars, info) = to_qtx(&text);
    let meta = crate::Metadata { title: info.title.unwrap_or_else(|| crate::title_from_name(name)), ..Default::default() };
    sink.metadata(&meta)?;
    sink.begin_chapter(0, Some(&meta.title))?;
    sink.chapter_bytes(&bytes)?;
    sink.end_chapter(chars)?;
    sink.toc(&[crate::TocEntry { title: meta.title.clone(), chapter: 0, anchor: None, depth: 0 }])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_qtx::Reader;

    #[test]
    fn entities_and_attrs() {
        assert_eq!(decode_entities("Tom &amp; Jerry &mdash; &#8220;hi&#x201D; &unknown; &lt;b&gt;"), "Tom & Jerry — “hi” &unknown; <b>");
        let ev = tokenize(r#"<p class="x" id='a1' hidden>Hi<br/>there</p>"#);
        assert!(matches!(&ev[0], Ev::Open { name: "p", attrs, self_closing: false } if attrs.len() == 3 && attrs[1].1 == "a1"));
        assert!(matches!(&ev[2], Ev::Open { name: "br", self_closing: true, .. }));
    }

    #[test]
    fn paragraphs_styles_and_whitespace() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body>\n<h1>Chapter <em>One</em></h1>\n<p>Hello,\n   <b>bold</b> world.</p>\n<p>Second   para</p><script>alert(1)</script></body></html>";
        let (bytes, chars, info) = to_qtx(html);
        assert_eq!(info.title.as_deref(), Some("T"));
        let toks: Vec<Token> = Reader::new(&bytes).collect();
        let text: Vec<String> = toks.iter().filter_map(|t| if let Token::Text(s) = t { Some(s.clone()) } else { None }).collect();
        assert_eq!(text, ["Chapter", " One", "Hello,", " bold", " world.", "Second para"]);
        assert!(toks.contains(&Token::Para(ParaKind::Heading(1))));
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
        assert!(toks.contains(&Token::Footnote("n1".into())));
        assert!(!toks.iter().any(|t| matches!(t, Token::Text(s) if s == "1")), "noteref text replaced");
        assert!(toks.contains(&Token::Rule));
        assert_eq!(info.images, alloc::vec![(0u16, String::from("pic.jpg"))]);
    }

    #[test]
    fn unclosed_tags_and_junk_do_not_panic() {
        for junk in ["<p>unclosed <b>bold", "<<<>>>", "<p", "&", "<!-- open comment", "<![CDATA[ x", "<a href='x", "</p></p></b>", ""] {
            let (bytes, _, _) = to_qtx(junk);
            let _: Vec<Token> = Reader::new(&bytes).collect();
        }
    }
}

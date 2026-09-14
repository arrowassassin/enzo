//! PDF on the device: objects, cross-reference tables and streams, object streams,
//! filters, the page tree, and text extraction with font encodings and ToUnicode maps.
//! Text PDFs are reflowed into QTX; scanned pages become image pages. Encrypted files
//! are refused like DRM. Everything is bounded so a hostile file cannot exhaust RAM.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;

use crate::inflate::{Framing, Inflater};
use crate::DocError;

mod text;
pub use text::ingest;

/// Largest inflated stream we hold in memory (content streams, object streams, ToUnicode).
#[cfg(target_os = "none")]
pub(crate) const STREAM_LIMIT: usize = 192 * 1024;
/// Largest inflated stream we hold in memory on the host.
#[cfg(not(target_os = "none"))]
pub(crate) const STREAM_LIMIT: usize = 2 * 1024 * 1024;
/// Largest object we parse.
#[cfg(target_os = "none")]
const OBJ_LIMIT: usize = 64 * 1024;
/// Largest object we parse on the host.
#[cfg(not(target_os = "none"))]
const OBJ_LIMIT: usize = 512 * 1024;

/// A PDF object.
#[derive(Clone, Debug, PartialEq)]
pub enum Obj {
    /// null
    Null,
    /// true / false
    Bool(bool),
    /// Integer.
    Int(i64),
    /// Real number.
    Real(f32),
    /// (string) or <hex>
    Str(Vec<u8>),
    /// /Name (decoded).
    Name(String),
    /// [ ... ]
    Array(Vec<Obj>),
    /// << ... >>
    Dict(Vec<(String, Obj)>),
    /// A stream: its dictionary and the absolute offset and length of the raw data.
    Stream {
        /// The stream dictionary.
        dict: Vec<(String, Obj)>,
        /// Absolute file offset of the data.
        offset: u64,
        /// Raw (encoded) length.
        len: u64,
    },
    /// n g R
    Ref(u32, u16),
}

impl Obj {
    /// Dictionary lookup (works on dicts and stream dicts).
    pub fn get(&self, key: &str) -> Option<&Obj> {
        match self {
            Obj::Dict(d) | Obj::Stream { dict: d, .. } => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// As a number.
    pub fn num(&self) -> Option<f32> {
        match self {
            Obj::Int(i) => Some(*i as f32),
            Obj::Real(r) => Some(*r),
            _ => None,
        }
    }
    /// As an integer.
    pub fn int(&self) -> Option<i64> {
        match self {
            Obj::Int(i) => Some(*i),
            Obj::Real(r) => Some(*r as i64),
            _ => None,
        }
    }
    /// As a name.
    pub fn name(&self) -> Option<&str> {
        if let Obj::Name(n) = self {
            Some(n)
        } else {
            None
        }
    }
    /// As an array.
    pub fn array(&self) -> Option<&[Obj]> {
        if let Obj::Array(a) = self {
            Some(a)
        } else {
            None
        }
    }
    /// The dictionary entries.
    pub fn dict(&self) -> Option<&[(String, Obj)]> {
        match self {
            Obj::Dict(d) | Obj::Stream { dict: d, .. } => Some(d),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Lexer / parser over a byte slice.

pub(crate) fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\n' | b'\r' | b'\t' | b'\x0C' | b'\0')
}
pub(crate) fn is_delim(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

/// A parser over a byte slice with a base offset (so stream data offsets are absolute).
pub struct Lexer<'a> {
    /// Bytes.
    pub b: &'a [u8],
    /// Position.
    pub pos: usize,
    /// Absolute offset of `b[0]` in the file.
    pub base: u64,
}

/// A lexer token.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// Any object that is not an operator/keyword.
    Obj(Obj),
    /// A bare keyword: `obj`, `endobj`, `stream`, `R`, operators in content streams…
    Kw(String),
    /// `[`
    ArrOpen,
    /// `]`
    ArrClose,
    /// `<<`
    DictOpen,
    /// `>>`
    DictClose,
    /// End of input.
    Eof,
}

impl<'a> Lexer<'a> {
    /// New lexer.
    pub fn new(b: &'a [u8], base: u64) -> Self {
        Lexer { b, pos: 0, base }
    }
    /// Skip whitespace and comments.
    pub fn skip_ws(&mut self) {
        while self.pos < self.b.len() {
            let c = self.b[self.pos];
            if is_ws(c) {
                self.pos += 1;
            } else if c == b'%' {
                while self.pos < self.b.len() && self.b[self.pos] != b'\n' && self.b[self.pos] != b'\r' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }
    /// Next token.
    pub fn next_tok(&mut self) -> Tok {
        self.skip_ws();
        if self.pos >= self.b.len() {
            return Tok::Eof;
        }
        let c = self.b[self.pos];
        match c {
            b'[' => {
                self.pos += 1;
                Tok::ArrOpen
            }
            b']' => {
                self.pos += 1;
                Tok::ArrClose
            }
            b'<' => {
                if self.b.get(self.pos + 1) == Some(&b'<') {
                    self.pos += 2;
                    Tok::DictOpen
                } else {
                    self.pos += 1;
                    let mut out = Vec::new();
                    let mut hi: Option<u8> = None;
                    while self.pos < self.b.len() && self.b[self.pos] != b'>' {
                        let d = self.b[self.pos];
                        self.pos += 1;
                        let v = match d {
                            b'0'..=b'9' => d - b'0',
                            b'a'..=b'f' => d - b'a' + 10,
                            b'A'..=b'F' => d - b'A' + 10,
                            _ => continue,
                        };
                        match hi {
                            None => hi = Some(v),
                            Some(h) => {
                                out.push(h * 16 + v);
                                hi = None;
                            }
                        }
                    }
                    if let Some(h) = hi {
                        out.push(h * 16);
                    }
                    self.pos += 1;
                    Tok::Obj(Obj::Str(out))
                }
            }
            b'>' => {
                if self.b.get(self.pos + 1) == Some(&b'>') {
                    self.pos += 2;
                    Tok::DictClose
                } else {
                    self.pos += 1;
                    self.next_tok()
                }
            }
            b'(' => {
                self.pos += 1;
                let mut out = Vec::new();
                let mut depth = 1;
                while self.pos < self.b.len() {
                    let d = self.b[self.pos];
                    self.pos += 1;
                    match d {
                        b'\\' => {
                            let e = *self.b.get(self.pos).unwrap_or(&b'\\');
                            self.pos += 1;
                            match e {
                                b'n' => out.push(b'\n'),
                                b'r' => out.push(b'\r'),
                                b't' => out.push(b'\t'),
                                b'b' => out.push(8),
                                b'f' => out.push(12),
                                b'\n' => {}
                                b'\r' => {
                                    if self.b.get(self.pos) == Some(&b'\n') {
                                        self.pos += 1;
                                    }
                                }
                                b'0'..=b'7' => {
                                    let mut v = (e - b'0') as u32;
                                    for _ in 0..2 {
                                        if let Some(&o) = self.b.get(self.pos) {
                                            if (b'0'..=b'7').contains(&o) {
                                                v = v * 8 + (o - b'0') as u32;
                                                self.pos += 1;
                                            } else {
                                                break;
                                            }
                                        }
                                    }
                                    out.push(v as u8);
                                }
                                other => out.push(other),
                            }
                        }
                        b'(' => {
                            depth += 1;
                            out.push(d);
                        }
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            out.push(d);
                        }
                        _ => out.push(d),
                    }
                }
                Tok::Obj(Obj::Str(out))
            }
            b'/' => {
                self.pos += 1;
                let start = self.pos;
                while self.pos < self.b.len() && !is_ws(self.b[self.pos]) && !is_delim(self.b[self.pos]) {
                    self.pos += 1;
                }
                let raw = &self.b[start..self.pos];
                let mut name = String::with_capacity(raw.len());
                let mut i = 0;
                while i < raw.len() {
                    if raw[i] == b'#' && i + 2 < raw.len() + 1 && i + 2 <= raw.len() - 1 + 1 {
                        let h = |c: u8| (c as char).to_digit(16);
                        if let (Some(a), Some(b2)) = (raw.get(i + 1).and_then(|&c| h(c)), raw.get(i + 2).and_then(|&c| h(c))) {
                            name.push(((a * 16 + b2) as u8) as char);
                            i += 3;
                            continue;
                        }
                    }
                    name.push(raw[i] as char);
                    i += 1;
                }
                Tok::Obj(Obj::Name(name))
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => {
                let start = self.pos;
                self.pos += 1;
                while self.pos < self.b.len()
                    && (self.b[self.pos].is_ascii_digit()
                        || self.b[self.pos] == b'.'
                        || self.b[self.pos] == b'-'
                        || self.b[self.pos] == b'+'
                        || self.b[self.pos] == b'e'
                        || self.b[self.pos] == b'E')
                {
                    self.pos += 1;
                }
                let s = core::str::from_utf8(&self.b[start..self.pos]).unwrap_or("0");
                if s.contains('.') || s.contains('e') || s.contains('E') {
                    Tok::Obj(Obj::Real(parse_real(s)))
                } else {
                    Tok::Obj(Obj::Int(s.parse::<i64>().unwrap_or_else(|_| parse_real(s) as i64)))
                }
            }
            b')' | b'{' | b'}' => {
                self.pos += 1;
                self.next_tok()
            }
            _ => {
                let start = self.pos;
                while self.pos < self.b.len() && !is_ws(self.b[self.pos]) && !is_delim(self.b[self.pos]) {
                    self.pos += 1;
                }
                if self.pos == start {
                    self.pos += 1;
                    return self.next_tok();
                }
                let kw = String::from_utf8_lossy(&self.b[start..self.pos]).into_owned();
                match kw.as_str() {
                    "true" => Tok::Obj(Obj::Bool(true)),
                    "false" => Tok::Obj(Obj::Bool(false)),
                    "null" => Tok::Obj(Obj::Null),
                    _ => Tok::Kw(kw),
                }
            }
        }
    }

    /// Parse one object (arrays and dicts recursively, `n g R` references, streams).
    pub fn parse_obj(&mut self, depth: u32) -> Result<Obj, DocError> {
        if depth > 64 {
            return Err(DocError::Malformed("pdf: nesting"));
        }
        match self.next_tok() {
            Tok::Obj(Obj::Int(n)) => {
                // Lookahead for "g R".
                let save = self.pos;
                if let Tok::Obj(Obj::Int(g)) = self.next_tok() {
                    let save2 = self.pos;
                    if let Tok::Kw(k) = self.next_tok() {
                        if k == "R" && n >= 0 && g >= 0 {
                            return Ok(Obj::Ref(n as u32, g as u16));
                        }
                    }
                    self.pos = save2;
                    let _ = g;
                }
                self.pos = save;
                Ok(Obj::Int(n))
            }
            Tok::Obj(o) => Ok(o),
            Tok::ArrOpen => {
                let mut v = Vec::new();
                loop {
                    self.skip_ws();
                    if self.pos >= self.b.len() {
                        break;
                    }
                    if self.b[self.pos] == b']' {
                        self.pos += 1;
                        break;
                    }
                    match self.parse_obj(depth + 1) {
                        Ok(o) => v.push(o),
                        Err(_) => break,
                    }
                    if v.len() > 100_000 {
                        return Err(DocError::TooLarge("pdf array"));
                    }
                }
                Ok(Obj::Array(v))
            }
            Tok::DictOpen => {
                let mut d: Vec<(String, Obj)> = Vec::new();
                loop {
                    match self.next_tok() {
                        Tok::DictClose | Tok::Eof => break,
                        Tok::Obj(Obj::Name(k)) => {
                            let v = self.parse_obj(depth + 1)?;
                            d.push((k, v));
                        }
                        _ => {}
                    }
                    if d.len() > 10_000 {
                        return Err(DocError::TooLarge("pdf dict"));
                    }
                }
                // A stream?
                let save = self.pos;
                if let Tok::Kw(k) = self.next_tok() {
                    if k == "stream" {
                        // After 'stream': CRLF or LF.
                        if self.b.get(self.pos) == Some(&b'\r') {
                            self.pos += 1;
                        }
                        if self.b.get(self.pos) == Some(&b'\n') {
                            self.pos += 1;
                        }
                        let offset = self.base + self.pos as u64;
                        let len = d.iter().find(|(k, _)| k == "Length").map(|(_, v)| v.clone());
                        // Length may be indirect; resolved by the document. Store a marker (-1)
                        // by keeping len = u64::MAX and letting Document fix it up.
                        let l = match len {
                            Some(Obj::Int(n)) if n >= 0 => n as u64,
                            _ => u64::MAX,
                        };
                        return Ok(Obj::Stream { dict: d, offset, len: l });
                    }
                }
                self.pos = save;
                Ok(Obj::Dict(d))
            }
            Tok::Kw(k) => Ok(Obj::Name(alloc::format!("__kw_{k}"))),
            Tok::ArrClose | Tok::DictClose => Ok(Obj::Null),
            Tok::Eof => Err(DocError::Malformed("pdf: unexpected end")),
        }
    }
}

fn parse_real(s: &str) -> f32 {
    // Simple decimal parser (no_std has no f32::from_str? it does; but be lenient with "--5" etc.)
    let s = s.trim_start_matches('+');
    let neg = s.starts_with('-');
    let s = s.trim_start_matches('-');
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    let mut v = 0f32;
    for c in int.bytes() {
        if c.is_ascii_digit() {
            v = v * 10.0 + (c - b'0') as f32;
        } else {
            break;
        }
    }
    let mut scale = 0.1f32;
    for c in frac.bytes() {
        if c.is_ascii_digit() {
            v += (c - b'0') as f32 * scale;
            scale *= 0.1;
        } else {
            break;
        }
    }
    if neg {
        -v
    } else {
        v
    }
}

// ---------------------------------------------------------------------------------------
// Cross-reference and document.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Loc {
    Free,
    Offset(u64),
    InStream(u32, u32),
}

/// A page: its object number (for destinations) and its dictionary with inherited attributes.
#[derive(Clone, Debug)]
pub struct Page {
    /// Object number, when the page was reached through a reference.
    pub num: Option<u32>,
    /// The page dictionary.
    pub dict: Obj,
}

/// A parsed object stream: (stream number, offsets table, data).
type ObjStm = (u32, Vec<(u32, usize)>, Vec<u8>);
/// An object stream's offsets table and data.
type ObjStmData = (Vec<(u32, usize)>, Vec<u8>);

/// An open document.
pub struct Document<'a, R: ReadAt> {
    src: &'a R,
    xref: Vec<Loc>,
    /// Trailer entries merged across the /Prev chain (first wins).
    pub trailer: Vec<(String, Obj)>,
    /// Cache of parsed object streams.
    objstm_cache: Vec<ObjStm>,
    reconstructed: bool,
}

impl<'a, R: ReadAt> Document<'a, R> {
    /// Open and parse the cross-reference data.
    pub fn open(src: &'a R) -> Result<Self, DocError> {
        let len = src.len();
        let mut head = [0u8; 8];
        let n = src.read_at(0, &mut head)?;
        if !head[..n].starts_with(b"%PDF") {
            return Err(DocError::Malformed("not a pdf"));
        }
        let mut doc = Document { src, xref: Vec::new(), trailer: Vec::new(), objstm_cache: Vec::new(), reconstructed: false };
        // startxref in the last 2 KB.
        let tail_len = len.min(2048) as usize;
        let tail = src.read_range(len - tail_len as u64, tail_len)?;
        let mut start: Option<u64> = None;
        if let Some(i) = rfind(&tail, b"startxref") {
            let mut lx = Lexer::new(&tail[i + 9..], 0);
            if let Tok::Obj(Obj::Int(off)) = lx.next_tok() {
                start = Some(off as u64);
            }
        }
        let mut ok = false;
        if let Some(s) = start {
            ok = doc.load_xref_chain(s).is_ok();
        }
        if !ok || doc.trailer.iter().all(|(k, _)| k != "Root") {
            doc.reconstruct()?;
        }
        if doc.trailer.iter().any(|(k, _)| k == "Encrypt") {
            return Err(DocError::Drm);
        }
        Ok(doc)
    }

    fn load_xref_chain(&mut self, start: u64) -> Result<(), DocError> {
        let mut next = Some(start);
        let mut seen = 0;
        let mut visited: Vec<u64> = Vec::new();
        while let Some(off) = next {
            if visited.contains(&off) || seen > 64 {
                break;
            }
            visited.push(off);
            seen += 1;
            next = self.load_xref_at(off)?;
        }
        Ok(())
    }

    fn set(&mut self, num: u32, loc: Loc) {
        if num as usize > 5_000_000 {
            return;
        }
        if self.xref.len() <= num as usize {
            self.xref.resize(num as usize + 1, Loc::Free);
        }
        // First definition wins (newest xref section is loaded first).
        if self.xref[num as usize] == Loc::Free {
            self.xref[num as usize] = loc;
        }
    }

    /// Load one xref section (table or stream); returns /Prev.
    fn load_xref_at(&mut self, off: u64) -> Result<Option<u64>, DocError> {
        if off >= self.src.len() {
            return Err(DocError::Malformed("pdf: xref offset"));
        }
        let chunk_len = (self.src.len() - off).min(OBJ_LIMIT as u64) as usize;
        let buf = self.src.read_range(off, chunk_len)?;
        let mut lx = Lexer::new(&buf, off);
        lx.skip_ws();
        if buf[lx.pos..].starts_with(b"xref") {
            lx.pos += 4;
            // Sections: "start count" then entries of 20 bytes (tolerate 19).
            loop {
                lx.skip_ws();
                if buf[lx.pos..].starts_with(b"trailer") {
                    lx.pos += 7;
                    let t = lx.parse_obj(0)?;
                    let mut prev = None;
                    let mut xrefstm = None;
                    if let Obj::Dict(d) = t {
                        for (k, v) in d {
                            if k == "Prev" {
                                prev = v.int().map(|x| x as u64);
                            } else if k == "XRefStm" {
                                xrefstm = v.int().map(|x| x as u64);
                            }
                            if !self.trailer.iter().any(|(kk, _)| *kk == k) {
                                self.trailer.push((k, v));
                            }
                        }
                    }
                    if let Some(x) = xrefstm {
                        let _ = self.load_xref_at(x);
                    }
                    return Ok(prev);
                }
                let (start, count) = match (lx.next_tok(), lx.next_tok()) {
                    (Tok::Obj(Obj::Int(s)), Tok::Obj(Obj::Int(c))) if s >= 0 && c >= 0 => (s as u32, c as u32),
                    _ => return Err(DocError::Malformed("pdf: xref section")),
                };
                lx.skip_ws();
                for i in 0..count {
                    lx.skip_ws();
                    if lx.pos + 18 > buf.len() {
                        return Err(DocError::Malformed("pdf: xref truncated"));
                    }
                    let e = &buf[lx.pos..lx.pos + 18];
                    let o = core::str::from_utf8(&e[..10]).ok().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
                    let ty = e[17];
                    lx.pos += 18;
                    if ty == b'n' {
                        self.set(start + i, Loc::Offset(o));
                    } else {
                        self.set(start + i, Loc::Free);
                        if self.xref.len() > (start + i) as usize && self.xref[(start + i) as usize] == Loc::Free {
                            // keep free
                        }
                    }
                }
            }
        }
        // Cross-reference stream: "n g obj <<...>> stream".
        let obj = self.parse_indirect_at(off)?;
        let (dict, data) = match &obj {
            Obj::Stream { dict, .. } => (dict.clone(), self.stream_data(&obj)?),
            _ => return Err(DocError::Malformed("pdf: xref stream")),
        };
        let w: Vec<usize> = dict
            .iter()
            .find(|(k, _)| k == "W")
            .and_then(|(_, v)| v.array())
            .map(|a| a.iter().map(|x| x.int().unwrap_or(0) as usize).collect())
            .unwrap_or_default();
        if w.len() < 3 {
            return Err(DocError::Malformed("pdf: xref /W"));
        }
        let size = dict.iter().find(|(k, _)| k == "Size").and_then(|(_, v)| v.int()).unwrap_or(0) as u32;
        let index: Vec<u32> = dict
            .iter()
            .find(|(k, _)| k == "Index")
            .and_then(|(_, v)| v.array())
            .map(|a| a.iter().map(|x| x.int().unwrap_or(0) as u32).collect())
            .unwrap_or_else(|| alloc::vec![0, size]);
        let row = w[0] + w[1] + w[2];
        let mut p = 0usize;
        for pair in index.chunks(2) {
            let (start, count) = (pair[0], *pair.get(1).unwrap_or(&0));
            for i in 0..count {
                if p + row > data.len() {
                    break;
                }
                let rd = |off: usize, n: usize| -> u64 {
                    let mut v = 0u64;
                    for k in 0..n {
                        v = (v << 8) | data[off + k] as u64;
                    }
                    v
                };
                let ty = if w[0] == 0 { 1 } else { rd(p, w[0]) };
                let f2 = rd(p + w[0], w[1]);
                let f3 = rd(p + w[0] + w[1], w[2]);
                p += row;
                match ty {
                    1 => self.set(start + i, Loc::Offset(f2)),
                    2 => self.set(start + i, Loc::InStream(f2 as u32, f3 as u32)),
                    _ => self.set(start + i, Loc::Free),
                }
            }
        }
        let mut prev = None;
        for (k, v) in dict {
            if k == "Prev" {
                prev = v.int().map(|x| x as u64);
            }
            if !self.trailer.iter().any(|(kk, _)| *kk == k) {
                self.trailer.push((k, v));
            }
        }
        Ok(prev)
    }

    /// Rebuild the xref by scanning for "N G obj" and "trailer" — the recovery path for
    /// broken files, which are common.
    fn reconstruct(&mut self) -> Result<(), DocError> {
        self.reconstructed = true;
        self.xref.clear();
        let len = self.src.len();
        let mut pos = 0u64;
        let mut buf = alloc::vec![0u8; 64 * 1024];
        let mut carry: Vec<u8> = Vec::new();
        let mut found_root = false;
        while pos < len {
            let n = self.src.read_at(pos, &mut buf)?;
            if n == 0 {
                break;
            }
            let mut data = core::mem::take(&mut carry);
            let carry_len = data.len();
            data.extend_from_slice(&buf[..n]);
            let base = pos - carry_len as u64;
            let mut i = 0usize;
            while i + 3 < data.len() {
                if &data[i..i + 3] == b"obj" && (i == 0 || is_ws(data[i - 1]) || data[i - 1].is_ascii_digit()) {
                    // Walk back over "N G ".
                    let mut j = i;
                    while j > 0 && is_ws(data[j - 1]) {
                        j -= 1;
                    }
                    let ge = j;
                    while j > 0 && data[j - 1].is_ascii_digit() {
                        j -= 1;
                    }
                    let gs = j;
                    while j > 0 && is_ws(data[j - 1]) {
                        j -= 1;
                    }
                    let ne = j;
                    while j > 0 && data[j - 1].is_ascii_digit() {
                        j -= 1;
                    }
                    let ns = j;
                    if ns < ne && gs < ge && (ns == 0 || !data[ns - 1].is_ascii_alphanumeric()) {
                        if let Ok(num) = core::str::from_utf8(&data[ns..ne]).unwrap_or("x").parse::<u32>() {
                            let off = base + ns as u64;
                            if (num as usize) < 5_000_000 {
                                if self.xref.len() <= num as usize {
                                    self.xref.resize(num as usize + 1, Loc::Free);
                                }
                                self.xref[num as usize] = Loc::Offset(off); // later definitions win
                            }
                        }
                    }
                    i += 3;
                    continue;
                }
                if &data[i..i + 3] == b"tra" && data[i..].starts_with(b"trailer") {
                    let mut lx = Lexer::new(&data[i + 7..], base + i as u64 + 7);
                    if let Ok(Obj::Dict(d)) = lx.parse_obj(0) {
                        for (k, v) in d {
                            if k == "Root" {
                                found_root = true;
                            }
                            // later trailers win
                            self.trailer.retain(|(kk, _)| *kk != k);
                            self.trailer.push((k, v));
                        }
                    }
                }
                i += 1;
            }
            // Keep a tail so patterns spanning chunks are found.
            let keep = data.len().min(64);
            carry = data[data.len() - keep..].to_vec();
            pos += n as u64;
        }
        if !found_root {
            // Look for an object with /Type /Catalog, or an xref stream trailer dict.
            for num in 0..self.xref.len() {
                if let Loc::Offset(_) = self.xref[num] {
                    if let Ok(o) = self.get(num as u32) {
                        if o.get("Type").and_then(|t| t.name()) == Some("Catalog") {
                            self.trailer.push(("Root".into(), Obj::Ref(num as u32, 0)));
                            found_root = true;
                            break;
                        }
                        if o.get("Type").and_then(|t| t.name()) == Some("XRef") {
                            if let Some(r) = o.get("Root") {
                                self.trailer.push(("Root".into(), r.clone()));
                                found_root = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        // Objects inside object streams: register them so lookups succeed.
        let nums: Vec<u32> = (0..self.xref.len() as u32).collect();
        for num in nums {
            if let Loc::Offset(_) = self.xref[num as usize] {
                if let Ok(o) = self.get(num) {
                    if o.get("Type").and_then(|t| t.name()) == Some("ObjStm") {
                        if let Ok((table, _)) = self.load_objstm(num) {
                            for (idx, (onum, _)) in table.iter().enumerate() {
                                let on = *onum as usize;
                                if on < 5_000_000 {
                                    if self.xref.len() <= on {
                                        self.xref.resize(on + 1, Loc::Free);
                                    }
                                    if self.xref[on] == Loc::Free {
                                        self.xref[on] = Loc::InStream(num, idx as u32);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !found_root {
            return Err(DocError::Malformed("pdf: no catalog"));
        }
        Ok(())
    }

    /// Parse "n g obj ... endobj" at an offset.
    fn parse_indirect_at(&mut self, off: u64) -> Result<Obj, DocError> {
        if off >= self.src.len() {
            return Err(DocError::Malformed("pdf: object offset"));
        }
        let n = (self.src.len() - off).min(OBJ_LIMIT as u64) as usize;
        let buf = self.src.read_range(off, n)?;
        let mut lx = Lexer::new(&buf, off);
        // n g obj
        match (lx.next_tok(), lx.next_tok(), lx.next_tok()) {
            (Tok::Obj(Obj::Int(_)), Tok::Obj(Obj::Int(_)), Tok::Kw(k)) if k == "obj" => {}
            _ => return Err(DocError::Malformed("pdf: expected obj")),
        }
        let mut o = lx.parse_obj(0)?;
        if let Obj::Stream { dict, offset, len } = &mut o {
            if *len == u64::MAX {
                // Indirect /Length: resolve, else scan for "endstream".
                let l = dict.iter().find(|(k, _)| k == "Length").map(|(_, v)| v.clone());
                let resolved = match l {
                    Some(Obj::Ref(n, _)) => self.get(n).ok().and_then(|o| o.int()).map(|x| x as u64),
                    _ => None,
                };
                *len = match resolved {
                    Some(v) => v,
                    None => {
                        let start = (*offset - off) as usize;
                        find(&buf[start..], b"endstream").map(|e| e as u64).unwrap_or((buf.len() - start) as u64)
                    }
                };
                // Trim a trailing EOL before endstream when the length overshoots.
            }
        }
        Ok(o)
    }

    fn load_objstm(&mut self, num: u32) -> Result<ObjStmData, DocError> {
        if let Some((_, t, d)) = self.objstm_cache.iter().find(|(n, _, _)| *n == num) {
            return Ok((t.clone(), d.clone()));
        }
        let s = self.get(num)?;
        let n = s.get("N").and_then(|v| v.int()).unwrap_or(0) as usize;
        let first = s.get("First").and_then(|v| v.int()).unwrap_or(0) as usize;
        let data = self.stream_data(&s)?;
        let mut lx = Lexer::new(&data[..first.min(data.len())], 0);
        let mut table = Vec::with_capacity(n.min(10_000));
        for _ in 0..n {
            match (lx.next_tok(), lx.next_tok()) {
                (Tok::Obj(Obj::Int(on)), Tok::Obj(Obj::Int(off))) => table.push((on as u32, first + off as usize)),
                _ => break,
            }
        }
        if self.objstm_cache.len() >= 4 {
            self.objstm_cache.remove(0);
        }
        self.objstm_cache.push((num, table.clone(), data.clone()));
        Ok((table, data))
    }

    /// The underlying reader.
    pub fn source(&self) -> &'a R {
        self.src
    }

    /// Fetch an object by number, resolving object streams.
    pub fn get(&mut self, num: u32) -> Result<Obj, DocError> {
        let loc = self.xref.get(num as usize).copied().unwrap_or(Loc::Free);
        match loc {
            Loc::Free => {
                if !self.reconstructed {
                    self.reconstruct()?;
                    return self.get(num);
                }
                Ok(Obj::Null)
            }
            Loc::Offset(off) => match self.parse_indirect_at(off) {
                Ok(o) => Ok(o),
                Err(e) => {
                    if !self.reconstructed {
                        self.reconstruct()?;
                        self.get(num)
                    } else {
                        Err(e)
                    }
                }
            },
            Loc::InStream(snum, idx) => {
                let (table, data) = self.load_objstm(snum)?;
                let Some(&(_, off)) = table.get(idx as usize) else { return Ok(Obj::Null) };
                let mut lx = Lexer::new(&data[off.min(data.len())..], 0);
                lx.parse_obj(0)
            }
        }
    }

    /// Follow references until a direct object.
    pub fn resolve(&mut self, o: &Obj) -> Result<Obj, DocError> {
        let mut cur = o.clone();
        for _ in 0..32 {
            match cur {
                Obj::Ref(n, _) => cur = self.get(n)?,
                other => return Ok(other),
            }
        }
        Ok(Obj::Null)
    }

    /// `dict[key]`, resolved.
    pub fn get_key(&mut self, dict: &Obj, key: &str) -> Result<Obj, DocError> {
        match dict.get(key) {
            Some(v) => self.resolve(v),
            None => Ok(Obj::Null),
        }
    }

    /// Decode a stream fully (bounded), applying its filters.
    pub fn stream_data(&mut self, s: &Obj) -> Result<Vec<u8>, DocError> {
        let Obj::Stream { dict, offset, len } = s else { return Err(DocError::Malformed("pdf: not a stream")) };
        let dict_obj = Obj::Dict(dict.clone());
        let filters = self.filters_of(&dict_obj)?;
        let raw_len = (*len).min(self.src.len().saturating_sub(*offset));
        let mut data: Vec<u8> = Vec::new();
        let mut first = true;
        if filters.is_empty() {
            data = quire_fs::Slice::new(self.src, *offset, raw_len).read_range(0, raw_len.min(STREAM_LIMIT as u64) as usize)?;
        }
        for (name, parms) in &filters {
            let out = match name.as_str() {
                "FlateDecode" | "Fl" => {
                    if first {
                        Inflater::new(quire_fs::Slice::new(self.src, *offset, raw_len), Framing::Zlib).read_all(STREAM_LIMIT)?
                    } else {
                        Inflater::new(&data, Framing::Zlib).read_all(STREAM_LIMIT)?
                    }
                }
                "ASCIIHexDecode" | "AHx" => {
                    let input = if first { self.src.read_range(*offset, raw_len as usize)? } else { core::mem::take(&mut data) };
                    ascii_hex(&input)
                }
                "ASCII85Decode" | "A85" => {
                    let input = if first { self.src.read_range(*offset, raw_len as usize)? } else { core::mem::take(&mut data) };
                    ascii85(&input)
                }
                "RunLengthDecode" | "RL" => {
                    let input = if first { self.src.read_range(*offset, raw_len as usize)? } else { core::mem::take(&mut data) };
                    runlength(&input)
                }
                "DCTDecode" | "DCT" | "JPXDecode" | "CCITTFaxDecode" | "CCF" | "JBIG2Decode" => {
                    // Image filters: leave the bytes encoded; the image path handles them.
                    if first {
                        self.src.read_range(*offset, raw_len.min(STREAM_LIMIT as u64 * 8) as usize)?
                    } else {
                        core::mem::take(&mut data)
                    }
                }
                "LZWDecode" | "LZW" => {
                    let input = if first { self.src.read_range(*offset, raw_len as usize)? } else { core::mem::take(&mut data) };
                    let early = parms.as_ref().and_then(|p| p.get("EarlyChange")).and_then(|v| v.int()).unwrap_or(1) != 0;
                    lzw(&input, early)
                }
                "Crypt" => {
                    if first {
                        self.src.read_range(*offset, raw_len as usize)?
                    } else {
                        core::mem::take(&mut data)
                    }
                }
                _ => return Err(DocError::Unsupported("pdf filter")),
            };
            first = false;
            data = out;
            // PNG / TIFF predictors.
            if let Some(p) = parms {
                let pred = p.get("Predictor").and_then(|v| v.int()).unwrap_or(1);
                if pred >= 10 {
                    let colors = p.get("Colors").and_then(|v| v.int()).unwrap_or(1) as usize;
                    let bpc = p.get("BitsPerComponent").and_then(|v| v.int()).unwrap_or(8) as usize;
                    let columns = p.get("Columns").and_then(|v| v.int()).unwrap_or(1) as usize;
                    data = png_predictor(&data, colors, bpc, columns);
                } else if pred == 2 {
                    let colors = p.get("Colors").and_then(|v| v.int()).unwrap_or(1) as usize;
                    let columns = p.get("Columns").and_then(|v| v.int()).unwrap_or(1) as usize;
                    data = tiff_predictor(&data, colors, columns);
                }
            }
        }
        Ok(data)
    }

    /// The (filter, parms) list of a stream dictionary, resolved.
    pub fn filters_of(&mut self, dict: &Obj) -> Result<Vec<(String, Option<Obj>)>, DocError> {
        let f = self.get_key(dict, "Filter")?;
        let p = self.get_key(dict, "DecodeParms")?;
        let mut out = Vec::new();
        match f {
            Obj::Name(n) => out.push((n, if p == Obj::Null { None } else { Some(p) })),
            Obj::Array(a) => {
                let parms: Vec<Obj> = match &p {
                    Obj::Array(pa) => pa.clone(),
                    Obj::Null => Vec::new(),
                    other => alloc::vec![other.clone()],
                };
                for (i, x) in a.iter().enumerate() {
                    let n = self.resolve(x)?;
                    if let Obj::Name(n) = n {
                        let pp = parms.get(i).cloned();
                        let pp = match pp {
                            Some(o) => Some(self.resolve(&o)?).filter(|o| *o != Obj::Null),
                            None => None,
                        };
                        out.push((n, pp));
                    }
                }
            }
            _ => {}
        }
        Ok(out)
    }

    /// Whether the first filter of a stream is an image codec, and which.
    pub fn image_filter(&mut self, dict: &Obj) -> Result<Option<String>, DocError> {
        let f = self.filters_of(dict)?;
        Ok(f.iter()
            .map(|(n, _)| n.clone())
            .find(|n| matches!(n.as_str(), "DCTDecode" | "DCT" | "JPXDecode" | "CCITTFaxDecode" | "CCF" | "JBIG2Decode")))
    }

    /// The catalog.
    pub fn catalog(&mut self) -> Result<Obj, DocError> {
        let r = self.trailer.iter().find(|(k, _)| k == "Root").map(|(_, v)| v.clone()).ok_or(DocError::Malformed("pdf: no root"))?;
        self.resolve(&r)
    }

    /// All pages in order, each with inherited attributes merged in.
    pub fn pages(&mut self) -> Result<Vec<Page>, DocError> {
        let cat = self.catalog()?;
        let root_ref = cat.get("Pages").cloned().unwrap_or(Obj::Null);
        let root = self.resolve(&root_ref)?;
        let mut out = Vec::new();
        let inherited: Vec<(String, Obj)> = Vec::new();
        let root_num = if let Obj::Ref(n, _) = root_ref { Some(n) } else { None };
        self.walk_pages(root_num, &root, &inherited, &mut out, 0)?;
        if out.is_empty() {
            // Reconstruct: any object of /Type /Page.
            let n = self.xref.len();
            for num in 0..n as u32 {
                if let Ok(o) = self.get(num) {
                    if o.get("Type").and_then(|t| t.name()) == Some("Page") {
                        out.push(Page { num: Some(num), dict: o });
                    }
                }
                if out.len() > 5000 {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn walk_pages(
        &mut self,
        num: Option<u32>,
        node: &Obj,
        inherited: &[(String, Obj)],
        out: &mut Vec<Page>,
        depth: u32,
    ) -> Result<(), DocError> {
        if depth > 32 || out.len() > 5000 {
            return Ok(());
        }
        let ty = node.get("Type").and_then(|t| t.name()).unwrap_or("");
        let mut inh: Vec<(String, Obj)> = inherited.to_vec();
        for key in ["Resources", "MediaBox", "CropBox", "Rotate"] {
            if let Some(v) = node.get(key) {
                inh.retain(|(k, _)| k != key);
                inh.push((key.into(), v.clone()));
            }
        }
        let kids = self.get_key(node, "Kids")?;
        if ty == "Page" || (kids == Obj::Null && ty != "Pages") {
            let mut d: Vec<(String, Obj)> = node.dict().map(|x| x.to_vec()).unwrap_or_default();
            for (k, v) in inh {
                if !d.iter().any(|(kk, _)| *kk == k) {
                    d.push((k, v));
                }
            }
            out.push(Page { num, dict: Obj::Dict(d) });
            return Ok(());
        }
        if let Obj::Array(a) = kids {
            for k in a {
                let child = self.resolve(&k)?;
                let knum = if let Obj::Ref(n, _) = k { Some(n) } else { None };
                if child != Obj::Null {
                    self.walk_pages(knum, &child, &inh, out, depth + 1)?;
                }
            }
        }
        Ok(())
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}
fn rfind(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).rposition(|w| w == n)
}

fn ascii_hex(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() / 2);
    let mut hi = None;
    for &c in input {
        if c == b'>' {
            break;
        }
        let v = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => continue,
        };
        match hi {
            None => hi = Some(v),
            Some(h) => {
                out.push(h * 16 + v);
                hi = None;
            }
        }
    }
    if let Some(h) = hi {
        out.push(h * 16);
    }
    out
}

fn ascii85(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 4 / 5);
    let mut tuple = [0u8; 5];
    let mut n = 0;
    let mut i = 0;
    if input.starts_with(b"<~") {
        i = 2;
    }
    while i < input.len() {
        let c = input[i];
        i += 1;
        if c == b'~' {
            break;
        }
        if is_ws(c) {
            continue;
        }
        if c == b'z' && n == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&c) {
            continue;
        }
        tuple[n] = c - b'!';
        n += 1;
        if n == 5 {
            let mut v = 0u32;
            for t in tuple {
                v = v.wrapping_mul(85).wrapping_add(t as u32);
            }
            out.extend_from_slice(&v.to_be_bytes());
            n = 0;
        }
    }
    if n > 0 {
        for t in tuple.iter_mut().skip(n) {
            *t = 84;
        }
        let mut v = 0u32;
        for t in tuple {
            v = v.wrapping_mul(85).wrapping_add(t as u32);
        }
        let b = v.to_be_bytes();
        out.extend_from_slice(&b[..n - 1]);
    }
    out
}

fn runlength(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < input.len() {
        let l = input[i] as usize;
        i += 1;
        if l == 128 {
            break;
        }
        if l < 128 {
            let end = (i + l + 1).min(input.len());
            out.extend_from_slice(&input[i..end]);
            i = end;
        } else {
            if let Some(&b) = input.get(i) {
                for _ in 0..(257 - l) {
                    out.push(b);
                }
            }
            i += 1;
        }
    }
    out
}

fn lzw(input: &[u8], early: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut dict: Vec<Vec<u8>> = (0..256).map(|i| alloc::vec![i as u8]).collect();
    dict.push(Vec::new());
    dict.push(Vec::new());
    let mut code_len = 9u32;
    let mut prev: Option<Vec<u8>> = None;
    let mut bitpos = 0usize;
    let total_bits = input.len() * 8;
    while bitpos + code_len as usize <= total_bits {
        let mut code = 0u32;
        for _ in 0..code_len {
            let byte = input[bitpos / 8];
            let bit = (byte >> (7 - (bitpos % 8))) & 1;
            code = (code << 1) | bit as u32;
            bitpos += 1;
        }
        match code {
            256 => {
                dict.truncate(258);
                code_len = 9;
                prev = None;
            }
            257 => break,
            _ => {
                let entry = if (code as usize) < dict.len() {
                    dict[code as usize].clone()
                } else if let Some(p) = &prev {
                    let mut e = p.clone();
                    e.push(p[0]);
                    e
                } else {
                    break;
                };
                out.extend_from_slice(&entry);
                if let Some(p) = prev {
                    let mut ne = p;
                    ne.push(entry[0]);
                    dict.push(ne);
                }
                prev = Some(entry);
                let limit = if early { 1 } else { 0 };
                if dict.len() + limit >= (1 << code_len) && code_len < 12 {
                    code_len += 1;
                }
                if out.len() > STREAM_LIMIT {
                    break;
                }
            }
        }
    }
    out
}

fn png_predictor(data: &[u8], colors: usize, bpc: usize, columns: usize) -> Vec<u8> {
    let bpp = (colors * bpc).div_ceil(8).max(1);
    let row_len = (columns * colors * bpc).div_ceil(8);
    let mut out = Vec::with_capacity(data.len());
    let mut prev = alloc::vec![0u8; row_len];
    let mut i = 0;
    while i < data.len() {
        let ft = data[i];
        i += 1;
        let end = (i + row_len).min(data.len());
        let mut row = data[i..end].to_vec();
        row.resize(row_len, 0);
        i = end;
        match ft {
            1 => {
                for k in bpp..row_len {
                    row[k] = row[k].wrapping_add(row[k - bpp]);
                }
            }
            2 => {
                for k in 0..row_len {
                    row[k] = row[k].wrapping_add(prev[k]);
                }
            }
            3 => {
                for k in 0..row_len {
                    let a = if k >= bpp { row[k - bpp] as u16 } else { 0 };
                    row[k] = row[k].wrapping_add(((a + prev[k] as u16) / 2) as u8);
                }
            }
            4 => {
                for k in 0..row_len {
                    let a = if k >= bpp { row[k - bpp] as i16 } else { 0 };
                    let b = prev[k] as i16;
                    let c = if k >= bpp { prev[k - bpp] as i16 } else { 0 };
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    let pred = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    row[k] = row[k].wrapping_add(pred as u8);
                }
            }
            _ => {}
        }
        out.extend_from_slice(&row);
        prev = row;
        if i >= data.len() {
            break;
        }
    }
    out
}

fn tiff_predictor(data: &[u8], colors: usize, columns: usize) -> Vec<u8> {
    let row_len = columns * colors;
    let mut out = data.to_vec();
    for row in out.chunks_mut(row_len.max(1)) {
        for k in colors..row.len() {
            row[k] = row[k].wrapping_add(row[k - colors]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexer_objects() {
        let src = b"<< /Type /Page /Kids [1 0 R 2 0 R] /Count 2 /Name /A#20B /S (hi\\)there) /H <414243> /R 1.5 /N -3 >>";
        let mut lx = Lexer::new(src, 0);
        let o = lx.parse_obj(0).unwrap();
        assert_eq!(o.get("Type").and_then(|t| t.name()), Some("Page"));
        assert_eq!(o.get("Kids").and_then(|k| k.array()).map(|a| a.len()), Some(2));
        assert_eq!(o.get("Kids").and_then(|k| k.array()).map(|a| a[1].clone()), Some(Obj::Ref(2, 0)));
        assert_eq!(o.get("Name").and_then(|t| t.name()), Some("A B"));
        assert_eq!(o.get("S"), Some(&Obj::Str(b"hi)there".to_vec())));
        assert_eq!(o.get("H"), Some(&Obj::Str(b"ABC".to_vec())));
        assert_eq!(o.get("R").and_then(|r| r.num()), Some(1.5));
        assert_eq!(o.get("N").and_then(|r| r.int()), Some(-3));
    }

    #[test]
    fn filters() {
        assert_eq!(ascii_hex(b"48656C6C6F>"), b"Hello");
        assert_eq!(ascii85(b"87cURD]i,\"Ebo80~>"), b"Hello World!");
        assert_eq!(runlength(&[2, b'a', b'b', b'c', 254, b'x', 128]), b"abcxxx");
        let pred = png_predictor(&[2, 1, 2, 2, 1, 1], 1, 8, 2);
        assert_eq!(pred, alloc::vec![1, 2, 2, 3]);
    }

    #[test]
    fn opens_all_fixtures_and_finds_pages() {
        for (name, bytes, pages) in [
            ("basicapi", &include_bytes!("../fixtures/basicapi.pdf")[..], 3usize),
            ("tracemonkey", &include_bytes!("../fixtures/tracemonkey.pdf")[..], 14),
            ("alphatrans", &include_bytes!("../fixtures/alphatrans.pdf")[..], 1),
            ("issue1002", &include_bytes!("../fixtures/issue1002.pdf")[..], 1),
        ] {
            let mut doc = Document::open(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            let p = doc.pages().unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(p.len(), pages, "{name} page count");
            let cat = doc.catalog().unwrap();
            assert_eq!(cat.get("Type").and_then(|t| t.name()), Some("Catalog"), "{name}");
        }
    }

    #[test]
    fn broken_xref_is_reconstructed() {
        let mut bytes = include_bytes!("../fixtures/basicapi.pdf").to_vec();
        // Corrupt the startxref offset.
        if let Some(i) = rfind(&bytes, b"startxref") {
            for b in &mut bytes[i + 10..i + 14] {
                if b.is_ascii_digit() {
                    *b = b'9';
                }
            }
        }
        let mut doc = Document::open(&bytes).expect("reconstructed");
        assert_eq!(doc.pages().unwrap().len(), 3);
    }

    #[test]
    fn encrypted_is_refused() {
        let pdf = b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [] /Count 0 >> endobj\ntrailer << /Root 1 0 R /Encrypt 3 0 R >>\n%%EOF";
        assert!(matches!(Document::open(&&pdf[..]), Err(DocError::Drm)));
    }
}

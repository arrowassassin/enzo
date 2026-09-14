//! PDF on the device: objects, cross-reference tables and streams, object streams,
//! filters, the page tree, and text extraction with font encodings and ToUnicode maps.
//! Text PDFs are reflowed into QTX; scanned pages become image pages. Encrypted files
//! are refused like DRM. Everything is bounded so a hostile file cannot exhaust RAM.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;

use crate::inflate::{Framing, Inflater};
use crate::DocError;

mod text;
pub use text::ingest;

/// Largest inflated stream we hold in memory (content streams, object streams, ToUnicode).
#[cfg(target_os = "none")]
pub(crate) const STREAM_LIMIT: usize = 96 * 1024;
/// Largest inflated stream we hold in memory on the host.
#[cfg(not(target_os = "none"))]
pub(crate) const STREAM_LIMIT: usize = 2 * 1024 * 1024;
/// Largest object we parse.
#[cfg(target_os = "none")]
const OBJ_LIMIT: usize = 64 * 1024;
/// Largest object we parse on the host.
#[cfg(not(target_os = "none"))]
const OBJ_LIMIT: usize = 512 * 1024;
/// First read for an object; grown ×4 up to `OBJ_LIMIT` when the object runs past it.
/// Most objects are under 1 KB, and every card read below 512 B costs the same as one of
/// 512 B, so 2 KB fetches the typical object in one go.
const OBJ_FIRST_READ: usize = 2048;
/// First read for a cross-reference table.
const XREF_FIRST_READ: usize = 8 * 1024;
/// Parsed objects kept across lookups (resources, fonts, page nodes are hit repeatedly).
const OBJ_CACHE: usize = 16;
/// Largest parsed object we cache, as a rough weight (bytes of strings plus nodes).
const OBJ_CACHE_WEIGHT: usize = 3072;
/// Parsed object streams kept in memory.
#[cfg(target_os = "none")]
const OBJSTM_CACHE: usize = 1;
/// Parsed object streams kept in memory on the host.
#[cfg(not(target_os = "none"))]
const OBJSTM_CACHE: usize = 4;
/// Chunk size for the reconstruction scan.
const SCAN_CHUNK: usize = 32 * 1024;
/// Most pages we list.
#[cfg(target_os = "none")]
const PAGE_LIMIT: usize = 2000;
/// Most pages we list on the host.
#[cfg(not(target_os = "none"))]
const PAGE_LIMIT: usize = 5000;

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
    /// Rough memory weight, stopping early once `budget` is exhausted.
    fn weigh(&self, budget: &mut usize) -> bool {
        let cost = match self {
            Obj::Str(s) => 24 + s.len(),
            Obj::Name(n) => 24 + n.len(),
            Obj::Array(_) | Obj::Dict(_) | Obj::Stream { .. } => 32,
            _ => 8,
        };
        if *budget < cost {
            return false;
        }
        *budget -= cost;
        match self {
            Obj::Array(a) => a.iter().all(|o| o.weigh(budget)),
            Obj::Dict(d) | Obj::Stream { dict: d, .. } => d.iter().all(|(k, v)| {
                if *budget < k.len() + 24 {
                    return false;
                }
                *budget -= k.len() + 24;
                v.weigh(budget)
            }),
            _ => true,
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

/// A lexer token. Keywords borrow the input: content streams have tens of thousands of
/// operators and must not allocate one string each.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok<'a> {
    /// Any object that is not an operator/keyword.
    Obj(Obj),
    /// A bare keyword: `obj`, `endobj`, `stream`, `R`, operators in content streams…
    Kw(&'a str),
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
    pub fn next_tok(&mut self) -> Tok<'a> {
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
                let raw = &self.b[start..self.pos];
                // Keywords are ASCII; binary junk is cut at the first invalid byte.
                let kw = match core::str::from_utf8(raw) {
                    Ok(s) => s,
                    Err(e) => core::str::from_utf8(&raw[..e.valid_up_to()]).unwrap_or(""),
                };
                match kw {
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
                    if let Tok::Kw("R") = self.next_tok() {
                        if n >= 0 && g >= 0 {
                            return Ok(Obj::Ref(n as u32, g as u16));
                        }
                    }
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
                if let Tok::Kw("stream") = self.next_tok() {
                    {
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

/// Where an object lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Loc {
    Free,
    Offset(u64),
    InStream(u32, u32),
}

/// A `Loc` packed into eight bytes: two tag bits, then a 62-bit offset or a 30-bit index
/// over a 32-bit stream number. Halves the cross-reference table on the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Packed(u64);

impl Packed {
    const FREE: Packed = Packed(0);
    fn from(loc: Loc) -> Packed {
        match loc {
            Loc::Free => Packed::FREE,
            Loc::Offset(o) => Packed((1 << 62) | (o & ((1 << 62) - 1))),
            Loc::InStream(s, i) => Packed((2 << 62) | (((i as u64) & 0x3FFF_FFFF) << 32) | s as u64),
        }
    }
    fn loc(self) -> Loc {
        match self.0 >> 62 {
            1 => Loc::Offset(self.0 & ((1 << 62) - 1)),
            2 => Loc::InStream(self.0 as u32, ((self.0 >> 32) & 0x3FFF_FFFF) as u32),
            _ => Loc::Free,
        }
    }
}

/// A page: its object number (for destinations), its own dictionary when it was not
/// reached through a reference, and the page-tree ancestors that may hold inherited
/// attributes (`Resources`, `MediaBox`, `CropBox`, `Rotate`), nearest first.
#[derive(Clone, Debug)]
pub struct Page {
    /// Object number, when the page was reached through a reference.
    pub num: Option<u32>,
    /// The page dictionary, kept only for pages without an object number.
    pub dict: Option<Obj>,
    /// Ancestors, nearest first, shared between siblings.
    pub ancestors: alloc::rc::Rc<[u32]>,
}

/// A parsed object stream: (stream number, offsets table, data).
type ObjStm = (u32, Vec<(u32, usize)>, Vec<u8>);
/// An object stream's offsets table and data.
type ObjStmData = (Vec<(u32, usize)>, Vec<u8>);

/// An open document.
pub struct Document<'a, R: ReadAt> {
    src: &'a R,
    xref: Vec<Packed>,
    /// Trailer entries merged across the /Prev chain (first wins).
    pub trailer: Vec<(String, Obj)>,
    /// Cache of parsed object streams.
    objstm_cache: Vec<ObjStm>,
    /// Small parsed objects, least recently used first.
    obj_cache: Vec<(u32, Obj)>,
    /// Objects the reconstruction scan saw a `/Page` in, for the page-tree fallback.
    page_candidates: Vec<u32>,
    reconstructed: bool,
    /// Objects whose parse is in progress (an indirect /Length or an object stream being
    /// resolved), so a reference that leads back to one of them cannot recurse.
    resolving: Vec<u32>,
}

/// Which reading of a buffer `parse_indirect_in` produced.
enum Parsed {
    /// The object, and whether its end (`endobj`, or `stream`) lay inside the buffer.
    Obj(u32, Obj, bool),
    /// The buffer ended inside the object.
    Truncated,
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
        let mut doc = Document {
            src,
            xref: Vec::new(),
            trailer: Vec::new(),
            objstm_cache: Vec::new(),
            obj_cache: Vec::new(),
            page_candidates: Vec::new(),
            reconstructed: false,
            resolving: Vec::new(),
        };
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
        } else if !matches!(doc.catalog(), Ok(Obj::Dict(_))) {
            // The xref loaded but does not lead to a catalog: rebuild it.
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

    fn loc(&self, num: u32) -> Loc {
        self.xref.get(num as usize).map(|p| p.loc()).unwrap_or(Loc::Free)
    }

    fn set(&mut self, num: u32, loc: Loc) {
        if num as usize > 5_000_000 {
            return;
        }
        if self.xref.len() <= num as usize {
            self.xref.resize(num as usize + 1, Packed::FREE);
        }
        // First definition wins (newest xref section is loaded first).
        if self.xref[num as usize] == Packed::FREE {
            self.xref[num as usize] = Packed::from(loc);
        }
    }

    /// Read a cross-reference table at `off`: a small read first, grown until the
    /// `trailer` keyword is inside the buffer (or the buffer is as large as we allow).
    fn read_xref_buf(&self, off: u64) -> Result<Vec<u8>, DocError> {
        let avail = (self.src.len() - off).min(OBJ_LIMIT as u64) as usize;
        let mut n = XREF_FIRST_READ.min(avail);
        loop {
            let buf = self.src.read_range(off, n)?;
            let mut lx = Lexer::new(&buf, off);
            lx.skip_ws();
            let table = buf[lx.pos..].starts_with(b"xref");
            if !table || n >= avail || find(&buf, b"trailer").is_some() {
                return Ok(buf);
            }
            n = (n * 4).min(avail);
        }
    }

    /// Load one xref section (table or stream); returns /Prev.
    fn load_xref_at(&mut self, off: u64) -> Result<Option<u64>, DocError> {
        if off >= self.src.len() {
            return Err(DocError::Malformed("pdf: xref offset"));
        }
        let buf = self.read_xref_buf(off)?;
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
                    }
                }
            }
        }
        // Cross-reference stream: "n g obj <<...>> stream".
        let (_, obj) = self.parse_indirect_at(off)?;
        let data = match &obj {
            Obj::Stream { .. } => self.stream_data(&obj)?,
            _ => return Err(DocError::Malformed("pdf: xref stream")),
        };
        let Obj::Stream { dict, .. } = obj else { return Err(DocError::Malformed("pdf: xref stream")) };
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
    /// broken files, which are common. Only objects whose header is followed by
    /// `/ObjStm`, `/Catalog` or `/XRef` are parsed afterwards; everything else is just
    /// registered by offset.
    fn reconstruct(&mut self) -> Result<(), DocError> {
        /// Bytes after `obj` inspected for the type of the object.
        const PEEK: usize = 256;
        /// Bytes carried from one chunk to the next, so headers and peeks never split.
        const CARRY: usize = 1024;
        self.reconstructed = true;
        self.xref.clear();
        self.obj_cache.clear();
        self.objstm_cache.clear();
        self.page_candidates.clear();
        let len = self.src.len();
        let mut pos = 0u64;
        let mut buf = alloc::vec![0u8; SCAN_CHUNK];
        let mut carry: Vec<u8> = Vec::new();
        let mut found_root = false;
        let mut objstms: Vec<u32> = Vec::new();
        let mut catalogs: Vec<u32> = Vec::new();
        let mut xrefstms: Vec<u32> = Vec::new();
        while pos < len {
            let n = self.src.read_at(pos, &mut buf)?;
            if n == 0 {
                break;
            }
            let last = pos + n as u64 >= len;
            let mut data = core::mem::take(&mut carry);
            let carry_len = data.len();
            data.extend_from_slice(&buf[..n]);
            let base = pos - carry_len as u64;
            let mut i = 0usize;
            while i + 3 < data.len() {
                if &data[i..i + 3] == b"obj" && (i == 0 || is_ws(data[i - 1]) || data[i - 1].is_ascii_digit()) {
                    // Headers too close to the end of this chunk are left to the next
                    // pass, whose carry contains them whole.
                    if !last && i + 3 + PEEK > data.len() {
                        break;
                    }
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
                                    self.xref.resize(num as usize + 1, Packed::FREE);
                                }
                                self.xref[num as usize] = Packed::from(Loc::Offset(off)); // later definitions win
                                let window = &data[i + 3..(i + 3 + PEEK).min(data.len())];
                                if find(window, b"/ObjStm").is_some() && objstms.len() < 4096 {
                                    objstms.push(num);
                                }
                                if find(window, b"/Catalog").is_some() && catalogs.len() < 64 {
                                    catalogs.push(num);
                                }
                                if find(window, b"/XRef").is_some() && xrefstms.len() < 64 {
                                    xrefstms.push(num);
                                }
                                if has_page_type(window) && self.page_candidates.len() < PAGE_LIMIT {
                                    self.page_candidates.push(num);
                                }
                            }
                        }
                    }
                    i += 3;
                    continue;
                }
                if data[i] == b't' && data[i..].starts_with(b"trailer") {
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
            let keep = data.len().min(CARRY);
            carry = data[data.len() - keep..].to_vec();
            pos += n as u64;
        }
        for v in [&mut objstms, &mut catalogs, &mut xrefstms, &mut self.page_candidates] {
            v.sort_unstable();
            v.dedup();
        }
        if !found_root {
            // Prefer the last catalog in the file (incremental updates append).
            for &num in catalogs.iter().rev() {
                if let Ok(o) = self.get(num) {
                    if o.get("Type").and_then(|t| t.name()) == Some("Catalog") {
                        self.trailer.push(("Root".into(), Obj::Ref(num, 0)));
                        found_root = true;
                        break;
                    }
                }
            }
        }
        if !found_root {
            for &num in xrefstms.iter().rev() {
                if let Ok(o) = self.get(num) {
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
        // Objects inside object streams: register them so lookups succeed.
        for &num in &objstms {
            let Ok(i) = self.objstm_index(num) else { continue };
            let count = self.objstm_cache[i].1.len();
            for idx in 0..count {
                let on = self.objstm_cache[i].1[idx].0 as usize;
                if on < 5_000_000 {
                    if self.xref.len() <= on {
                        self.xref.resize(on + 1, Packed::FREE);
                    }
                    if self.xref[on] == Packed::FREE {
                        self.xref[on] = Packed::from(Loc::InStream(num, idx as u32));
                    }
                }
            }
        }
        if !found_root {
            // The catalog may live inside an object stream: look there, stream by stream.
            let mut in_streams: Vec<(u32, u32)> = Vec::new();
            for (num, p) in self.xref.iter().enumerate() {
                if let Loc::InStream(s, _) = p.loc() {
                    in_streams.push((s, num as u32));
                }
            }
            in_streams.sort_unstable();
            for (_, num) in in_streams {
                if let Ok(o) = self.get(num) {
                    if o.get("Type").and_then(|t| t.name()) == Some("Catalog") {
                        self.trailer.push(("Root".into(), Obj::Ref(num, 0)));
                        found_root = true;
                        break;
                    }
                    if o.get("Type").and_then(|t| t.name()) == Some("Page") && self.page_candidates.len() < PAGE_LIMIT {
                        self.page_candidates.push(num);
                    }
                }
            }
        }
        if !found_root {
            return Err(DocError::Malformed("pdf: no catalog"));
        }
        Ok(())
    }

    /// Parse "n g obj ... endobj" at an offset: a small read first, grown ×4 up to
    /// `OBJ_LIMIT` while the buffer ends inside the object.
    fn parse_indirect_at(&mut self, off: u64) -> Result<(u32, Obj), DocError> {
        if off >= self.src.len() {
            return Err(DocError::Malformed("pdf: object offset"));
        }
        let avail = (self.src.len() - off).min(OBJ_LIMIT as u64) as usize;
        let mut n = OBJ_FIRST_READ.min(avail);
        loop {
            let buf = self.src.read_range(off, n)?;
            let whole = n >= avail;
            match Self::parse_indirect_in(&buf, off)? {
                Parsed::Obj(num, o, done) if done || whole => return Ok((num, self.fix_stream_len(num, o)?)),
                Parsed::Truncated if whole => return Err(DocError::Malformed("pdf: unexpected end")),
                _ => {}
            }
            n = (n * 4).min(avail);
        }
    }

    /// Parse an indirect object from a buffer.
    fn parse_indirect_in(buf: &[u8], off: u64) -> Result<Parsed, DocError> {
        let mut lx = Lexer::new(buf, off);
        let num = match (lx.next_tok(), lx.next_tok(), lx.next_tok()) {
            (Tok::Obj(Obj::Int(n)), Tok::Obj(Obj::Int(_)), Tok::Kw("obj")) if n >= 0 => n as u32,
            (Tok::Eof, _, _) | (_, Tok::Eof, _) | (_, _, Tok::Eof) => return Ok(Parsed::Truncated),
            _ => return Err(DocError::Malformed("pdf: expected obj")),
        };
        let Ok(o) = lx.parse_obj(0) else { return Ok(Parsed::Truncated) };
        let done = match &o {
            Obj::Stream { .. } => true,
            _ => {
                lx.skip_ws();
                let rest = &buf[lx.pos.min(buf.len())..];
                // Nothing after the object, or the start of a keyword cut short, means
                // the buffer may have ended inside it.
                !rest.is_empty() && !(rest.len() < 6 && (b"stream".starts_with(rest) || b"endobj".starts_with(rest)))
            }
        };
        Ok(Parsed::Obj(num, o, done))
    }

    /// Resolve an indirect /Length, else scan the file for `endstream`. The object being
    /// parsed (`num`) and its parents are never re-entered: a /Length that points back at
    /// one of them (a known hostile pattern) falls back to the scan.
    fn fix_stream_len(&mut self, num: u32, mut o: Obj) -> Result<Obj, DocError> {
        if let Obj::Stream { dict, offset, len } = &mut o {
            if *len == u64::MAX {
                let l = dict.iter().find(|(k, _)| k == "Length").map(|(_, v)| v.clone());
                let resolved = match l {
                    Some(Obj::Ref(n, _)) if n != num && !self.resolving.contains(&n) && self.resolving.len() < 4 => {
                        self.resolving.push(num);
                        let r = self.get(n).ok().and_then(|o| o.int()).filter(|x| *x >= 0).map(|x| x as u64);
                        self.resolving.pop();
                        r
                    }
                    _ => None,
                };
                *len = match resolved {
                    Some(v) => v,
                    None => self.scan_endstream(*offset)?,
                };
            }
        }
        Ok(o)
    }

    /// Distance from `start` to the `endstream` keyword (less a trailing EOL), read in
    /// small chunks and bounded by `STREAM_LIMIT`.
    fn scan_endstream(&self, start: u64) -> Result<u64, DocError> {
        const CHUNK: usize = 4096;
        const NEEDLE: &[u8] = b"endstream";
        let mut buf = alloc::vec![0u8; CHUNK + NEEDLE.len()];
        let mut pos = start;
        let mut carry = 0usize;
        let end = start.saturating_add(STREAM_LIMIT as u64).min(self.src.len());
        while pos < end {
            let want = ((end - pos) as usize).min(CHUNK);
            let n = self.src.read_at(pos, &mut buf[carry..carry + want])?;
            if n == 0 {
                break;
            }
            let total = carry + n;
            let data = &buf[..total];
            if let Some(i) = find(data, NEEDLE) {
                let mut e = pos - carry as u64 + i as u64;
                // Trim the EOL that precedes the keyword when the data is shorter.
                let mut k = i;
                if k > 0 && data[k - 1] == b'\n' {
                    k -= 1;
                    e -= 1;
                }
                if k > 0 && data[k - 1] == b'\r' {
                    e -= 1;
                }
                return Ok(e - start);
            }
            pos += n as u64;
            carry = total.min(NEEDLE.len() - 1);
            buf.copy_within(total - carry..total, 0);
        }
        Ok(end - start)
    }

    /// Index into `objstm_cache` of a parsed object stream, loading it if needed.
    fn objstm_index(&mut self, num: u32) -> Result<usize, DocError> {
        if let Some(i) = self.objstm_cache.iter().position(|e| e.0 == num) {
            return Ok(i);
        }
        if self.resolving.contains(&num) || self.resolving.len() >= 4 {
            return Err(DocError::Malformed("pdf: nested object streams"));
        }
        self.resolving.push(num);
        let loaded = self.load_objstm(num);
        self.resolving.pop();
        let (table, data) = loaded?;
        if self.objstm_cache.len() >= OBJSTM_CACHE {
            self.objstm_cache.remove(0);
        }
        self.objstm_cache.push((num, table, data));
        Ok(self.objstm_cache.len() - 1)
    }

    fn load_objstm(&mut self, num: u32) -> Result<ObjStmData, DocError> {
        let s = self.get(num)?;
        let n = s.get("N").and_then(|v| v.int()).unwrap_or(0).clamp(0, 10_000) as usize;
        let first = s.get("First").and_then(|v| v.int()).unwrap_or(0).max(0) as usize;
        let data = self.stream_data(&s)?;
        let mut lx = Lexer::new(&data[..first.min(data.len())], 0);
        let mut table = Vec::with_capacity(n);
        for _ in 0..n {
            match (lx.next_tok(), lx.next_tok()) {
                (Tok::Obj(Obj::Int(on)), Tok::Obj(Obj::Int(off))) if on >= 0 && off >= 0 => {
                    table.push((on as u32, first.saturating_add(off as usize)))
                }
                _ => break,
            }
        }
        Ok((table, data))
    }

    /// The underlying reader.
    pub fn source(&self) -> &'a R {
        self.src
    }

    /// Fetch an object by number, resolving object streams. Small objects are served
    /// from a cache so page nodes, resources and font dictionaries are parsed once.
    pub fn get(&mut self, num: u32) -> Result<Obj, DocError> {
        if let Some(i) = self.obj_cache.iter().position(|(n, _)| *n == num) {
            let e = self.obj_cache.remove(i);
            let o = e.1.clone();
            self.obj_cache.push(e);
            return Ok(o);
        }
        let o = self.get_uncached(num)?;
        let mut budget = OBJ_CACHE_WEIGHT;
        if o != Obj::Null && o.weigh(&mut budget) {
            if self.obj_cache.len() >= OBJ_CACHE {
                self.obj_cache.remove(0);
            }
            self.obj_cache.push((num, o.clone()));
        }
        Ok(o)
    }

    fn get_uncached(&mut self, num: u32) -> Result<Obj, DocError> {
        if self.resolving.contains(&num) {
            return Err(DocError::Malformed("pdf: object refers to itself"));
        }
        match self.loc(num) {
            // A reference to a free entry is a dangling reference, not a broken file.
            Loc::Free => Ok(Obj::Null),
            Loc::Offset(off) => {
                let parsed = match self.parse_indirect_at(off) {
                    Ok((n, _)) if n != num => Err(DocError::Malformed("pdf: object number")),
                    other => other,
                };
                match parsed {
                    Ok((_, o)) => Ok(o),
                    Err(e) => {
                        if !self.reconstructed {
                            self.reconstruct()?;
                            self.get(num)
                        } else {
                            Err(e)
                        }
                    }
                }
            }
            Loc::InStream(snum, idx) => {
                let i = self.objstm_index(snum)?;
                let (_, table, data) = &self.objstm_cache[i];
                let Some(&(_, off)) = table.get(idx as usize) else { return Ok(Obj::Null) };
                let mut lx = Lexer::new(&data[off.min(data.len())..], 0);
                lx.parse_obj(0)
            }
        }
    }

    /// Follow references until a direct object; a direct object is returned as is.
    pub fn resolve(&mut self, o: &Obj) -> Result<Obj, DocError> {
        let Obj::Ref(n, _) = o else { return Ok(o.clone()) };
        let mut cur = self.get(*n)?;
        for _ in 0..32 {
            match cur {
                Obj::Ref(n, _) => cur = self.get(n)?,
                other => return Ok(other),
            }
        }
        Ok(Obj::Null)
    }

    /// `resolve` without a copy when the object is already direct.
    pub fn resolve_cow<'o>(&mut self, o: &'o Obj) -> Result<Cow<'o, Obj>, DocError> {
        if matches!(o, Obj::Ref(..)) {
            self.resolve(o).map(Cow::Owned)
        } else {
            Ok(Cow::Borrowed(o))
        }
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
        let Obj::Stream { offset, len, .. } = s else { return Err(DocError::Malformed("pdf: not a stream")) };
        let filters = self.filters_of(s)?;
        let raw_len = (*len).min(self.src.len().saturating_sub(*offset));
        // Raw bytes are never read past STREAM_LIMIT; Flate streams inflate straight
        // from the file.
        let capped = raw_len.min(STREAM_LIMIT as u64) as usize;
        let mut data: Vec<u8> = Vec::new();
        let mut first = true;
        if filters.is_empty() {
            data = self.src.read_range(*offset, capped)?;
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
                    let input = if first { self.src.read_range(*offset, capped)? } else { core::mem::take(&mut data) };
                    ascii_hex(&input)
                }
                "ASCII85Decode" | "A85" => {
                    let input = if first { self.src.read_range(*offset, capped)? } else { core::mem::take(&mut data) };
                    ascii85(&input)
                }
                "RunLengthDecode" | "RL" => {
                    let input = if first { self.src.read_range(*offset, capped)? } else { core::mem::take(&mut data) };
                    runlength(&input)
                }
                "DCTDecode" | "DCT" | "JPXDecode" | "CCITTFaxDecode" | "CCF" | "JBIG2Decode" => {
                    // Image filters: leave the bytes encoded; the image path handles them
                    // (a JPEG that is the raw stream is decoded straight from the file).
                    if first {
                        self.src.read_range(*offset, capped)?
                    } else {
                        core::mem::take(&mut data)
                    }
                }
                "LZWDecode" | "LZW" => {
                    let input = if first { self.src.read_range(*offset, capped)? } else { core::mem::take(&mut data) };
                    let early = parms.as_ref().and_then(|p| p.get("EarlyChange")).and_then(|v| v.int()).unwrap_or(1) != 0;
                    lzw(&input, early)
                }
                "Crypt" => {
                    if first {
                        self.src.read_range(*offset, capped)?
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
                let parms: Vec<Obj> = match p {
                    Obj::Array(pa) => pa,
                    Obj::Null => Vec::new(),
                    other => alloc::vec![other],
                };
                for (i, x) in a.iter().enumerate() {
                    let n = self.resolve(x)?;
                    if let Obj::Name(n) = n {
                        let pp = match parms.get(i) {
                            Some(o) => Some(self.resolve(o)?).filter(|o| *o != Obj::Null),
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
        Ok(f.into_iter()
            .map(|(n, _)| n)
            .find(|n| matches!(n.as_str(), "DCTDecode" | "DCT" | "JPXDecode" | "CCITTFaxDecode" | "CCF" | "JBIG2Decode")))
    }

    /// The catalog.
    pub fn catalog(&mut self) -> Result<Obj, DocError> {
        let r = self.trailer.iter().find(|(k, _)| k == "Root").map(|(_, v)| v.clone()).ok_or(DocError::Malformed("pdf: no root"))?;
        self.resolve(&r)
    }

    /// All pages in order. Inherited attributes are not copied in: `page_attr` follows
    /// the ancestor chain when they are needed.
    pub fn pages(&mut self) -> Result<Vec<Page>, DocError> {
        let mut out = self.walk_page_tree()?;
        if out.is_empty() && !self.reconstructed {
            self.reconstruct()?;
            out = self.walk_page_tree()?;
        }
        if out.is_empty() {
            // Fallback: the objects the reconstruction scan saw a /Page in.
            let candidates = core::mem::take(&mut self.page_candidates);
            for &num in &candidates {
                if let Ok(o) = self.get(num) {
                    if o.get("Type").and_then(|t| t.name()) == Some("Page") {
                        out.push(Page { num: Some(num), dict: None, ancestors: alloc::rc::Rc::from(&[][..]) });
                    }
                }
                if out.len() >= PAGE_LIMIT {
                    break;
                }
            }
            self.page_candidates = candidates;
        }
        Ok(out)
    }

    fn walk_page_tree(&mut self) -> Result<Vec<Page>, DocError> {
        let cat = self.catalog()?;
        let root_ref = cat.get("Pages").cloned().unwrap_or(Obj::Null);
        let root = self.resolve(&root_ref)?;
        let mut out = Vec::new();
        let root_num = if let Obj::Ref(n, _) = root_ref { Some(n) } else { None };
        let mut seen: Vec<u32> = Vec::new();
        self.walk_pages(root_num, &root, &[], &[], &mut out, &mut seen, 0)?;
        Ok(out)
    }

    /// The page's own dictionary.
    pub fn page_dict(&mut self, page: &Page) -> Result<Obj, DocError> {
        match (&page.dict, page.num) {
            (Some(d), _) => Ok(d.clone()),
            (None, Some(n)) => self.get(n),
            (None, None) => Ok(Obj::Null),
        }
    }

    /// A page attribute, resolved, following the inheritance chain up the page tree.
    pub fn page_attr(&mut self, page: &Page, key: &str) -> Result<Obj, DocError> {
        let own = self.page_dict(page)?;
        if let Some(v) = own.get(key) {
            return self.resolve(v);
        }
        for &a in page.ancestors.iter() {
            let node = self.get(a)?;
            if let Some(v) = node.get(key) {
                return self.resolve(v);
            }
        }
        Ok(Obj::Null)
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_pages(
        &mut self,
        num: Option<u32>,
        node: &Obj,
        ancestors: &[u32],
        direct_inh: &[(String, Obj)],
        out: &mut Vec<Page>,
        seen: &mut Vec<u32>,
        depth: u32,
    ) -> Result<(), DocError> {
        if depth > 32 || out.len() >= PAGE_LIMIT {
            return Ok(());
        }
        let ty = node.get("Type").and_then(|t| t.name()).unwrap_or("");
        let kids = self.get_key(node, "Kids")?;
        if ty == "Page" || (kids == Obj::Null && ty != "Pages") {
            // Attributes of ancestors without an object number (rare) are copied in;
            // everything else is reached lazily through the chain.
            let dict = if num.is_some() && direct_inh.is_empty() {
                None
            } else {
                let mut d: Vec<(String, Obj)> = node.dict().map(|x| x.to_vec()).unwrap_or_default();
                for (k, v) in direct_inh {
                    if !d.iter().any(|(kk, _)| kk == k) {
                        d.push((k.clone(), v.clone()));
                    }
                }
                Some(Obj::Dict(d))
            };
            out.push(Page { num, dict, ancestors: alloc::rc::Rc::from(ancestors) });
            return Ok(());
        }
        let mut chain: Vec<u32> = Vec::with_capacity(ancestors.len() + 1);
        let mut inh: Vec<(String, Obj)> = direct_inh.to_vec();
        match num {
            Some(n) => {
                chain.push(n);
                chain.extend_from_slice(ancestors);
            }
            None => {
                chain.extend_from_slice(ancestors);
                for key in ["Resources", "MediaBox", "CropBox", "Rotate"] {
                    if let Some(v) = node.get(key) {
                        inh.retain(|(k, _)| k != key);
                        inh.push((key.into(), v.clone()));
                    }
                }
            }
        }
        if let Obj::Array(a) = kids {
            for k in a {
                let knum = if let Obj::Ref(n, _) = k { Some(n) } else { None };
                if let Some(n) = knum {
                    if seen.contains(&n) {
                        continue;
                    }
                    seen.push(n);
                }
                let child = self.resolve(&k)?;
                if child != Obj::Null {
                    self.walk_pages(knum, &child, &chain, &inh, out, seen, depth + 1)?;
                }
            }
        }
        Ok(())
    }
}

/// Whether a peek window after `obj` names a `/Type /Page` (not `/Pages`).
fn has_page_type(w: &[u8]) -> bool {
    let mut i = 0;
    while let Some(p) = find(&w[i..], b"/Page") {
        let after = w.get(i + p + 5).copied();
        if after.map(|c| !c.is_ascii_alphanumeric()).unwrap_or(true) {
            return true;
        }
        i += p + 5;
    }
    false
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
    fn self_referencing_length_falls_back_to_scanning() {
        // /Length pointing at the stream's own object (or through a cycle) must not
        // recurse; the data length comes from the `endstream` scan instead.
        let pdf = b"%PDF-1.4\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >> endobj\n\
4 0 obj << /Length 4 0 R >>\nstream\nBT (hi) Tj ET\nendstream endobj\n\
5 0 obj << /Length 6 0 R >>\nstream\nabc\nendstream endobj\n\
6 0 obj << /Length 5 0 R >>\nstream\nxy\nendstream endobj\n\
trailer << /Root 1 0 R >>\nstartxref\n0\n%%EOF";
        let src: &[u8] = &pdf[..];
        let mut doc = Document::open(&src).expect("open");
        assert_eq!(doc.pages().unwrap().len(), 1);
        let s = doc.get(4).unwrap();
        assert!(matches!(s, Obj::Stream { len: 13, .. }), "{s:?}");
        assert_eq!(doc.stream_data(&s).unwrap(), b"BT (hi) Tj ET");
        let s5 = doc.get(5).unwrap();
        assert!(matches!(s5, Obj::Stream { len: 3, .. }), "{s5:?}");
        let s6 = doc.get(6).unwrap();
        assert!(matches!(s6, Obj::Stream { len: 2, .. }), "{s6:?}");
    }

    #[test]
    fn objects_longer_than_the_first_read_are_grown() {
        // A 20 KB string object (far past the 2 KB first read) parses whole, and a
        // dangling reference resolves to null without triggering reconstruction.
        let big: String = core::iter::repeat_n("abcdefghij", 2000).collect();
        let pdf = alloc::format!(
            "%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R /Big 4 0 R /Gone 9 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R >> endobj\n\
             4 0 obj [ ({big}) /After ] endobj\n\
             trailer << /Root 1 0 R >>\n%%EOF"
        );
        let src: &[u8] = pdf.as_bytes();
        let mut doc = Document::open(&src).expect("open");
        let cat = doc.catalog().unwrap();
        let b = doc.get_key(&cat, "Big").unwrap();
        let a = b.array().expect("array");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0], Obj::Str(big.into_bytes()));
        assert_eq!(a[1], Obj::Name("After".into()));
        assert_eq!(doc.get_key(&cat, "Gone").unwrap(), Obj::Null);
        // The catalog is small enough to be cached: fetching it again costs no read.
        let before = doc.obj_cache.len();
        let _ = doc.catalog().unwrap();
        assert_eq!(doc.obj_cache.len(), before);
        assert!(doc.obj_cache.iter().any(|(n, _)| *n == 1));
    }

    #[test]
    fn inherited_page_attributes_follow_the_chain() {
        let pdf = b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [5 0 R] /Count 1 /MediaBox [0 0 300 400] /Rotate 90 >> endobj\n\
5 0 obj << /Type /Pages /Parent 2 0 R /Kids [3 0 R] /Count 1 /Rotate 180 >> endobj\n\
3 0 obj << /Type /Page /Parent 5 0 R >> endobj\n\
trailer << /Root 1 0 R >>\n%%EOF";
        let src: &[u8] = &pdf[..];
        let mut doc = Document::open(&src).expect("open");
        let pages = doc.pages().unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].num, Some(3));
        assert!(pages[0].dict.is_none(), "page reached by reference keeps no copy of its dictionary");
        assert_eq!(&pages[0].ancestors[..], &[5, 2]);
        assert_eq!(doc.page_attr(&pages[0], "Rotate").unwrap(), Obj::Int(180), "nearest ancestor wins");
        let mb = doc.page_attr(&pages[0], "MediaBox").unwrap();
        assert_eq!(mb.array().map(|a| a.len()), Some(4));
        assert_eq!(doc.page_attr(&pages[0], "Resources").unwrap(), Obj::Null);
    }

    #[test]
    fn encrypted_is_refused() {
        let pdf = b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [] /Count 0 >> endobj\ntrailer << /Root 1 0 R /Encrypt 3 0 R >>\n%%EOF";
        assert!(matches!(Document::open(&&pdf[..]), Err(DocError::Drm)));
    }
}

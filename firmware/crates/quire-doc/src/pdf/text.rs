//! PDF text extraction and reflow.
//!
//! Every page's content streams are interpreted (text state, matrices, forms, images)
//! into positioned runs, which a recursive XY-cut turns back into reading order:
//! columns, paragraphs, headings, figures. Fonts are decoded through ToUnicode maps,
//! the standard encodings and Differences. Scanned pages come out as image pages.
//! Everything is bounded so a hostile file cannot exhaust the device's memory.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_gfx::Bitmap;
use quire_qtx::{style, ParaKind, Token, Writer};

use super::{Document, Lexer, Obj, Tok};
use crate::image::{Fit, RowSink};
use crate::inflate::{Framing, Inflater};
use crate::{limits, DocError, Metadata, Sink, TocEntry};

/// Largest inflated page content we hold at once.
#[cfg(target_os = "none")]
const CONTENT_LIMIT: usize = 96 * 1024;
/// Largest inflated page content we hold at once on the host.
#[cfg(not(target_os = "none"))]
const CONTENT_LIMIT: usize = 1024 * 1024;
/// Operators per page before we stop interpreting (runaway forms).
const OP_BUDGET: u32 = 400_000;
/// Positioned items per page.
#[cfg(target_os = "none")]
const ITEM_LIMIT: usize = 1200;
/// Positioned items per page on the host.
#[cfg(not(target_os = "none"))]
const ITEM_LIMIT: usize = 20_000;
/// Text of one page (all items share one arena).
#[cfg(target_os = "none")]
const TEXT_LIMIT: usize = 48 * 1024;
/// Text of one page on the host.
#[cfg(not(target_os = "none"))]
const TEXT_LIMIT: usize = 512 * 1024;
/// Fonts kept decoded across pages.
#[cfg(target_os = "none")]
const FONT_CACHE: usize = 8;
/// Fonts kept decoded across pages on the host.
#[cfg(not(target_os = "none"))]
const FONT_CACHE: usize = 48;
/// ToUnicode entries per font.
#[cfg(target_os = "none")]
const CMAP_LIMIT: usize = 4096;
/// ToUnicode entries per font on the host.
#[cfg(not(target_os = "none"))]
const CMAP_LIMIT: usize = 20_000;
/// CID width ranges per font.
#[cfg(target_os = "none")]
const CID_WIDTHS: usize = 2048;
/// CID width ranges per font on the host.
#[cfg(not(target_os = "none"))]
const CID_WIDTHS: usize = 8192;
/// Named destinations we keep.
#[cfg(target_os = "none")]
const NAMED_LIMIT: usize = 512;
/// Named destinations we keep on the host.
#[cfg(not(target_os = "none"))]
const NAMED_LIMIT: usize = 5000;
/// Pages per chapter when the file has no outline.
const PAGES_PER_CHAPTER: usize = 10;
/// Page image target (a scanned page fills the reading area).
const PAGE_W: u32 = 496;
const PAGE_H: u32 = 744;

// ---------------------------------------------------------------------------------------
// Small float helpers (no libm on the device).

fn fabs(x: f32) -> f32 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}
/// Approximate hypotenuse without sqrt (within 3 %).
fn mag(a: f32, b: f32) -> f32 {
    let (a, b) = (fabs(a), fabs(b));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    if hi <= 0.0 {
        return 0.0;
    }
    let r = lo / hi;
    let r2 = r * r;
    hi * (1.0 + 0.5 * r2 - 0.125 * r2 * r2)
}
fn finite(x: f32) -> f32 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}
fn fcmp(a: f32, b: f32) -> core::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(core::cmp::Ordering::Equal)
}

/// A 2-D affine matrix `[a b c d e f]`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mat {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Mat {
    const ID: Mat = Mat { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
    fn translate(x: f32, y: f32) -> Mat {
        Mat { e: x, f: y, ..Mat::ID }
    }
    /// `self × m` (apply self first, then m).
    fn mul(self, m: Mat) -> Mat {
        Mat {
            a: self.a * m.a + self.b * m.c,
            b: self.a * m.b + self.b * m.d,
            c: self.c * m.a + self.d * m.c,
            d: self.c * m.b + self.d * m.d,
            e: self.e * m.a + self.f * m.c + m.e,
            f: self.e * m.b + self.f * m.d + m.f,
        }
    }
    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
    fn from_obj(v: &[Obj]) -> Option<Mat> {
        if v.len() < 6 {
            return None;
        }
        let n: Vec<f32> = v.iter().take(6).map(|o| finite(o.num().unwrap_or(0.0))).collect();
        Some(Mat { a: n[0], b: n[1], c: n[2], d: n[3], e: n[4], f: n[5] })
    }
}

// ---------------------------------------------------------------------------------------
// Encodings.

/// Windows-1252 upper half (128–159); the rest is Latin-1.
const WIN_HIGH: [char; 32] = [
    '€', '\0', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\0', 'Ž', '\0', '\0', '‘', '’', '“', '”', '•', '–', '—', '˜', '™',
    'š', '›', 'œ', '\0', 'ž', 'Ÿ',
];
/// MacRomanEncoding 128–255.
const MAC_HIGH: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»…\u{a0}ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{f8ff}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";
/// StandardEncoding entries above 126.
const STD_HIGH: &[(u8, char)] = &[
    (161, '¡'),
    (162, '¢'),
    (163, '£'),
    (164, '⁄'),
    (165, '¥'),
    (166, 'ƒ'),
    (167, '§'),
    (168, '¤'),
    (169, '\''),
    (170, '“'),
    (171, '«'),
    (172, '‹'),
    (173, '›'),
    (174, 'ﬁ'),
    (175, 'ﬂ'),
    (177, '–'),
    (178, '†'),
    (179, '‡'),
    (180, '·'),
    (182, '¶'),
    (183, '•'),
    (184, '‚'),
    (185, '„'),
    (186, '”'),
    (187, '»'),
    (188, '…'),
    (189, '‰'),
    (191, '¿'),
    (193, '`'),
    (194, '´'),
    (195, 'ˆ'),
    (196, '˜'),
    (197, '¯'),
    (198, '˘'),
    (199, '˙'),
    (200, '¨'),
    (202, '˚'),
    (203, '¸'),
    (205, '˝'),
    (206, '˛'),
    (207, 'ˇ'),
    (208, '—'),
    (225, 'Æ'),
    (227, 'ª'),
    (232, 'Ł'),
    (233, 'Ø'),
    (234, 'Œ'),
    (235, 'º'),
    (241, 'æ'),
    (245, 'ı'),
    (248, 'ł'),
    (249, 'ø'),
    (250, 'œ'),
    (251, 'ß'),
];

#[derive(Clone, Copy, PartialEq)]
enum BaseEnc {
    Standard,
    WinAnsi,
    MacRoman,
}

fn base_char(enc: BaseEnc, code: u8) -> Option<char> {
    match enc {
        BaseEnc::WinAnsi => match code {
            32..=126 => Some(code as char),
            128..=159 => Some(WIN_HIGH[(code - 128) as usize]).filter(|c| *c != '\0'),
            160 => Some('\u{a0}'),
            173 => Some('-'),
            161..=255 => Some(code as char),
            _ => None,
        },
        BaseEnc::MacRoman => match code {
            32..=126 => Some(code as char),
            128..=255 => MAC_HIGH.chars().nth((code - 128) as usize),
            _ => None,
        },
        BaseEnc::Standard => match code {
            39 => Some('’'),
            96 => Some('‘'),
            32..=126 => Some(code as char),
            _ => STD_HIGH.iter().find(|(c, _)| *c == code).map(|(_, ch)| *ch),
        },
    }
}

/// Adobe Glyph List subset: every name the base encodings use, plus common extras.
const AGL: &[(&str, char)] = &[
    ("space", ' '),
    ("exclam", '!'),
    ("quotedbl", '"'),
    ("numbersign", '#'),
    ("dollar", '$'),
    ("percent", '%'),
    ("ampersand", '&'),
    ("quotesingle", '\''),
    ("quoteright", '’'),
    ("quoteleft", '‘'),
    ("parenleft", '('),
    ("parenright", ')'),
    ("asterisk", '*'),
    ("plus", '+'),
    ("comma", ','),
    ("hyphen", '-'),
    ("minus", '−'),
    ("period", '.'),
    ("slash", '/'),
    ("zero", '0'),
    ("one", '1'),
    ("two", '2'),
    ("three", '3'),
    ("four", '4'),
    ("five", '5'),
    ("six", '6'),
    ("seven", '7'),
    ("eight", '8'),
    ("nine", '9'),
    ("colon", ':'),
    ("semicolon", ';'),
    ("less", '<'),
    ("equal", '='),
    ("greater", '>'),
    ("question", '?'),
    ("at", '@'),
    ("bracketleft", '['),
    ("backslash", '\\'),
    ("bracketright", ']'),
    ("asciicircum", '^'),
    ("underscore", '_'),
    ("grave", '`'),
    ("braceleft", '{'),
    ("bar", '|'),
    ("braceright", '}'),
    ("asciitilde", '~'),
    ("exclamdown", '¡'),
    ("cent", '¢'),
    ("sterling", '£'),
    ("fraction", '⁄'),
    ("yen", '¥'),
    ("florin", 'ƒ'),
    ("section", '§'),
    ("currency", '¤'),
    ("quotedblleft", '“'),
    ("quotedblright", '”'),
    ("guillemotleft", '«'),
    ("guillemotright", '»'),
    ("guilsinglleft", '‹'),
    ("guilsinglright", '›'),
    ("fi", 'ﬁ'),
    ("fl", 'ﬂ'),
    ("ff", 'ﬀ'),
    ("ffi", 'ﬃ'),
    ("ffl", 'ﬄ'),
    ("endash", '–'),
    ("emdash", '—'),
    ("dagger", '†'),
    ("daggerdbl", '‡'),
    ("periodcentered", '·'),
    ("paragraph", '¶'),
    ("bullet", '•'),
    ("quotesinglbase", '‚'),
    ("quotedblbase", '„'),
    ("ellipsis", '…'),
    ("perthousand", '‰'),
    ("questiondown", '¿'),
    ("acute", '´'),
    ("circumflex", 'ˆ'),
    ("tilde", '˜'),
    ("macron", '¯'),
    ("breve", '˘'),
    ("dotaccent", '˙'),
    ("dieresis", '¨'),
    ("ring", '˚'),
    ("cedilla", '¸'),
    ("hungarumlaut", '˝'),
    ("ogonek", '˛'),
    ("caron", 'ˇ'),
    ("AE", 'Æ'),
    ("ordfeminine", 'ª'),
    ("Lslash", 'Ł'),
    ("Oslash", 'Ø'),
    ("OE", 'Œ'),
    ("ordmasculine", 'º'),
    ("ae", 'æ'),
    ("dotlessi", 'ı'),
    ("lslash", 'ł'),
    ("oslash", 'ø'),
    ("oe", 'œ'),
    ("germandbls", 'ß'),
    ("Euro", '€'),
    ("trademark", '™'),
    ("registered", '®'),
    ("copyright", '©'),
    ("degree", '°'),
    ("plusminus", '±'),
    ("multiply", '×'),
    ("divide", '÷'),
    ("onehalf", '½'),
    ("onequarter", '¼'),
    ("threequarters", '¾'),
    ("onesuperior", '¹'),
    ("twosuperior", '²'),
    ("threesuperior", '³'),
    ("mu", 'µ'),
    ("logicalnot", '¬'),
    ("brokenbar", '¦'),
    ("nbspace", '\u{a0}'),
    ("sfthyphen", '-'),
    ("softhyphen", '-'),
    ("Agrave", 'À'),
    ("Aacute", 'Á'),
    ("Acircumflex", 'Â'),
    ("Atilde", 'Ã'),
    ("Adieresis", 'Ä'),
    ("Aring", 'Å'),
    ("Ccedilla", 'Ç'),
    ("Egrave", 'È'),
    ("Eacute", 'É'),
    ("Ecircumflex", 'Ê'),
    ("Edieresis", 'Ë'),
    ("Igrave", 'Ì'),
    ("Iacute", 'Í'),
    ("Icircumflex", 'Î'),
    ("Idieresis", 'Ï'),
    ("Eth", 'Ð'),
    ("Ntilde", 'Ñ'),
    ("Ograve", 'Ò'),
    ("Oacute", 'Ó'),
    ("Ocircumflex", 'Ô'),
    ("Otilde", 'Õ'),
    ("Odieresis", 'Ö'),
    ("Ugrave", 'Ù'),
    ("Uacute", 'Ú'),
    ("Ucircumflex", 'Û'),
    ("Udieresis", 'Ü'),
    ("Yacute", 'Ý'),
    ("Thorn", 'Þ'),
    ("agrave", 'à'),
    ("aacute", 'á'),
    ("acircumflex", 'â'),
    ("atilde", 'ã'),
    ("adieresis", 'ä'),
    ("aring", 'å'),
    ("ccedilla", 'ç'),
    ("egrave", 'è'),
    ("eacute", 'é'),
    ("ecircumflex", 'ê'),
    ("edieresis", 'ë'),
    ("igrave", 'ì'),
    ("iacute", 'í'),
    ("icircumflex", 'î'),
    ("idieresis", 'ï'),
    ("eth", 'ð'),
    ("ntilde", 'ñ'),
    ("ograve", 'ò'),
    ("oacute", 'ó'),
    ("ocircumflex", 'ô'),
    ("otilde", 'õ'),
    ("odieresis", 'ö'),
    ("ugrave", 'ù'),
    ("uacute", 'ú'),
    ("ucircumflex", 'û'),
    ("udieresis", 'ü'),
    ("yacute", 'ý'),
    ("thorn", 'þ'),
    ("ydieresis", 'ÿ'),
    ("Ydieresis", 'Ÿ'),
    ("Scaron", 'Š'),
    ("scaron", 'š'),
    ("Zcaron", 'Ž'),
    ("zcaron", 'ž'),
    ("Ccaron", 'Č'),
    ("ccaron", 'č'),
    ("Rcaron", 'Ř'),
    ("rcaron", 'ř'),
    ("Ecaron", 'Ě'),
    ("ecaron", 'ě'),
    ("Sacute", 'Ś'),
    ("sacute", 'ś'),
    ("Zacute", 'Ź'),
    ("zacute", 'ź'),
    ("Zdotaccent", 'Ż'),
    ("zdotaccent", 'ż'),
    ("Aogonek", 'Ą'),
    ("aogonek", 'ą'),
    ("Eogonek", 'Ę'),
    ("eogonek", 'ę'),
    ("Nacute", 'Ń'),
    ("nacute", 'ń'),
    ("Cacute", 'Ć'),
    ("cacute", 'ć'),
    ("Gbreve", 'Ğ'),
    ("gbreve", 'ğ'),
    ("Idotaccent", 'İ'),
    ("Scedilla", 'Ş'),
    ("scedilla", 'ş'),
    ("Alpha", 'Α'),
    ("Beta", 'Β'),
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Theta", 'Θ'),
    ("Lambda", 'Λ'),
    ("Pi", 'Π'),
    ("Sigma", 'Σ'),
    ("Phi", 'Φ'),
    ("Omega", 'Ω'),
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("theta", 'θ'),
    ("lambda", 'λ'),
    ("pi", 'π'),
    ("rho", 'ρ'),
    ("sigma", 'σ'),
    ("tau", 'τ'),
    ("phi", 'φ'),
    ("omega", 'ω'),
    ("infinity", '∞'),
    ("lessequal", '≤'),
    ("greaterequal", '≥'),
    ("notequal", '≠'),
    ("approxequal", '≈'),
    ("arrowright", '→'),
    ("arrowleft", '←'),
    ("arrowup", '↑'),
    ("arrowdown", '↓'),
    ("summation", '∑'),
    ("product", '∏'),
    ("radical", '√'),
    ("integral", '∫'),
    ("partialdiff", '∂'),
    ("lozenge", '◊'),
    ("apple", '\u{f8ff}'),
    ("checkmark", '✓'),
    ("dotmath", '⋅'),
    ("universal", '∀'),
    ("existential", '∃'),
    ("element", '∈'),
    ("emptyset", '∅'),
];

/// A glyph name to a character: AGL names, `uniXXXX`, `uXXXX`, and `gXX`-style names (none).
fn glyph_char(name: &str) -> Option<char> {
    if let Some(hex) = name.strip_prefix("uni") {
        if hex.len() >= 4 {
            return u32::from_str_radix(&hex[..4], 16).ok().and_then(char::from_u32);
        }
    }
    if let Some(hex) = name.strip_prefix('u') {
        if (4..=6).contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
        }
    }
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(c);
    }
    // Names like "a123" / "period.sc": strip a suffix after '.'.
    let base = name.split('.').next().unwrap_or(name);
    if base != name && !base.is_empty() {
        return glyph_char(base);
    }
    AGL.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Helvetica widths for codes 32–126 (1/1000 em).
const HELVETICA_W: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278,
    278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556,
    556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
/// Times-Roman widths for codes 32–126.
const TIMES_W: [u16; 95] = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 278,
    278, 564, 564, 564, 444, 921, 722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611, 889, 722, 722, 556, 722, 667, 556, 611, 722,
    722, 944, 722, 722, 611, 333, 278, 333, 469, 500, 333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, 500, 500, 500,
    500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541,
];

// ---------------------------------------------------------------------------------------
// Fonts.

/// A ToUnicode map. Single-code entries hold up to two UTF-16 units inline (one BMP
/// character, a surrogate pair, or a two-character expansion); the rare longer results
/// live in `long`. Both are sorted by code for binary search.
#[derive(Default)]
struct ToUnicode {
    /// (code, units); a zero second unit means a single unit.
    single: Vec<(u32, [u16; 2])>,
    /// Results of more than two units.
    long: Vec<(u32, String)>,
    /// (lo, hi, destination UTF-16 units for lo), sorted by lo.
    ranges: Vec<(u32, u32, Vec<u16>)>,
}

impl ToUnicode {
    /// Append the text of `code` to `out`; false when the map has no usable entry.
    fn push(&self, code: u32, out: &mut String) -> bool {
        if let Ok(i) = self.single.binary_search_by_key(&code, |e| e.0) {
            let u = self.single[i].1;
            let n = if u[1] == 0 { 1 } else { 2 };
            return push_units(u[..n].iter().copied(), out);
        }
        if let Ok(i) = self.long.binary_search_by_key(&code, |e| e.0) {
            let start = out.len();
            push_expanded(out, &self.long[i].1);
            return out.len() > start;
        }
        // The nearest range first, then a few earlier ones (ranges may overlap).
        let idx = self.ranges.partition_point(|r| r.0 <= code);
        for r in self.ranges[..idx].iter().rev().take(64) {
            if code <= r.1 {
                let n = r.2.len();
                let delta = (code - r.0) as u16;
                let units = r.2.iter().enumerate().map(|(k, &u)| if k + 1 == n { u.wrapping_add(delta) } else { u });
                return push_units(units, out);
            }
        }
        false
    }

    fn add_units(&mut self, code: u32, units: &[u16]) {
        let mut n = units.len();
        while n > 0 && units[n - 1] == 0 {
            n -= 1;
        }
        match n {
            0 => {}
            1 => self.single.push((code, [units[0], 0])),
            2 => self.single.push((code, [units[0], units[1]])),
            _ => {
                if self.long.len() < 256 {
                    self.long.push((code, utf16_string(&units[..n])));
                }
            }
        }
    }

    fn add_dst(&mut self, code: u32, dst: &Obj) {
        match dst {
            Obj::Str(d) => {
                let mut units = [0u16; 8];
                let n = d.len().div_ceil(2);
                if n <= units.len() {
                    for (k, c) in d.chunks(2).enumerate() {
                        units[k] = ((c[0] as u16) << 8) | *c.get(1).unwrap_or(&0) as u16;
                    }
                    self.add_units(code, &units[..n]);
                } else {
                    let all: Vec<u16> = d.chunks(2).map(|c| ((c[0] as u16) << 8) | *c.get(1).unwrap_or(&0) as u16).collect();
                    self.add_units(code, &all);
                }
            }
            Obj::Name(n) => {
                if let Some(c) = glyph_char(n) {
                    let mut units = [0u16; 2];
                    let k = c.encode_utf16(&mut units).len();
                    self.add_units(code, &units[..k]);
                }
            }
            _ => {}
        }
    }

    /// Sort the tables; later definitions of a code win.
    fn finish(&mut self) {
        self.single.sort_by_key(|e| e.0);
        dedup_last(&mut self.single);
        self.long.sort_by_key(|e| e.0);
        dedup_last(&mut self.long);
        self.ranges.sort_by_key(|r| r.0);
    }
}

/// Keep the last of each run of equal keys in a stably sorted vector.
fn dedup_last<T>(v: &mut Vec<(u32, T)>) {
    let mut w = 0;
    for i in 0..v.len() {
        if i + 1 < v.len() && v[i + 1].0 == v[i].0 {
            continue;
        }
        v.swap(w, i);
        w += 1;
    }
    v.truncate(w);
}

/// Decode UTF-16 units into `out` (ligatures expanded, NULs dropped). False when nothing
/// usable came out (empty, or only U+FFFD), in which case `out` is left as it was.
fn push_units(units: impl Iterator<Item = u16>, out: &mut String) -> bool {
    let start = out.len();
    let mut useful = false;
    for r in char::decode_utf16(units) {
        let c = r.unwrap_or('\u{fffd}');
        if c == '\0' {
            continue;
        }
        if c != '\u{fffd}' {
            useful = true;
        }
        push_expanded_char(out, c);
    }
    if !useful {
        out.truncate(start);
    }
    useful
}

/// Push a character, expanding typographic ligatures and normalising the few
/// characters the reading fonts lack. ASCII takes the fast path.
fn push_expanded_char(out: &mut String, c: char) {
    if (c as u32) < 0xa0 {
        out.push(c);
        return;
    }
    match c {
        '\u{fb00}' => out.push_str("ff"),
        '\u{fb01}' => out.push_str("fi"),
        '\u{fb02}' => out.push_str("fl"),
        '\u{fb03}' => out.push_str("ffi"),
        '\u{fb04}' => out.push_str("ffl"),
        '\u{fb05}' | '\u{fb06}' => out.push_str("st"),
        '\u{a0}' => out.push(' '),
        '\u{ad}' => {}
        c => out.push(c),
    }
}

fn push_expanded(out: &mut String, s: &str) {
    for c in s.chars() {
        push_expanded_char(out, c);
    }
}

fn utf16_string(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied()).map(|r| r.unwrap_or('\u{fffd}')).filter(|c| *c != '\0').collect()
}

fn bytes_code(b: &[u8]) -> u32 {
    b.iter().take(4).fold(0u32, |acc, x| (acc << 8) | *x as u32)
}

/// A decoded CMap: codespace byte lengths, cid ranges (embedded encodings), bf entries (ToUnicode).
#[derive(Default)]
struct CMap {
    /// (byte length, lo, hi).
    codespace: Vec<(u8, u32, u32)>,
    /// (lo, hi, cid of lo).
    cid: Vec<(u32, u32, u32)>,
    to_unicode: ToUnicode,
}

fn parse_cmap(data: &[u8]) -> CMap {
    let mut cm = CMap::default();
    let mut lx = Lexer::new(data, 0);
    let mut entries = 0usize;
    let mut guard = 0u32;
    loop {
        guard += 1;
        if guard > 2_000_000 || entries > CMAP_LIMIT {
            break;
        }
        let t = lx.next_tok();
        match t {
            Tok::Eof => break,
            Tok::Obj(_) | Tok::ArrClose | Tok::DictClose => {}
            Tok::ArrOpen => {
                lx.pos -= 1;
                if lx.parse_obj(0).is_err() {
                    break;
                }
            }
            Tok::DictOpen => {
                lx.pos -= 2;
                if lx.parse_obj(0).is_err() {
                    break;
                }
            }
            Tok::Kw(k) => match k {
                "begincodespacerange" => loop {
                    let (lo, hi) = (lx.next_tok(), lx.next_tok());
                    match (lo, hi) {
                        (Tok::Obj(Obj::Str(lo)), Tok::Obj(Obj::Str(hi))) => {
                            let n = lo.len().clamp(1, 4) as u8;
                            cm.codespace.push((n, bytes_code(&lo), bytes_code(&hi)));
                        }
                        _ => break,
                    }
                },
                "begincidrange" => {
                    while let (Tok::Obj(Obj::Str(lo)), Tok::Obj(Obj::Str(hi)), Tok::Obj(Obj::Int(c))) =
                        (lx.next_tok(), lx.next_tok(), lx.next_tok())
                    {
                        cm.cid.push((bytes_code(&lo), bytes_code(&hi), c.max(0) as u32));
                        entries += 1;
                        if entries > CMAP_LIMIT {
                            break;
                        }
                    }
                }
                "begincidchar" => {
                    while let (Tok::Obj(Obj::Str(s)), Tok::Obj(Obj::Int(c))) = (lx.next_tok(), lx.next_tok()) {
                        let code = bytes_code(&s);
                        cm.cid.push((code, code, c.max(0) as u32));
                        entries += 1;
                        if entries > CMAP_LIMIT {
                            break;
                        }
                    }
                }
                "beginbfchar" => {
                    while let (Tok::Obj(Obj::Str(src)), Tok::Obj(dst)) = (lx.next_tok(), lx.next_tok()) {
                        cm.to_unicode.add_dst(bytes_code(&src), &dst);
                        entries += 1;
                        if entries > CMAP_LIMIT {
                            break;
                        }
                    }
                }
                "beginbfrange" => 'ranges: loop {
                    let (lo, hi) = match (lx.next_tok(), lx.next_tok()) {
                        (Tok::Obj(Obj::Str(lo)), Tok::Obj(Obj::Str(hi))) => (bytes_code(&lo), bytes_code(&hi)),
                        _ => break 'ranges,
                    };
                    if entries > CMAP_LIMIT {
                        break 'ranges;
                    }
                    match lx.next_tok() {
                        Tok::Obj(Obj::Str(d)) => {
                            let units: Vec<u16> = d.chunks(2).map(|c| ((c[0] as u16) << 8) | *c.get(1).unwrap_or(&0) as u16).collect();
                            if hi >= lo {
                                cm.to_unicode.ranges.push((lo, hi, units));
                            }
                            entries += 1;
                        }
                        Tok::ArrOpen => {
                            lx.pos -= 1;
                            if let Ok(Obj::Array(a)) = lx.parse_obj(0) {
                                for (i, d) in a.iter().enumerate().take(CMAP_LIMIT) {
                                    cm.to_unicode.add_dst(lo + i as u32, d);
                                }
                                entries += a.len();
                            }
                        }
                        _ => break,
                    }
                },
                _ => {}
            },
        }
    }
    cm.to_unicode.finish();
    cm.cid.sort_by_key(|r| r.0);
    cm
}

#[derive(Clone, Copy, PartialEq)]
enum FontKind {
    Simple,
    Type0,
    Type3,
}

struct Font {
    kind: FontKind,
    /// Code byte lengths (Type0); empty means two bytes.
    codespace: Vec<(u8, u32, u32)>,
    cid: Vec<(u32, u32, u32)>,
    ucs2_direct: bool,
    to_unicode: ToUnicode,
    /// Simple fonts: code → char.
    encoding: Vec<char>,
    first_char: u32,
    widths: Vec<u16>,
    has_widths: bool,
    missing_width: u16,
    std_widths: Option<&'static [u16; 95]>,
    fixed_width: Option<u16>,
    /// CID widths (lo, hi, w) sorted by lo, and the default.
    cid_widths: Vec<(u32, u32, u16)>,
    dw: u16,
    type3_scale: f32,
    flags: u8,
}

/// The (code, byte length) pairs of a shown string, without an intermediate vector.
struct Codes<'a> {
    font: &'a Font,
    b: &'a [u8],
    i: usize,
}

impl Iterator for Codes<'_> {
    type Item = (u32, u8);
    fn next(&mut self) -> Option<(u32, u8)> {
        let (b, i) = (self.b, self.i);
        if i >= b.len() {
            return None;
        }
        if self.font.kind != FontKind::Type0 {
            self.i += 1;
            return Some((b[i] as u32, 1));
        }
        let mut n = 0u8;
        if self.font.codespace.is_empty() {
            n = 2;
        } else {
            // Try lengths 1..4: the first codespace range that contains the prefix wins.
            for len in 1..=4u8 {
                if i + len as usize > b.len() {
                    break;
                }
                let c = bytes_code(&b[i..i + len as usize]);
                if self.font.codespace.iter().any(|(l, lo, hi)| *l == len && c >= *lo && c <= *hi) {
                    n = len;
                    break;
                }
            }
            if n == 0 {
                // Not in any range: use the shortest declared length.
                n = self.font.codespace.iter().map(|r| r.0).min().unwrap_or(1);
            }
        }
        let n = (n as usize).min(b.len() - i).max(1);
        self.i += n;
        Some((bytes_code(&b[i..i + n]), n as u8))
    }
}

impl Font {
    /// Split a string into (code, byte length) pairs.
    fn codes<'a>(&'a self, b: &'a [u8]) -> Codes<'a> {
        Codes { font: self, b, i: 0 }
    }

    fn cid_of(&self, code: u32) -> u32 {
        if self.cid.is_empty() {
            return code;
        }
        let idx = self.cid.partition_point(|r| r.0 <= code);
        for r in self.cid[..idx].iter().rev().take(64) {
            if code >= r.0 && code <= r.1 {
                return r.2 + (code - r.0);
            }
        }
        code
    }

    /// Append the text for a code to `out` (ligatures expanded so the reading fonts can
    /// show them); false when the font has no mapping for it.
    fn push_text(&self, code: u32, out: &mut String) -> bool {
        if self.to_unicode.push(code, out) {
            return true;
        }
        let c = match self.kind {
            FontKind::Type0 => {
                if self.ucs2_direct {
                    char::from_u32(code)
                } else {
                    None
                }
            }
            _ => self.encoding.get(code as usize).copied().filter(|c| *c != '\0'),
        };
        match c {
            Some(c) => {
                push_expanded_char(out, c);
                true
            }
            None => false,
        }
    }

    /// Advance width in text space units (1/1000 em).
    fn width(&self, code: u32) -> f32 {
        match self.kind {
            FontKind::Type0 => {
                let cid = self.cid_of(code);
                let idx = self.cid_widths.partition_point(|r| r.0 <= cid);
                for r in self.cid_widths[..idx].iter().rev().take(32) {
                    if cid <= r.1 {
                        return r.2 as f32;
                    }
                }
                self.dw as f32
            }
            _ => {
                if self.has_widths {
                    if code >= self.first_char {
                        if let Some(w) = self.widths.get((code - self.first_char) as usize) {
                            return *w as f32 * self.type3_scale;
                        }
                    }
                    return self.missing_width as f32 * self.type3_scale;
                }
                if let Some(w) = self.fixed_width {
                    return w as f32;
                }
                if let Some(t) = self.std_widths {
                    if (32..=126).contains(&code) {
                        return t[(code - 32) as usize] as f32;
                    }
                    return 500.0;
                }
                if self.missing_width > 0 {
                    return self.missing_width as f32;
                }
                500.0
            }
        }
    }
}

/// Whether a font name is a monospaced face (Courier, TeX typewriter, coding fonts…).
fn mono_name(lname: &str) -> bool {
    ["courier", "mono", "cmtt", "typewriter", "consol", "menlo", "monaco", "inconsolata", "sourcecode", "lettergothic", "luximono"]
        .iter()
        .any(|n| lname.contains(n))
}

fn load_font<R: ReadAt>(doc: &mut Document<'_, R>, f: &Obj) -> Font {
    let subtype = doc.get_key(f, "Subtype").ok().and_then(|o| o.name().map(String::from)).unwrap_or_default();
    let base_font = doc.get_key(f, "BaseFont").ok().and_then(|o| o.name().map(String::from)).unwrap_or_default();
    let mut font = Font {
        kind: match subtype.as_str() {
            "Type0" => FontKind::Type0,
            "Type3" => FontKind::Type3,
            _ => FontKind::Simple,
        },
        codespace: Vec::new(),
        cid: Vec::new(),
        ucs2_direct: false,
        to_unicode: ToUnicode::default(),
        encoding: Vec::new(),
        first_char: 0,
        widths: Vec::new(),
        has_widths: false,
        missing_width: 0,
        std_widths: None,
        fixed_width: None,
        cid_widths: Vec::new(),
        dw: 1000,
        type3_scale: 1.0,
        flags: 0,
    };
    // ToUnicode.
    if let Ok(tu) = doc.get_key(f, "ToUnicode") {
        if matches!(tu, Obj::Stream { .. }) {
            if let Ok(data) = doc.stream_data(&tu) {
                font.to_unicode = parse_cmap(&data).to_unicode;
            }
        }
    }
    let lname = base_font.to_ascii_lowercase();
    let name_bold = lname.contains("bold") || lname.contains("black") || lname.contains("heavy") || lname.contains("semibold");
    let name_italic = lname.contains("italic") || lname.contains("oblique");
    let name_mono = mono_name(&lname);

    let descriptor_of = |doc: &mut Document<'_, R>, d: &Obj| -> (u32, u16, f32, bool) {
        let fd = doc.get_key(d, "FontDescriptor").unwrap_or(Obj::Null);
        let flags = doc.get_key(&fd, "Flags").ok().and_then(|o| o.int()).unwrap_or(0) as u32;
        let mw = doc.get_key(&fd, "MissingWidth").ok().and_then(|o| o.int()).unwrap_or(0).clamp(0, 5000) as u16;
        let stemv = doc.get_key(&fd, "StemV").ok().and_then(|o| o.num()).unwrap_or(0.0);
        (flags, mw, stemv, fd != Obj::Null)
    };

    match font.kind {
        FontKind::Type0 => {
            let enc = doc.get_key(f, "Encoding").unwrap_or(Obj::Null);
            match &enc {
                Obj::Name(n) => {
                    if n.contains("UCS2") || n.contains("UTF16") {
                        font.ucs2_direct = true;
                    }
                }
                Obj::Stream { .. } => {
                    if let Ok(data) = doc.stream_data(&enc) {
                        let cm = parse_cmap(&data);
                        font.codespace = cm.codespace;
                        font.cid = cm.cid;
                    }
                }
                _ => {}
            }
            let desc = doc.get_key(f, "DescendantFonts").unwrap_or(Obj::Null);
            let df = match desc.array().and_then(|a| a.first()) {
                Some(d) => doc.resolve(d).unwrap_or(Obj::Null),
                None => Obj::Null,
            };
            let (flags, mw, stemv, _) = descriptor_of(doc, &df);
            font.missing_width = mw;
            font.dw = doc.get_key(&df, "DW").ok().and_then(|o| o.int()).unwrap_or(1000).clamp(0, 5000) as u16;
            if let Ok(Obj::Array(w)) = doc.get_key(&df, "W") {
                let w: Vec<Obj> = w.iter().map(|o| doc.resolve(o).unwrap_or(Obj::Null)).collect();
                let mut i = 0;
                while i < w.len() && font.cid_widths.len() < CID_WIDTHS {
                    let Some(c) = w[i].int() else { break };
                    let c = c.clamp(0, u32::MAX as i64) as u32;
                    match w.get(i + 1) {
                        Some(Obj::Array(ws)) => {
                            // Consecutive equal widths collapse into one range.
                            for (k, x) in ws.iter().enumerate().take(4096) {
                                let width = x.num().unwrap_or(0.0).clamp(0.0, 5000.0) as u16;
                                let cid = c.saturating_add(k as u32);
                                match font.cid_widths.last_mut() {
                                    Some(last) if last.2 == width && last.1 + 1 == cid => last.1 = cid,
                                    _ => font.cid_widths.push((cid, cid, width)),
                                }
                                if font.cid_widths.len() >= CID_WIDTHS {
                                    break;
                                }
                            }
                            i += 2;
                        }
                        Some(o) => {
                            let c2 = o.int().unwrap_or(c as i64).clamp(0, u32::MAX as i64) as u32;
                            let width = w.get(i + 2).and_then(|x| x.num()).unwrap_or(0.0).clamp(0.0, 5000.0) as u16;
                            font.cid_widths.push((c, c2.max(c), width));
                            i += 3;
                        }
                        None => break,
                    }
                }
                font.cid_widths.sort_by_key(|r| r.0);
            }
            font.flags = style_flags(flags, stemv, name_bold, name_italic, name_mono);
        }
        FontKind::Simple | FontKind::Type3 => {
            let (flags, mw, stemv, has_desc) = descriptor_of(doc, f);
            font.missing_width = mw;
            font.flags = style_flags(flags, stemv, name_bold, name_italic, name_mono);
            font.first_char = doc.get_key(f, "FirstChar").ok().and_then(|o| o.int()).unwrap_or(0).clamp(0, 255) as u32;
            if let Ok(Obj::Array(w)) = doc.get_key(f, "Widths") {
                if !w.is_empty() {
                    font.has_widths = true;
                    font.widths = w
                        .iter()
                        .take(256)
                        .map(|o| doc.resolve(o).ok().and_then(|o| o.num()).unwrap_or(0.0).clamp(0.0, 5000.0) as u16)
                        .collect();
                    // A face whose glyphs all share one width is monospaced whatever
                    // its name or flags say (TeX's cmtt, for one).
                    let mut first = 0u16;
                    let mut n = 0;
                    let mut same = true;
                    for &w in &font.widths {
                        if w == 0 {
                            continue;
                        }
                        if n == 0 {
                            first = w;
                        } else if w != first {
                            same = false;
                            break;
                        }
                        n += 1;
                    }
                    if same && n >= 16 && font.kind != FontKind::Type3 {
                        font.flags |= style::MONO;
                    }
                }
            }
            if font.kind == FontKind::Type3 {
                if let Ok(Obj::Array(m)) = doc.get_key(f, "FontMatrix") {
                    if let Some(mat) = Mat::from_obj(&m) {
                        font.type3_scale = mag(mat.a, mat.b) * 1000.0;
                    }
                }
            } else if !font.has_widths {
                if name_mono {
                    font.fixed_width = Some(600);
                } else if lname.contains("times") || lname.contains("georgia") || lname.contains("garamond") || lname.contains("serif") {
                    font.std_widths = Some(&TIMES_W);
                } else if flags & 1 != 0 {
                    font.fixed_width = Some(600);
                } else if flags & 2 != 0 && has_desc {
                    font.std_widths = Some(&TIMES_W);
                } else {
                    font.std_widths = Some(&HELVETICA_W);
                }
            }
            // Encoding.
            let symbolic = flags & 4 != 0 && flags & 32 == 0;
            let mut base = if lname.contains("symbol") || lname.contains("dingbat") { None } else { Some(BaseEnc::Standard) };
            let enc = doc.get_key(f, "Encoding").unwrap_or(Obj::Null);
            let mut diffs: Vec<(u8, String)> = Vec::new();
            let name_enc = |n: &str| match n {
                "WinAnsiEncoding" => Some(BaseEnc::WinAnsi),
                "MacRomanEncoding" => Some(BaseEnc::MacRoman),
                "StandardEncoding" | "MacExpertEncoding" => Some(BaseEnc::Standard),
                _ => None,
            };
            match &enc {
                Obj::Name(n) => {
                    if let Some(b) = name_enc(n) {
                        base = Some(b);
                    }
                }
                Obj::Dict(_) => {
                    if let Ok(Obj::Name(n)) = doc.get_key(&enc, "BaseEncoding") {
                        if let Some(b) = name_enc(&n) {
                            base = Some(b);
                        }
                    } else if symbolic && subtype == "TrueType" && base.is_some() {
                        base = Some(BaseEnc::Standard);
                    }
                    if let Ok(Obj::Array(d)) = doc.get_key(&enc, "Differences") {
                        let mut code = 0i64;
                        for o in d.iter().take(1024) {
                            match o {
                                Obj::Int(i) => code = *i,
                                Obj::Real(r) => code = *r as i64,
                                Obj::Name(n) => {
                                    if (0..256).contains(&code) {
                                        diffs.push((code as u8, n.clone()));
                                    }
                                    code += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            let mut table = alloc::vec!['\0'; 256];
            if let Some(b) = base {
                for (c, slot) in table.iter_mut().enumerate() {
                    if let Some(ch) = base_char(b, c as u8) {
                        *slot = ch;
                    }
                }
            }
            for (c, n) in diffs {
                table[c as usize] = glyph_char(&n).unwrap_or('\0');
            }
            font.encoding = table;
        }
    }
    font
}

fn style_flags(flags: u32, stemv: f32, name_bold: bool, name_italic: bool, name_mono: bool) -> u8 {
    let mut s = 0u8;
    if name_bold || flags & (1 << 18) != 0 || stemv >= 120.0 {
        s |= style::BOLD;
    }
    if name_italic || flags & (1 << 6) != 0 {
        s |= style::ITALIC;
    }
    if name_mono || flags & 1 != 0 {
        s |= style::MONO;
    }
    s
}

struct FontCache {
    /// (font object number, font), least recently used first.
    entries: Vec<(u32, Rc<Font>)>,
}

impl FontCache {
    /// The font `name` refers to in an already resolved `/Font` resource dictionary.
    fn get<R: ReadAt>(&mut self, doc: &mut Document<'_, R>, fonts: &Obj, name: &str) -> Option<Rc<Font>> {
        let entry = fonts.get(name)?;
        if let Obj::Ref(n, _) = entry {
            let n = *n;
            if let Some(i) = self.entries.iter().position(|(k, _)| *k == n) {
                let e = self.entries.remove(i);
                let f = e.1.clone();
                self.entries.push(e);
                return Some(f);
            }
            let fobj = doc.resolve(entry).ok()?;
            let f = Rc::new(load_font(doc, &fobj));
            if self.entries.len() >= FONT_CACHE {
                self.entries.remove(0);
            }
            self.entries.push((n, f.clone()));
            Some(f)
        } else {
            Some(Rc::new(load_font(doc, entry)))
        }
    }
}

// ---------------------------------------------------------------------------------------
// Content stream interpretation.

#[derive(Clone)]
struct TextState {
    font: Option<Rc<Font>>,
    size: f32,
    char_sp: f32,
    word_sp: f32,
    hscale: f32,
    leading: f32,
    rise: f32,
    render: i32,
}

#[derive(Clone)]
struct GState {
    ctm: Mat,
    ts: TextState,
}

enum ItemKind {
    /// A run of text: a range of the page's text arena.
    Text { start: u32, len: u32 },
    /// An image XObject drawn at the item's box, as the reference from the resources.
    Image(Obj),
}

struct Item {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    /// Baseline (text) or bottom (image).
    base: f32,
    size: f32,
    flags: u8,
    invisible: bool,
    kind: ItemKind,
}

impl Item {
    fn text<'t>(&self, arena: &'t str) -> &'t str {
        match self.kind {
            ItemKind::Text { start, len } => &arena[start as usize..(start + len) as usize],
            ItemKind::Image(_) => "",
        }
    }
}

/// The resource sub-dictionaries a content stream refers to, resolved once per stream
/// rather than on every `Tf`, `Do` and `gs`.
struct Res {
    resources: Obj,
    fonts: Option<Obj>,
    xobjects: Option<Obj>,
    ext_gstates: Option<Obj>,
}

impl Res {
    fn new(resources: Obj) -> Res {
        Res { resources, fonts: None, xobjects: None, ext_gstates: None }
    }
    fn fonts<R: ReadAt>(&mut self, doc: &mut Document<'_, R>) -> &Obj {
        self.fonts.get_or_insert_with(|| doc.get_key(&self.resources, "Font").unwrap_or(Obj::Null))
    }
    fn xobjects<R: ReadAt>(&mut self, doc: &mut Document<'_, R>) -> &Obj {
        self.xobjects.get_or_insert_with(|| doc.get_key(&self.resources, "XObject").unwrap_or(Obj::Null))
    }
    fn ext_gstates<R: ReadAt>(&mut self, doc: &mut Document<'_, R>) -> &Obj {
        self.ext_gstates.get_or_insert_with(|| doc.get_key(&self.resources, "ExtGState").unwrap_or(Obj::Null))
    }
}

struct Interp<'d, 'a, R: ReadAt> {
    doc: &'d mut Document<'a, R>,
    fonts: &'d mut FontCache,
    items: Vec<Item>,
    /// Text of every item on the page, one arena rather than a string per item.
    text: String,
    ops: u32,
}

impl<R: ReadAt> Interp<'_, '_, R> {
    fn run(&mut self, data: &[u8], res: &mut Res, init: GState, depth: u32) {
        let mut lx = Lexer::new(data, 0);
        let mut stack: Vec<GState> = Vec::new();
        let mut gs = init;
        let mut tm = Mat::ID;
        let mut tlm = Mat::ID;
        let mut args: Vec<Obj> = Vec::new();
        loop {
            if self.ops >= OP_BUDGET || self.items.len() >= ITEM_LIMIT {
                return;
            }
            let t = lx.next_tok();
            let op = match t {
                Tok::Eof => return,
                Tok::Obj(o) => {
                    if args.len() < 32 {
                        args.push(o);
                    }
                    continue;
                }
                Tok::ArrOpen => {
                    lx.pos -= 1;
                    match lx.parse_obj(0) {
                        Ok(o) => {
                            if args.len() < 32 {
                                args.push(o);
                            }
                        }
                        Err(_) => return,
                    }
                    continue;
                }
                Tok::DictOpen => {
                    lx.pos -= 2;
                    match lx.parse_obj(0) {
                        Ok(o) => {
                            if args.len() < 32 {
                                args.push(o);
                            }
                        }
                        Err(_) => return,
                    }
                    continue;
                }
                Tok::ArrClose | Tok::DictClose => continue,
                Tok::Kw(k) => k,
            };
            self.ops += 1;
            let num = |i: usize| -> f32 { args.get(i).and_then(|o| o.num()).map(finite).unwrap_or(0.0) };
            match op {
                "q" => {
                    if stack.len() < 64 {
                        stack.push(gs.clone());
                    }
                }
                "Q" => {
                    if let Some(g) = stack.pop() {
                        gs = g;
                    }
                }
                "cm" => {
                    if let Some(m) = Mat::from_obj(&args) {
                        gs.ctm = m.mul(gs.ctm);
                    }
                }
                "BT" => {
                    tm = Mat::ID;
                    tlm = Mat::ID;
                }
                "ET" => {}
                "Tf" => {
                    gs.ts.size = num(1);
                    if let Some(Obj::Name(n)) = args.first_mut() {
                        let n = core::mem::take(n);
                        let fonts = res.fonts(self.doc);
                        gs.ts.font = self.fonts.get(self.doc, fonts, &n);
                    }
                }
                "Td" => {
                    tlm = Mat::translate(num(0), num(1)).mul(tlm);
                    tm = tlm;
                }
                "TD" => {
                    gs.ts.leading = -num(1);
                    tlm = Mat::translate(num(0), num(1)).mul(tlm);
                    tm = tlm;
                }
                "Tm" => {
                    if let Some(m) = Mat::from_obj(&args) {
                        tlm = m;
                        tm = m;
                    }
                }
                "T*" => {
                    tlm = Mat::translate(0.0, -gs.ts.leading).mul(tlm);
                    tm = tlm;
                }
                "TL" => gs.ts.leading = num(0),
                "Tc" => gs.ts.char_sp = num(0),
                "Tw" => gs.ts.word_sp = num(0),
                "Tz" => gs.ts.hscale = num(0) / 100.0,
                "Ts" => gs.ts.rise = num(0),
                "Tr" => gs.ts.render = num(0) as i32,
                "Tj" => {
                    if let Some(Obj::Str(s)) = args.first_mut() {
                        let s = core::mem::take(s);
                        self.show(&s, &gs, &mut tm);
                    }
                }
                "'" => {
                    tlm = Mat::translate(0.0, -gs.ts.leading).mul(tlm);
                    tm = tlm;
                    if let Some(Obj::Str(s)) = args.first_mut() {
                        let s = core::mem::take(s);
                        self.show(&s, &gs, &mut tm);
                    }
                }
                "\"" => {
                    gs.ts.word_sp = num(0);
                    gs.ts.char_sp = num(1);
                    tlm = Mat::translate(0.0, -gs.ts.leading).mul(tlm);
                    tm = tlm;
                    if let Some(Obj::Str(s)) = args.get_mut(2) {
                        let s = core::mem::take(s);
                        self.show(&s, &gs, &mut tm);
                    }
                }
                "TJ" => {
                    if let Some(Obj::Array(a)) = args.first_mut() {
                        let a = core::mem::take(a);
                        for el in a {
                            match el {
                                Obj::Str(s) => self.show(&s, &gs, &mut tm),
                                o => {
                                    if let Some(n) = o.num() {
                                        let tx = -finite(n) / 1000.0 * gs.ts.size * gs.ts.hscale;
                                        tm = Mat::translate(tx, 0.0).mul(tm);
                                    }
                                }
                            }
                        }
                    }
                }
                "Do" => {
                    if let Some(Obj::Name(n)) = args.first_mut() {
                        let n = core::mem::take(n);
                        self.do_xobject(&n, res, &gs, depth);
                    }
                }
                "BI" => {
                    // Inline image: skip to EI.
                    loop {
                        match lx.next_tok() {
                            Tok::Kw("ID") => break,
                            Tok::Eof => return,
                            _ => {}
                        }
                    }
                    let b = lx.b;
                    let mut p = lx.pos + 1;
                    while p + 1 < b.len() {
                        if b[p] == b'E' && b[p + 1] == b'I' && (p + 2 >= b.len() || super::is_ws(b[p + 2])) && super::is_ws(b[p - 1]) {
                            break;
                        }
                        p += 1;
                    }
                    lx.pos = (p + 2).min(b.len());
                }
                "gs" => {
                    if let Some(Obj::Name(n)) = args.first() {
                        let g = res.ext_gstates(self.doc).get(n).cloned();
                        if let Some(g) = g {
                            if let Ok(g) = self.doc.resolve(&g) {
                                if let Ok(Obj::Array(fa)) = self.doc.get_key(&g, "Font") {
                                    if let (Some(fref), Some(size)) = (fa.first(), fa.get(1).and_then(|o| o.num())) {
                                        if let Ok(fobj) = self.doc.resolve(fref) {
                                            gs.ts.font = Some(Rc::new(load_font(self.doc, &fobj)));
                                            gs.ts.size = size;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            args.clear();
        }
    }

    fn show(&mut self, s: &[u8], gs: &GState, tm: &mut Mat) {
        let Some(font) = gs.ts.font.as_ref() else { return };
        let ts = &gs.ts;
        let invisible = ts.render == 3 || ts.render == 7;
        let scale = Mat { a: ts.size * ts.hscale, b: 0.0, c: 0.0, d: ts.size, e: 0.0, f: ts.rise };
        for (code, nbytes) in font.codes(s) {
            let w0 = font.width(code) / 1000.0;
            let trm = scale.mul(*tm).mul(gs.ctm);
            let (gx, gy) = (trm.e, trm.f);
            let fsize = mag(trm.b, trm.d).max(0.01);
            let adv_text = (w0 * ts.size + ts.char_sp + if code == 32 && nbytes == 1 { ts.word_sp } else { 0.0 }) * ts.hscale;
            let glyph_w_dev = w0 * mag(trm.a, trm.c);
            // The glyph's text goes straight into the arena; it is dropped again when
            // it cannot be attached to a run.
            let start = self.text.len();
            if start < TEXT_LIMIT {
                font.push_text(code, &mut self.text);
            }
            let added = (self.text.len() - start) as u32;
            // Extend the open run or start a new one.
            let mut appended = false;
            if let Some(Item { x1, y1, base, size, flags, invisible: inv, kind: ItemKind::Text { start: s0, len }, .. }) =
                self.items.last_mut()
            {
                let same_line = fabs(*base - gy) < 0.15 * fsize;
                let gap = gx - *x1;
                let close = gap > -0.5 * fsize && gap < 0.12 * fsize;
                if same_line && close && *inv == invisible && *flags == font.flags && fabs(*size - fsize) < 0.5 {
                    if (*s0 + *len) as usize == start && *len < 4096 {
                        *len += added;
                    } else {
                        self.text.truncate(start);
                    }
                    *x1 = (gx + glyph_w_dev).max(*x1);
                    *y1 = (gy + 0.75 * fsize).max(*y1);
                    appended = true;
                }
            }
            if !appended {
                let blank = self.text[start..].trim().is_empty();
                if blank && self.items.is_empty() {
                    self.text.truncate(start);
                } else {
                    self.items.push(Item {
                        x0: gx,
                        y0: gy - 0.25 * fsize,
                        x1: gx + glyph_w_dev,
                        y1: gy + 0.75 * fsize,
                        base: gy,
                        size: fsize,
                        flags: font.flags,
                        invisible,
                        kind: ItemKind::Text { start: start as u32, len: added },
                    });
                }
            }
            *tm = Mat::translate(adv_text, 0.0).mul(*tm);
            if self.items.len() >= ITEM_LIMIT {
                return;
            }
        }
    }

    fn do_xobject(&mut self, name: &str, res: &mut Res, gs: &GState, depth: u32) {
        let Some(entry) = res.xobjects(self.doc).get(name).cloned() else { return };
        let Ok(x) = self.doc.resolve(&entry) else { return };
        if !matches!(x, Obj::Stream { .. }) {
            return;
        }
        let sub = x.get("Subtype").and_then(|s| s.name()).unwrap_or("");
        if sub == "Image" {
            let (ax, ay) = gs.ctm.apply(0.0, 0.0);
            let (bx, by) = gs.ctm.apply(1.0, 0.0);
            let (cx, cy) = gs.ctm.apply(0.0, 1.0);
            let (dx, dy) = gs.ctm.apply(1.0, 1.0);
            let x0 = ax.min(bx).min(cx).min(dx);
            let x1 = ax.max(bx).max(cx).max(dx);
            let y0 = ay.min(by).min(cy).min(dy);
            let y1 = ay.max(by).max(cy).max(dy);
            if x1 - x0 < 4.0 || y1 - y0 < 4.0 {
                return;
            }
            // Keep the reference, not the stream dictionary: it is resolved again only
            // when the image is decoded.
            self.items.push(Item { x0, y0, x1, y1, base: y0, size: y1 - y0, flags: 0, invisible: false, kind: ItemKind::Image(entry) });
        } else if sub == "Form" && depth < 8 {
            let mut g = gs.clone();
            if let Ok(Obj::Array(m)) = self.doc.get_key(&x, "Matrix") {
                if let Some(m) = Mat::from_obj(&m) {
                    g.ctm = m.mul(g.ctm);
                }
            }
            let Ok(data) = self.doc.stream_data(&x) else { return };
            if data.len() > CONTENT_LIMIT {
                return;
            }
            match self.doc.get_key(&x, "Resources") {
                Ok(r) if r != Obj::Null => {
                    let mut own = Res::new(r);
                    self.run(&data, &mut own, g, depth + 1);
                }
                _ => self.run(&data, res, g, depth + 1),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Layout recovery: XY-cut into blocks, lines, paragraphs.

struct Line {
    x0: f32,
    x1: f32,
    base: f32,
    size: f32,
    /// (flags, text) segments.
    segs: Vec<(u8, String)>,
    /// An image XObject reference.
    image: Option<Obj>,
    img_box: (f32, f32),
}

impl Line {
    /// Whether every run of the line is in a monospaced face (a code listing).
    fn mono(&self) -> bool {
        !self.segs.is_empty() && self.segs.iter().all(|s| s.0 & style::MONO != 0)
    }
}

struct Block {
    lines: Vec<Line>,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
}

fn gaps(intervals: &mut [(f32, f32)]) -> Vec<(f32, f32)> {
    intervals.sort_by(|a, b| fcmp(a.0, b.0));
    let mut out = Vec::new();
    let mut hi = match intervals.first() {
        Some(i) => i.1,
        None => return out,
    };
    for &(lo, h) in intervals.iter().skip(1) {
        if lo > hi + 0.01 {
            out.push((hi, lo));
        }
        if h > hi {
            hi = h;
        }
    }
    out
}

fn xy_cut(items: &[Item], text: &str, idx: Vec<usize>, body: f32, depth: u32, out: &mut Vec<Block>) {
    if idx.is_empty() {
        return;
    }
    if idx.len() > 1 && depth < 40 {
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &i in &idx {
            let it = &items[i];
            x0 = x0.min(it.x0);
            x1 = x1.max(it.x1);
            y0 = y0.min(it.y0);
            y1 = y1.max(it.y1);
        }
        // Vertical gaps (columns): both sides must be wide enough to be columns.
        let mut xs: Vec<(f32, f32)> = idx.iter().map(|&i| (items[i].x0, items[i].x1)).collect();
        let vgaps = gaps(&mut xs);
        let mut best_v: Option<(f32, f32)> = None;
        for g in vgaps {
            let w = g.1 - g.0;
            if w >= 1.0 * body && g.0 - x0 >= 6.0 * body && x1 - g.1 >= 6.0 * body && best_v.map(|b| w > b.1 - b.0).unwrap_or(true) {
                best_v = Some(g);
            }
        }
        if let Some(g) = best_v {
            let mid = (g.0 + g.1) * 0.5;
            let (l, r): (Vec<usize>, Vec<usize>) = idx.iter().partition(|&&i| items[i].x1 <= mid);
            // A real gutter separates several lines on each side; a run-in heading or a
            // short table does not.
            let baselines = |side: &[usize]| -> usize {
                let mut b: Vec<f32> = side.iter().map(|&i| items[i].base).collect();
                b.sort_by(|a, c| fcmp(*a, *c));
                let mut n = 0;
                let mut last = f32::MIN;
                for v in b {
                    if v - last > 0.5 * body {
                        n += 1;
                        last = v;
                    }
                }
                n
            };
            if baselines(&l) >= 3 && baselines(&r) >= 3 {
                xy_cut(items, text, l, body, depth + 1, out);
                xy_cut(items, text, r, body, depth + 1, out);
                return;
            }
        }
        // Horizontal gaps (blocks): the largest wins. A gap must be wide relative to the
        // body size and to the lines on either side of it, so a large heading's own line
        // spacing does not split it.
        let mut ys: Vec<(f32, f32, f32)> = idx.iter().map(|&i| (items[i].y0, items[i].y1, items[i].size)).collect();
        ys.sort_by(|a, b| fcmp(a.0, b.0));
        let mut best_h: Option<(f32, f32)> = None;
        if let Some(first) = ys.first() {
            let (mut hi, mut hi_size) = (first.1, first.2);
            for &(lo, h, size) in ys.iter().skip(1) {
                if lo > hi + 0.01 {
                    let gap = lo - hi;
                    if gap >= 0.45 * body && gap >= 0.3 * hi_size.max(size) && best_h.map(|b| gap > b.1 - b.0).unwrap_or(true) {
                        best_h = Some((hi, lo));
                    }
                }
                if h > hi {
                    hi = h;
                    hi_size = size;
                }
            }
        }
        if let Some(g) = best_h {
            let mid = (g.0 + g.1) * 0.5;
            let (top, bottom): (Vec<usize>, Vec<usize>) = idx.iter().partition(|&&i| items[i].y0 >= mid);
            if !top.is_empty() && !bottom.is_empty() {
                xy_cut(items, text, top, body, depth + 1, out);
                xy_cut(items, text, bottom, body, depth + 1, out);
                return;
            }
        }
    }
    out.push(leaf(items, text, idx));
}

/// Punctuation that closes a word: no space is inserted before it even after a gap (a
/// dropped superscript marker leaves one between a name and its comma). Colons and the
/// like are left alone: code (`x := y`) and some languages space them.
fn closes_word(c: char) -> bool {
    matches!(c, ',' | '.' | ';' | ')' | ']' | '’' | '”')
}

fn leaf(items: &[Item], text: &str, mut idx: Vec<usize>) -> Block {
    idx.sort_by(|&a, &b| fcmp(items[b].base, items[a].base));
    let mut lines: Vec<Line> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for &i in &idx {
        let it = &items[i];
        if matches!(it.kind, ItemKind::Image(_)) {
            groups.push(alloc::vec![i]);
            continue;
        }
        let mut placed = false;
        if let Some(g) = groups.last_mut() {
            // Compare against the largest item of the group (the line's main text).
            let main = g.iter().map(|&j| &items[j]).max_by(|a, b| fcmp(a.size, b.size)).unwrap_or(&items[g[0]]);
            if matches!(main.kind, ItemKind::Text { .. }) {
                let same = fabs(main.base - it.base) < 0.4 * main.size.min(it.size);
                let (small, big) = if it.size < main.size { (it, main) } else { (main, it) };
                let script =
                    small.size < 0.75 * big.size && small.base > big.base - 0.35 * big.size && small.base - big.base < 0.7 * big.size;
                if same || script {
                    g.push(i);
                    placed = true;
                }
            }
        }
        if !placed {
            groups.push(alloc::vec![i]);
        }
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for mut g in groups {
        g.sort_by(|&a, &b| fcmp(items[a].x0, items[b].x0));
        let first = &items[g[0]];
        if let ItemKind::Image(o) = &first.kind {
            x0 = x0.min(first.x0);
            x1 = x1.max(first.x1);
            y0 = y0.min(first.y0);
            y1 = y1.max(first.y1);
            lines.push(Line {
                x0: first.x0,
                x1: first.x1,
                base: first.base,
                size: first.size,
                segs: Vec::new(),
                image: Some(o.clone()),
                img_box: (first.x1 - first.x0, first.y1 - first.y0),
            });
            continue;
        }
        let main = g.iter().map(|&j| &items[j]).max_by(|a, b| fcmp(a.size, b.size)).unwrap_or(first);
        let (main_base, main_size) = (main.base, main.size);
        let mut line = Line { x0: f32::MAX, x1: f32::MIN, base: main_base, size: 0.0, segs: Vec::new(), image: None, img_box: (0.0, 0.0) };
        let mut prev_x1 = f32::MIN;
        for &i in &g {
            let it = &items[i];
            if !matches!(it.kind, ItemKind::Text { .. }) {
                continue;
            }
            let mut flags = it.flags;
            if it.size < 0.75 * main_size {
                if it.base > main_base + 0.15 * main_size {
                    flags |= style::SUP;
                } else if it.base < main_base - 0.15 * main_size {
                    flags |= style::SUB;
                }
            }
            let gap = it.x0 - prev_x1;
            let need_space = prev_x1 > f32::MIN && gap > 0.12 * it.size.min(line.size.max(it.size));
            let mut t = it.text(text);
            if need_space {
                t = t.trim_start();
            }
            if t.is_empty() {
                prev_x1 = prev_x1.max(it.x1);
                continue;
            }
            let ends_space = line.segs.last().map(|s| s.1.ends_with(' ')).unwrap_or(true);
            let starts_space = t.starts_with(' ');
            let punct = flags & style::MONO == 0 && t.chars().next().map(closes_word).unwrap_or(false);
            if need_space && !ends_space && !starts_space && !punct {
                if let Some(last) = line.segs.last_mut() {
                    last.1.push(' ');
                }
            }
            match line.segs.last_mut() {
                Some(last) if last.0 == flags => last.1.push_str(t),
                _ => line.segs.push((flags, String::from(t))),
            }
            line.x0 = line.x0.min(it.x0);
            line.x1 = line.x1.max(it.x1);
            line.size = line.size.max(it.size);
            prev_x1 = prev_x1.max(it.x1);
        }
        if line.segs.is_empty() {
            continue;
        }
        for s in &mut line.segs {
            // Collapse runs of spaces.
            if s.1.contains("  ") {
                let mut c = String::with_capacity(s.1.len());
                let mut sp = false;
                for ch in s.1.chars() {
                    if ch == ' ' || ch == '\u{a0}' {
                        if !sp {
                            c.push(' ');
                        }
                        sp = true;
                    } else {
                        sp = false;
                        c.push(ch);
                    }
                }
                s.1 = c;
            }
            compose_accents(&mut s.1);
        }
        x0 = x0.min(line.x0);
        x1 = x1.max(line.x1);
        y0 = y0.min(line.base - 0.25 * line.size);
        y1 = y1.max(line.base + 0.75 * line.size);
        lines.push(line);
    }
    Block { lines, x0, x1, y0, y1 }
}

// ---------------------------------------------------------------------------------------
// Accents: TeX-style PDFs draw a spacing accent glyph over a base letter, so the text
// comes out as "na¨ıve". The common Latin accents are composed back.

/// (spacing accent, combining mark, bases, precomposed letters in the same order).
const ACCENTS: &[(char, char, &str, &str)] = &[
    ('´', '\u{301}', "aeiouycnszlrAEIOUYCNSZLR", "áéíóúýćńśźĺŕÁÉÍÓÚÝĆŃŚŹĹŔ"),
    ('`', '\u{300}', "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
    ('ˆ', '\u{302}', "aeiouAEIOUcghjswyCGHJSWY", "âêîôûÂÊÎÔÛĉĝĥĵŝŵŷĈĜĤĴŜŴŶ"),
    ('˜', '\u{303}', "anoAiuNOIU", "ãñõÃĩũÑÕĨŨ"),
    ('¨', '\u{308}', "aeiouyAEIOUYı", "äëïöüÿÄËÏÖÜŸï"),
    ('¸', '\u{327}', "cCsStTgGkKlLnNrR", "çÇşŞţŢģĢķĶļĻņŅŗŖ"),
    ('ˇ', '\u{30c}', "cdenrstzCDENRSTZaiouAIOU", "čďěňřšťžČĎĚŇŘŠŤŽǎǐǒǔǍǏǑǓ"),
    ('˚', '\u{30a}', "auAU", "åůÅŮ"),
    ('¯', '\u{304}', "aeiouAEIOU", "āēīōūĀĒĪŌŪ"),
];

fn composed(accent: char, base: char) -> Option<char> {
    let (_, _, bases, out) = ACCENTS.iter().find(|a| a.0 == accent || a.1 == accent)?;
    let i = bases.chars().position(|b| b == base)?;
    out.chars().nth(i)
}

/// Compose "accent + letter" (TeX order) and "letter + combining mark" pairs in place.
/// Only runs when the text holds an accent character at all.
fn compose_accents(s: &mut String) {
    let has = s.bytes().any(|b| b == b'`' || b >= 0x80) && s.chars().any(|c| ACCENTS.iter().any(|a| a.0 == c || a.1 == c));
    if !has {
        return;
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    let mut prev: Option<char> = None;
    while let Some(c) = it.next() {
        let spacing = ACCENTS.iter().any(|a| a.0 == c);
        if spacing {
            if let Some(&next) = it.peek() {
                // A bare ASCII grave is an accent only inside a word.
                let plausible = c != '`' || prev.map(|p| p.is_alphabetic()).unwrap_or(false);
                if plausible {
                    if let Some(k) = composed(c, next) {
                        it.next();
                        out.push(k);
                        prev = Some(k);
                        continue;
                    }
                }
            }
        } else if ACCENTS.iter().any(|a| a.1 == c) {
            if let Some(p) = out.pop() {
                match composed(c, p) {
                    Some(k) => out.push(k),
                    None => out.push(p),
                }
                prev = out.chars().last();
                continue;
            }
        }
        out.push(c);
        prev = Some(c);
    }
    *s = out;
}

// ---------------------------------------------------------------------------------------
// Paragraphs.

/// Distinct capitalised-inside words seen on recent pages, so "Trace-" + "Monkey" at a
/// line break can be healed to "TraceMonkey" when the word occurs unhyphenated elsewhere.
/// Two generations bound the set to the last 2000–4000 words.
#[derive(Default)]
struct SeenWords {
    cur: BTreeSet<String>,
    prev: BTreeSet<String>,
}

impl SeenWords {
    const GENERATION: usize = 2000;
    fn clear(&mut self) {
        self.cur.clear();
        self.prev.clear();
    }
    fn contains(&self, w: &str) -> bool {
        self.cur.contains(w) || self.prev.contains(w)
    }
    /// Note the words of a line worth remembering: alphabetic, with an upper-case letter
    /// after the first (the only kind a capitalised continuation can complete).
    fn scan(&mut self, text: &str) {
        for tok in text.split_whitespace() {
            let w = tok.trim_matches(|c: char| !c.is_alphanumeric());
            if w.len() < 4 || w.len() > 40 || !w.chars().all(|c| c.is_alphabetic()) {
                continue;
            }
            if !w.chars().skip(1).any(|c| c.is_uppercase()) || self.contains(w) {
                continue;
            }
            if self.cur.len() >= Self::GENERATION {
                self.prev = core::mem::take(&mut self.cur);
            }
            self.cur.insert(w.into());
        }
    }
    /// Whether `head` (the word before a line-end hyphen) and `tail` (the start of the
    /// next line) are one word.
    fn joins(&self, head: &str, tail: &str) -> bool {
        let head = head.trim_end_matches(|c: char| !c.is_alphanumeric());
        let head = head.rsplit(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
        let tail = tail.trim_start();
        let tail: &str = tail.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
        if head.is_empty() || tail.is_empty() || (self.cur.is_empty() && self.prev.is_empty()) {
            return false;
        }
        let mut w = String::with_capacity(head.len() + tail.len());
        w.push_str(head);
        w.push_str(tail);
        self.contains(&w)
    }
}

/// A paragraph ready to write.
struct Para {
    kind: ParaKind,
    segs: Vec<(u8, String)>,
    /// An image reference and its box on the page.
    image: Option<(Obj, f32, f32)>,
    /// Left indent of the first line relative to the block, in ems.
    indent: f32,
    /// Whether the last line was short (ended a paragraph visually).
    ended: bool,
    /// Heading detected from boldness rather than size.
    by_bold: bool,
    /// Lines joined into this paragraph.
    lines: u16,
    /// The layout block the paragraph came from.
    block: u32,
}

impl Para {
    fn text_len(&self) -> usize {
        self.segs.iter().map(|s| s.1.len()).sum()
    }
    fn is_blank(&self) -> bool {
        self.segs.iter().all(|s| s.1.trim().is_empty())
    }
    /// The last segment with visible text.
    fn last_text(&self) -> &str {
        self.segs.iter().rev().map(|s| s.1.as_str()).find(|t| !t.trim().is_empty()).unwrap_or("")
    }
    fn first_text(&self) -> &str {
        self.segs.iter().map(|s| s.1.as_str()).find(|t| !t.trim().is_empty()).unwrap_or("")
    }
    fn starts_lower(&self) -> bool {
        self.first_text().trim_start().chars().next().map(|c| c.is_lowercase()).unwrap_or(false)
    }
    /// Face flags (bold, italic, mono) of the first and last visible segments.
    fn face(&self) -> (u8, u8) {
        const FACE: u8 = style::BOLD | style::ITALIC | style::MONO;
        let first = self.segs.iter().find(|s| !s.1.trim().is_empty()).map(|s| s.0).unwrap_or(0);
        let last = self.segs.iter().rev().find(|s| !s.1.trim().is_empty()).map(|s| s.0).unwrap_or(0);
        (first & FACE, last & FACE)
    }
    /// Whether the paragraph looks like a figure, table or listing.
    fn is_float(&self) -> bool {
        if self.image.is_some() || matches!(self.kind, ParaKind::Code | ParaKind::Caption) {
            return true;
        }
        let t = self.first_text().trim_start();
        let caption = ["Figure", "Fig.", "Table", "Listing"].iter().any(|p| t.starts_with(p));
        matches!(self.kind, ParaKind::Body) && (caption || self.text_len() <= 600)
    }
    /// Append the segments of a line, taking its strings. Code lines are separated by
    /// a break; prose lines by a space, healing an end-of-line hyphen.
    fn append_line(&mut self, line: &mut Line, words: &SeenWords) {
        if matches!(self.kind, ParaKind::Code) {
            if let Some(last) = self.segs.last_mut() {
                last.1.push('\n');
            }
        } else {
            let prev_hyphen = self.segs.last().map(|s| s.1.ends_with('-')).unwrap_or(false);
            let first = line.segs.first().map(|s| s.1.as_str()).unwrap_or("");
            let next_lower = first.chars().next().map(|c| c.is_lowercase()).unwrap_or(false);
            let joined = prev_hyphen && (next_lower || words.joins(self.last_text(), first));
            if joined {
                if let Some(last) = self.segs.last_mut() {
                    last.1.pop();
                }
            } else if let Some(last) = self.segs.last_mut() {
                if !last.1.ends_with(' ') {
                    last.1.push(' ');
                }
            }
        }
        for (f, t) in line.segs.iter_mut() {
            match self.segs.last_mut() {
                Some(last) if last.0 == *f => last.1.push_str(t),
                _ => self.segs.push((*f, core::mem::take(t))),
            }
        }
    }
    /// Append a whole paragraph (a continuation from the next block or page).
    fn append_para(&mut self, mut other: Para, words: &SeenWords) {
        let mut line =
            Line { x0: 0.0, x1: 0.0, base: 0.0, size: 0.0, segs: core::mem::take(&mut other.segs), image: None, img_box: (0.0, 0.0) };
        self.append_line(&mut line, words);
        self.ended = other.ended;
        self.lines = self.lines.saturating_add(other.lines);
    }
}

fn ends_sentence(s: &str) -> bool {
    let t = s.trim_end();
    t.ends_with(['.', '!', '?', ':', '”', '"', '’', ')', '…', ';'])
}

fn block_paras(block: &mut Block, body: f32, words: &SeenWords) -> Vec<Para> {
    let diagram = is_diagram(block);
    let mut out: Vec<Para> = Vec::new();
    let width = (block.x1 - block.x0).max(1.0);
    let (bx0, bx1) = (block.x0, block.x1);
    let n_lines = block.lines.len();
    let mut prev: Option<(f32, f32, f32, f32)> = None; // (base, size, x0, x1)
    for line in block.lines.iter_mut() {
        if let Some(img) = line.image.take() {
            out.push(Para {
                kind: ParaKind::Body,
                segs: Vec::new(),
                image: Some((img, line.img_box.0, line.img_box.1)),
                indent: 0.0,
                ended: true,
                by_bold: false,
                lines: 1,
                block: 0,
            });
            prev = None;
            continue;
        }
        let indent = (line.x0 - bx0) / line.size.max(1.0);
        let mono = line.mono();
        let mut new_para = true;
        if let (Some((pb, ps, _px0, px1)), Some(cur)) = (prev, out.last()) {
            let pitch = pb - line.base;
            let size_change = fabs(ps - line.size) > 0.15 * ps;
            let big_gap = pitch > 1.9 * line.size.max(ps);
            // Indentation starts a paragraph in body text; headings are often centred.
            let indented = matches!(cur.kind, ParaKind::Body) && indent > 0.6 && (line.x0 - bx0) > 0.6 * line.size;
            let prev_short = (bx1 - px1) > 2.0 * ps && ends_sentence(cur.last_text()) && width > 12.0 * ps;
            let line_bold = line.segs.iter().all(|s| s.0 & style::BOLD != 0);
            let heading_break = !matches!(cur.kind, ParaKind::Body) && ((cur.by_bold && !line_bold) || cur.ended);
            let code_switch = mono != matches!(cur.kind, ParaKind::Code);
            if mono && !code_switch {
                // Listing lines stay one code paragraph whatever their spacing.
                new_para = false;
            } else {
                new_para = size_change || big_gap || indented || prev_short || heading_break || cur.image.is_some() || code_switch;
            }
        }
        if new_para || out.is_empty() {
            let ratio = line.size / body.max(1.0);
            let all_bold = line.segs.iter().all(|s| s.0 & style::BOLD != 0);
            let chars: usize = line.segs.iter().map(|s| s.1.chars().count()).sum();
            let mut by_bold = false;
            let wordy = chars >= 3 && line.segs.iter().any(|s| s.1.chars().filter(|c| c.is_alphabetic()).count() >= 3);
            let kind = if mono {
                ParaKind::Code
            } else if ratio >= 1.5 && wordy {
                ParaKind::Heading(1)
            } else if ratio >= 1.25 && wordy {
                ParaKind::Heading(2)
            } else if ratio >= 1.1 && chars < 120 && wordy {
                ParaKind::Heading(3)
            } else if all_bold && wordy && chars < 90 && (bx1 - line.x1) > 1.5 * line.size && (n_lines > 1 || chars >= 12) {
                by_bold = true;
                ParaKind::Heading(3)
            } else {
                ParaKind::Body
            };
            let mut p = Para { kind, segs: Vec::new(), image: None, indent, ended: false, by_bold, lines: 1, block: 0 };
            p.append_line(line, words);
            out.push(p);
        } else if let Some(p) = out.last_mut() {
            p.append_line(line, words);
            p.lines = p.lines.saturating_add(1);
        }
        if let Some(p) = out.last_mut() {
            p.ended = (bx1 - line.x1) > 2.0 * line.size || ends_sentence(p.last_text());
        }
        prev = Some((line.base, line.size, line.x0, line.x1));
    }
    // A "heading" that runs on for several lines is body text in a slightly larger face.
    for p in &mut out {
        if matches!(p.kind, ParaKind::Heading(3)) && (p.lines > 2 || p.text_len() > 160) {
            p.kind = ParaKind::Body;
        }
    }
    // Headings never carry bold as inline style (the layout styles them).
    for p in &mut out {
        if !matches!(p.kind, ParaKind::Body | ParaKind::Code) {
            for s in &mut p.segs {
                s.0 &= !style::BOLD;
            }
        }
    }
    if diagram {
        collapse_diagram(out)
    } else {
        out
    }
}

/// Whether a block mixes tiny images (arrows, boxes) with many one-to-three-word labels:
/// a diagram we cannot reproduce.
fn is_diagram(block: &Block) -> bool {
    let specks = block.lines.iter().filter(|l| l.image.is_some() && l.img_box.0 < 100.0).count();
    let text_lines = block.lines.iter().filter(|l| l.image.is_none()).count();
    let short = block
        .lines
        .iter()
        .filter(|l| l.image.is_none() && l.segs.iter().map(|s| s.1.split_whitespace().count()).sum::<usize>() <= 3)
        .count();
    specks >= 2 && short >= 4 && short * 10 >= text_lines * 6
}

/// A diagram block becomes one caption paragraph, keeping any real caption line; the
/// specks and labels are skipped.
fn collapse_diagram(paras: Vec<Para>) -> Vec<Para> {
    let mut caption = Para {
        kind: ParaKind::Caption,
        segs: alloc::vec![(0, String::from("Figure — diagram not shown"))],
        image: None,
        indent: 0.0,
        ended: true,
        by_bold: false,
        lines: 1,
        block: 0,
    };
    for p in paras {
        let t = p.first_text().trim_start();
        if p.image.is_none() && ["Figure", "Fig.", "Table"].iter().any(|k| t.starts_with(k)) {
            if let Some(last) = caption.segs.last_mut() {
                last.1.push_str(". ");
            }
            for (f, t) in p.segs {
                caption.segs.push((f & !style::BOLD, t));
            }
        }
    }
    alloc::vec![caption]
}

/// Modal font size weighted by characters, in 0.5 pt bins.
fn body_size(items: &[Item]) -> f32 {
    let mut bins: BTreeMap<u32, u32> = BTreeMap::new();
    for it in items {
        if let ItemKind::Text { len, .. } = it.kind {
            if len > 0 {
                *bins.entry((it.size * 2.0) as u32).or_insert(0) += len;
            }
        }
    }
    bins.iter().max_by_key(|(_, n)| **n).map(|(b, _)| *b as f32 / 2.0).unwrap_or(10.0).max(3.0)
}

// ---------------------------------------------------------------------------------------
// Images.

enum Cs {
    Gray,
    Rgb,
    Cmyk,
    Indexed {
        base: alloc::boxed::Box<Cs>,
        lookup: Vec<u8>,
    },
    /// Separation / DeviceN: `n` tint components, 1 = full ink.
    Ink(usize),
    Mask,
}

impl Cs {
    fn ncomp(&self) -> usize {
        match self {
            Cs::Gray | Cs::Mask => 1,
            Cs::Rgb => 3,
            Cs::Cmyk => 4,
            Cs::Indexed { .. } => 1,
            Cs::Ink(n) => *n,
        }
    }
    fn grey(&self, c: &[u8]) -> u8 {
        match self {
            Cs::Gray | Cs::Mask => c[0],
            Cs::Rgb => ((c[0] as u32 * 77 + c[1] as u32 * 151 + c[2] as u32 * 28) >> 8) as u8,
            Cs::Cmyk => {
                let k = 255 - c[3] as u32;
                let g = 255 - ((c[0] as u32 * 77 + c[1] as u32 * 151 + c[2] as u32 * 28) >> 8);
                (g * k / 255) as u8
            }
            Cs::Indexed { base, lookup } => {
                let n = base.ncomp();
                let i = c[0] as usize * n;
                if i + n <= lookup.len() {
                    base.grey(&lookup[i..i + n])
                } else {
                    255
                }
            }
            Cs::Ink(n) => 255 - c.iter().take(*n).copied().max().unwrap_or(0),
        }
    }
}

fn colorspace<R: ReadAt>(doc: &mut Document<'_, R>, cs: &Obj, depth: u32) -> Option<Cs> {
    let cs = doc.resolve_cow(cs).ok()?;
    match &*cs {
        Obj::Name(n) => match n.as_str() {
            "DeviceGray" | "G" | "CalGray" => Some(Cs::Gray),
            "DeviceRGB" | "RGB" | "CalRGB" | "Lab" => Some(Cs::Rgb),
            "DeviceCMYK" | "CMYK" => Some(Cs::Cmyk),
            _ => None,
        },
        Obj::Array(a) if !a.is_empty() && depth < 4 => {
            let fam = doc.resolve_cow(&a[0]).ok()?;
            match fam.name().unwrap_or("") {
                "ICCBased" => {
                    let s = doc.resolve(a.get(1)?).ok()?;
                    match doc.get_key(&s, "N").ok().and_then(|o| o.int()) {
                        Some(1) => Some(Cs::Gray),
                        Some(4) => Some(Cs::Cmyk),
                        Some(3) => Some(Cs::Rgb),
                        _ => match doc.get_key(&s, "Alternate") {
                            Ok(alt) if alt != Obj::Null => colorspace(doc, &alt, depth + 1),
                            _ => Some(Cs::Rgb),
                        },
                    }
                }
                "CalRGB" | "Lab" => Some(Cs::Rgb),
                "CalGray" => Some(Cs::Gray),
                "Indexed" | "I" => {
                    let base = colorspace(doc, a.get(1)?, depth + 1)?;
                    let lk = doc.resolve(a.get(3)?).ok()?;
                    let lookup = match lk {
                        Obj::Str(s) => s,
                        Obj::Stream { .. } => doc.stream_data(&lk).ok()?,
                        _ => return None,
                    };
                    Some(Cs::Indexed { base: alloc::boxed::Box::new(base), lookup })
                }
                "Separation" => Some(Cs::Ink(1)),
                "DeviceN" => {
                    let n = doc.resolve(a.get(1)?).ok()?.array().map(|x| x.len()).unwrap_or(1).clamp(1, 8);
                    Some(Cs::Ink(n))
                }
                "DeviceGray" | "G" => Some(Cs::Gray),
                "DeviceRGB" | "RGB" => Some(Cs::Rgb),
                "DeviceCMYK" | "CMYK" => Some(Cs::Cmyk),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Convert one packed sample row to grey.
fn row_to_grey(row: &[u8], cs: &Cs, bpc: u32, w: usize, invert: bool, out: &mut [u8]) {
    let n = cs.ncomp();
    let max = (1u32 << bpc) - 1;
    let mut comps = [0u8; 8];
    let mut bitpos = 0usize;
    for out_px in out.iter_mut().take(w) {
        for c in comps.iter_mut().take(n) {
            let v: u32 = match bpc {
                8 => {
                    let i = bitpos >> 3;
                    bitpos += 8;
                    *row.get(i).unwrap_or(&0) as u32
                }
                16 => {
                    let i = bitpos >> 3;
                    bitpos += 16;
                    *row.get(i).unwrap_or(&0) as u32
                }
                _ => {
                    let i = bitpos >> 3;
                    let byte = *row.get(i).unwrap_or(&0) as u32;
                    let shift = 8 - bpc - (bitpos & 7) as u32;
                    bitpos += bpc as usize;
                    (byte >> shift) & max
                }
            };
            *c = match bpc {
                8 | 16 => v as u8,
                _ => {
                    if matches!(cs, Cs::Indexed { .. }) {
                        v as u8
                    } else {
                        (v * 255 / max) as u8
                    }
                }
            };
        }
        let mut g = cs.grey(&comps[..n]);
        if invert {
            g = 255 - g;
        }
        *out_px = g;
    }
}

/// Decode an image XObject (given by reference) to a fitted bitmap.
fn decode_image<R: ReadAt>(doc: &mut Document<'_, R>, xref: &Obj, fit: Fit) -> Result<Bitmap, DocError> {
    let x = doc.resolve(xref)?;
    let x = &x;
    let w = doc.get_key(x, "Width")?.int().unwrap_or(0);
    let h = doc.get_key(x, "Height")?.int().unwrap_or(0);
    if w <= 0 || h <= 0 || w > 20000 || h > 20000 {
        return Err(DocError::Malformed("pdf image size"));
    }
    let (w, h) = (w as u32, h as u32);
    let filters = doc.filters_of(x)?;
    let codec = filters
        .iter()
        .map(|(n, _)| n.as_str())
        .find(|n| matches!(*n, "DCTDecode" | "DCT" | "JPXDecode" | "CCITTFaxDecode" | "CCF" | "JBIG2Decode"));
    match codec {
        Some("DCTDecode") | Some("DCT") => {
            if filters.len() == 1 {
                // The JPEG is the raw stream: decode straight from the file, row by row.
                let Obj::Stream { offset, len, .. } = x else { return Err(DocError::Malformed("pdf: not a stream")) };
                let src = doc.source();
                let raw_len = (*len).min(src.len().saturating_sub(*offset));
                return crate::jpeg::decode(&quire_fs::Slice::new(src, *offset, raw_len), fit);
            }
            let bytes = doc.stream_data(x)?;
            return crate::jpeg::decode(&bytes, fit);
        }
        Some(_) => return Err(DocError::Unsupported("pdf image codec")),
        None => {}
    }
    let mask = matches!(doc.get_key(x, "ImageMask"), Ok(Obj::Bool(true)));
    let mut bpc = doc.get_key(x, "BitsPerComponent")?.int().unwrap_or(if mask { 1 } else { 8 }) as u32;
    if mask {
        bpc = 1;
    }
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return Err(DocError::Unsupported("pdf image depth"));
    }
    let cs = if mask {
        Cs::Mask
    } else {
        let cso = x.get("ColorSpace").unwrap_or(&Obj::Null);
        colorspace(doc, cso, 0).ok_or(DocError::Unsupported("pdf colour space"))?
    };
    let decode = doc.get_key(x, "Decode").unwrap_or(Obj::Null);
    let mut invert = decode.array().and_then(|a| a.first()).and_then(|o| o.num()).map(|v| v >= 0.5).unwrap_or(false);
    if matches!(cs, Cs::Indexed { .. }) {
        invert = false;
    }
    // ImageMask: sample 0 paints (black) by default.
    if mask {
        invert = !invert;
    }
    let row_bytes = (w as usize * cs.ncomp() * bpc as usize).div_ceil(8);
    let mut sink = RowSink::new(w, h, fit)?;
    let mut grey = alloc::vec![255u8; w as usize];
    let mut row_buf: Vec<u8> = Vec::with_capacity(row_bytes);
    let mut rows = 0u32;
    let mut feed = |chunk: &[u8], sink: &mut RowSink, rows: &mut u32| {
        let mut i = 0;
        while i < chunk.len() && *rows < h {
            let take = (row_bytes - row_buf.len()).min(chunk.len() - i);
            row_buf.extend_from_slice(&chunk[i..i + take]);
            i += take;
            if row_buf.len() == row_bytes {
                row_to_grey(&row_buf, &cs, bpc, w as usize, invert, &mut grey);
                sink.push_row(&grey);
                row_buf.clear();
                *rows += 1;
            }
        }
    };
    let streamable = filters.len() == 1 && matches!(filters[0].0.as_str(), "FlateDecode" | "Fl") && filters[0].1.is_none();
    if streamable {
        let Obj::Stream { offset, len, .. } = x else { return Err(DocError::Malformed("pdf: not a stream")) };
        let src = doc.source();
        let raw_len = (*len).min(src.len().saturating_sub(*offset));
        let mut inf = Inflater::new(quire_fs::Slice::new(src, *offset, raw_len), Framing::Zlib);
        loop {
            let c = inf.next_chunk()?;
            if c.is_empty() {
                break;
            }
            feed(c, &mut sink, &mut rows);
            if rows >= h {
                break;
            }
        }
    } else {
        let data = doc.stream_data(x)?;
        feed(&data, &mut sink, &mut rows);
    }
    if rows == 0 {
        return Err(DocError::Malformed("pdf image data"));
    }
    while rows < h {
        sink.push_row(&grey);
        rows += 1;
    }
    Ok(sink.finish())
}

// ---------------------------------------------------------------------------------------
// Outlines and destinations.

struct Outline {
    title: String,
    page: usize,
    depth: u8,
}

fn pdf_string(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let units: Vec<u16> = b[2..].chunks(2).map(|c| ((c[0] as u16) << 8) | *c.get(1).unwrap_or(&0) as u16).collect();
        return utf16_string(&units);
    }
    if b.len() >= 3 && b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF {
        return String::from_utf8_lossy(&b[3..]).into_owned();
    }
    if let Ok(s) = core::str::from_utf8(b) {
        if s.bytes().any(|x| x >= 0x80) {
            return s.into();
        }
    }
    b.iter().filter_map(|&c| if c < 0x80 { Some(c as char) } else { base_char(BaseEnc::WinAnsi, c) }).filter(|c| *c != '\0').collect()
}

/// Destinations resolved to page indexes. Named destinations are resolved while the
/// name tree is walked and kept as a sorted (name, page) table.
struct Dests {
    named: Option<Vec<(Vec<u8>, u16)>>,
    /// (page object number, page index), sorted.
    by_num: Vec<(u32, u16)>,
}

impl Dests {
    fn new(pages: &[super::Page]) -> Dests {
        let mut by_num: Vec<(u32, u16)> =
            pages.iter().enumerate().filter_map(|(i, p)| p.num.map(|n| (n, i.min(u16::MAX as usize) as u16))).collect();
        by_num.sort_unstable();
        Dests { named: None, by_num }
    }

    fn page_by_num(&self, n: u32) -> Option<usize> {
        self.by_num.binary_search_by_key(&n, |e| e.0).ok().map(|i| self.by_num[i].1 as usize)
    }

    /// The page of an explicit destination (an array, or a dictionary with /D).
    fn explicit_page<R: ReadAt>(&self, doc: &mut Document<'_, R>, dest: &Obj, depth: u32) -> Option<usize> {
        if depth > 4 {
            return None;
        }
        let dest = doc.resolve_cow(dest).ok()?;
        match &*dest {
            Obj::Array(a) => match a.first()? {
                Obj::Ref(n, _) => self.page_by_num(*n),
                Obj::Int(i) => Some((*i).max(0) as usize),
                _ => None,
            },
            Obj::Dict(_) => {
                let d = dest.get("D")?;
                self.explicit_page(doc, d, depth + 1)
            }
            _ => None,
        }
    }

    fn load_named<R: ReadAt>(&mut self, doc: &mut Document<'_, R>) {
        if self.named.is_some() {
            return;
        }
        let mut out: Vec<(Vec<u8>, u16)> = Vec::new();
        if let Ok(cat) = doc.catalog() {
            if let Ok(d) = doc.get_key(&cat, "Dests") {
                if let Some(entries) = d.dict() {
                    for (k, v) in entries.iter().take(NAMED_LIMIT) {
                        if let Some(p) = self.explicit_page(doc, v, 0) {
                            out.push((k.as_bytes().to_vec(), p.min(u16::MAX as usize) as u16));
                        }
                    }
                }
            }
            if let Ok(names) = doc.get_key(&cat, "Names") {
                if let Ok(tree) = doc.get_key(&names, "Dests") {
                    let mut seen = BTreeSet::new();
                    self.walk_name_tree(doc, &tree, &mut out, &mut seen, 0);
                }
            }
        }
        out.sort();
        out.dedup_by(|a, b| a.0 == b.0);
        self.named = Some(out);
    }

    fn walk_name_tree<R: ReadAt>(
        &self,
        doc: &mut Document<'_, R>,
        node: &Obj,
        out: &mut Vec<(Vec<u8>, u16)>,
        seen: &mut BTreeSet<u32>,
        depth: u32,
    ) {
        if depth > 16 || out.len() >= NAMED_LIMIT {
            return;
        }
        if let Ok(Obj::Array(names)) = doc.get_key(node, "Names") {
            for pair in names.chunks(2) {
                if let (Some(Obj::Str(k)), Some(v)) = (pair.first(), pair.get(1)) {
                    if let Some(p) = self.explicit_page(doc, v, 0) {
                        out.push((k.clone(), p.min(u16::MAX as usize) as u16));
                    }
                }
                if out.len() >= NAMED_LIMIT {
                    return;
                }
            }
        }
        if let Ok(Obj::Array(kids)) = doc.get_key(node, "Kids") {
            for k in kids.iter().take(512) {
                if let Obj::Ref(n, _) = k {
                    if !seen.insert(*n) {
                        continue;
                    }
                }
                if let Ok(child) = doc.resolve(k) {
                    self.walk_name_tree(doc, &child, out, seen, depth + 1);
                }
            }
        }
    }

    fn page_of<R: ReadAt>(&mut self, doc: &mut Document<'_, R>, dest: &Obj) -> Option<usize> {
        let dest = doc.resolve_cow(dest).ok()?;
        let key: &[u8] = match &*dest {
            Obj::Name(n) => n.as_bytes(),
            Obj::Str(s) => s,
            other => return self.explicit_page(doc, other, 0),
        };
        self.load_named(doc);
        let named = self.named.as_ref()?;
        named.binary_search_by(|e| e.0.as_slice().cmp(key)).ok().map(|i| named[i].1 as usize)
    }
}

fn outlines<R: ReadAt>(doc: &mut Document<'_, R>, dests: &mut Dests) -> Vec<Outline> {
    let mut out = Vec::new();
    let Ok(cat) = doc.catalog() else { return out };
    let Ok(root) = doc.get_key(&cat, "Outlines") else { return out };
    if root == Obj::Null {
        return out;
    }
    let first = root.get("First").cloned().unwrap_or(Obj::Null);
    let mut seen = BTreeSet::new();
    walk_outline(doc, dests, &first, 0, &mut out, &mut seen);
    out
}

fn walk_outline<R: ReadAt>(
    doc: &mut Document<'_, R>,
    dests: &mut Dests,
    first: &Obj,
    depth: u8,
    out: &mut Vec<Outline>,
    seen: &mut BTreeSet<u32>,
) {
    let mut cur = first.clone();
    let mut n = 0;
    while out.len() < 2000 && n < 2000 {
        n += 1;
        if let Obj::Ref(num, _) = cur {
            if !seen.insert(num) {
                return;
            }
        }
        let Ok(item) = doc.resolve(&cur) else { return };
        if item == Obj::Null {
            return;
        }
        let title = match doc.get_key(&item, "Title") {
            Ok(Obj::Str(s)) => pdf_string(&s),
            _ => String::new(),
        };
        let mut dest = item.get("Dest").cloned().unwrap_or(Obj::Null);
        if dest == Obj::Null {
            if let Ok(a) = doc.get_key(&item, "A") {
                if a.get("S").and_then(|s| s.name()) == Some("GoTo") {
                    dest = a.get("D").cloned().unwrap_or(Obj::Null);
                }
            }
        }
        let page = dests.page_of(doc, &dest);
        let t: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(p) = page {
            if !t.is_empty() {
                out.push(Outline { title: t, page: p, depth });
            }
        }
        if depth < 6 {
            if let Some(child) = item.get("First").cloned() {
                walk_outline(doc, dests, &child, depth + 1, out, seen);
            }
        }
        cur = item.get("Next").cloned().unwrap_or(Obj::Null);
        if cur == Obj::Null {
            return;
        }
    }
}

// ---------------------------------------------------------------------------------------
// Ingest.

fn page_geometry<R: ReadAt>(doc: &mut Document<'_, R>, page: &super::Page) -> (Mat, f32, f32) {
    let mb = doc.page_attr(page, "MediaBox").unwrap_or(Obj::Null);
    let mut b = [0.0f32, 0.0, 612.0, 792.0];
    if let Some(a) = mb.array() {
        if a.len() == 4 {
            for (i, o) in a.iter().enumerate() {
                b[i] = finite(doc.resolve_cow(o).ok().and_then(|v| v.num()).unwrap_or(b[i]));
            }
        }
    }
    let (x0, y0) = (b[0].min(b[2]), b[1].min(b[3]));
    let (w, h) = ((b[2] - b[0]).clamp(1.0, 20000.0), (b[3] - b[1]).clamp(1.0, 20000.0));
    let rot = doc.page_attr(page, "Rotate").ok().and_then(|o| o.int()).unwrap_or(0).rem_euclid(360);
    let base = Mat::translate(-x0, -y0);
    match rot {
        90 => (base.mul(Mat { a: 0.0, b: -1.0, c: 1.0, d: 0.0, e: 0.0, f: w }), h, w),
        180 => (base.mul(Mat { a: -1.0, b: 0.0, c: 0.0, d: -1.0, e: w, f: h }), w, h),
        270 => (base.mul(Mat { a: 0.0, b: 1.0, c: -1.0, d: 0.0, e: h, f: 0.0 }), h, w),
        _ => (base, w, h),
    }
}

fn page_content<R: ReadAt>(doc: &mut Document<'_, R>, page: &super::Page) -> Result<Vec<u8>, DocError> {
    let c = doc.page_attr(page, "Contents")?;
    let mut data = Vec::new();
    match &c {
        Obj::Stream { .. } => data = doc.stream_data(&c)?,
        Obj::Array(a) => {
            for o in a.iter().take(512) {
                let s = doc.resolve(o)?;
                if matches!(s, Obj::Stream { .. }) {
                    let part = doc.stream_data(&s)?;
                    if data.len() + part.len() > CONTENT_LIMIT {
                        return Err(DocError::TooLarge("pdf page content"));
                    }
                    data.extend_from_slice(&part);
                    data.push(b'\n');
                }
            }
        }
        _ => {}
    }
    if data.len() > CONTENT_LIMIT {
        return Err(DocError::TooLarge("pdf page content"));
    }
    Ok(data)
}

fn normalise_header(s: &str) -> String {
    s.chars().filter(|c| !c.is_ascii_digit()).flat_map(|c| c.to_lowercase()).filter(|c| !c.is_whitespace()).collect()
}

fn is_page_number(s: &str) -> bool {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    let t = lower.strip_prefix("page").map(|r| r.trim()).unwrap_or(t);
    // "3 / 12", "3 of 12".
    let t = t.split([' ', '/']).next().unwrap_or(t);
    if t.is_empty() || t.len() > 8 {
        return false;
    }
    let digits = t.chars().all(|c| c.is_ascii_digit());
    let roman = t.chars().all(|c| "ivxlcdm".contains(c));
    let framed = t.trim_matches(['-', '–', ' ', '[', ']', '(', ')']);
    digits || roman || (!framed.is_empty() && framed.chars().all(|c| c.is_ascii_digit()))
}

/// Join paragraphs a float interrupted. An unfinished body paragraph that ends its
/// block continues in the next block's first body paragraph when that starts in lower
/// case (or after a hyphen) — with nothing, or only figures, tables, listings, captions
/// and small blocks such as a copyright notice, in between; those are emitted after the
/// joined paragraph. A paragraph carried from the previous page (block 0) also continues
/// into an immediately following paragraph regardless of case, as before.
fn join_interrupted(paras: &mut Vec<Para>, words: &SeenWords) {
    const MAX_PARAS: usize = 16;
    const MAX_BLOCKS: u32 = 5;
    let mut i = 0;
    while i < paras.len() {
        let p = &paras[i];
        let open = p.image.is_none() && matches!(p.kind, ParaKind::Body) && !p.ended && !p.is_blank();
        let last_of_block = paras.get(i + 1).map(|n| n.block != p.block).unwrap_or(true);
        if !open || !last_of_block {
            i += 1;
            continue;
        }
        let hyphen = p.last_text().trim_end().ends_with('-');
        let cross_page = p.block == 0;
        let face = p.face().1;
        let mut j = i + 1;
        let mut found = false;
        let mut blocks_between = 0u32;
        let mut last_block = p.block;
        while j < paras.len() && j - i <= MAX_PARAS {
            let q = &paras[j];
            if q.block != last_block {
                blocks_between += 1;
                last_block = q.block;
            }
            if blocks_between > MAX_BLOCKS {
                break;
            }
            let first_of_block = paras[j - 1].block != q.block;
            let body = q.image.is_none() && matches!(q.kind, ParaKind::Body) && q.indent < 0.6 && !q.is_blank();
            // Prose in the same face, not a stray figure label that happens to start
            // in lower case.
            let prose = q.face().0 == face && (q.ended || q.text_len() >= 12);
            if body && first_of_block && prose && (q.starts_lower() || hyphen || (cross_page && j == i + 1)) {
                found = true;
                break;
            }
            if !q.is_float() {
                break;
            }
            j += 1;
        }
        if found {
            let q = paras.remove(j);
            paras[i].append_para(q, words);
        }
        i += 1;
    }
}

/// Ingest a PDF: text pages reflowed, scanned pages as images.
pub fn ingest<R: ReadAt>(file: &R, name: &str, sink: &mut dyn Sink) -> Result<(), DocError> {
    let mut doc = Document::open(file)?;
    let pages = doc.pages()?;
    if pages.is_empty() {
        return Err(DocError::Malformed("pdf: no pages"));
    }
    let total = pages.len();

    // Metadata.
    let mut meta = Metadata { title: crate::title_from_name(name), ..Default::default() };
    let info = doc.trailer.iter().find(|(k, _)| k == "Info").map(|(_, v)| v.clone());
    if let Some(info) = info {
        if let Ok(info) = doc.resolve(&info) {
            if let Ok(Obj::Str(t)) = doc.get_key(&info, "Title") {
                let t = pdf_string(&t);
                let t = t.trim();
                if !t.is_empty() && t.len() < 400 && !t.eq_ignore_ascii_case("untitled") {
                    meta.title = t.into();
                }
            }
            if let Ok(Obj::Str(a)) = doc.get_key(&info, "Author") {
                let a = pdf_string(&a);
                meta.authors = a
                    .split([';', ',', '&'])
                    .flat_map(|s| s.split(" and "))
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty() && s.len() < 120)
                    .take(8)
                    .map(String::from)
                    .collect();
            }
            if let Ok(Obj::Str(s)) = doc.get_key(&info, "Subject") {
                let s = pdf_string(&s);
                if !s.trim().is_empty() {
                    meta.description = Some(s.trim().into());
                }
            }
            if let Ok(Obj::Str(k)) = doc.get_key(&info, "Keywords") {
                meta.subjects =
                    pdf_string(&k).split([',', ';']).map(|s| s.trim()).filter(|s| !s.is_empty()).take(16).map(String::from).collect();
            }
        }
    }
    if let Ok(cat) = doc.catalog() {
        if let Ok(Obj::Str(l)) = doc.get_key(&cat, "Lang") {
            let l = pdf_string(&l);
            let l = l.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
            if l.len() == 2 {
                meta.language = l;
            }
        }
    }
    if meta.language.is_empty() {
        meta.language = "en".into();
    }
    sink.metadata(&meta)?;

    // Outlines → chapter plan: (first page, title, whether the title is a real one).
    let mut dests = Dests::new(&pages);
    let outline = outlines(&mut doc, &mut dests);
    let mut starts: Vec<(usize, Option<String>, bool)> = Vec::new();
    if !outline.is_empty() {
        // Top-level outline entries in ascending page order.
        let top_depth = outline.iter().map(|o| o.depth).min().unwrap_or(0);
        let mut last = 0usize;
        for o in outline.iter().filter(|o| o.depth == top_depth) {
            if o.page < total && (starts.is_empty() || o.page > last) {
                starts.push((o.page, Some(o.title.clone()), true));
                last = o.page;
            }
        }
        if starts.first().map(|s| s.0 > 0).unwrap_or(true) {
            starts.insert(0, (0, None, false));
        }
    } else {
        // No outline: a chapter every few pages, named for the library but not
        // typeset as a chapter opening.
        let mut p = 0;
        while p < total {
            starts.push((p, Some(alloc::format!("Pages {}–{}", p + 1, (p + PAGES_PER_CHAPTER).min(total))), false));
            p += PAGES_PER_CHAPTER;
        }
    }
    let chapter_of = |page: usize| -> u16 { (starts.partition_point(|s| s.0 <= page).saturating_sub(1)) as u16 };

    let mut toc: Vec<TocEntry> = Vec::new();
    if outline.is_empty() {
        for (i, (_, t, _)) in starts.iter().enumerate() {
            if let Some(t) = t {
                toc.push(TocEntry { title: t.clone(), chapter: i as u16, anchor: None, depth: 0 });
            }
        }
    } else {
        let top_depth = outline.iter().map(|o| o.depth).min().unwrap_or(0);
        for o in &outline {
            if o.page >= total {
                continue;
            }
            let ch = chapter_of(o.page);
            let anchor = if starts[ch as usize].0 == o.page { None } else { Some(alloc::format!("p{}", o.page + 1)) };
            toc.push(TocEntry { title: o.title.clone(), chapter: ch, anchor, depth: (o.depth - top_depth).min(6) });
        }
    }
    drop(outline);

    let mut fonts = FontCache { entries: Vec::new() };
    let mut headers: Vec<String> = Vec::new();
    let mut words = SeenWords::default();
    let mut w = Writer::new();
    // Characters handed to the sink for the open chapter before the current writer.
    let mut chapter_chars = 0u32;
    let mut chapter_open: Option<u16> = None;
    let mut carry: Option<Para> = None;
    let mut cover_done = false;
    let mut last_geom = (612.0f32, 792.0f32);

    let flush_para = |w: &mut Writer,
                      p: &Para,
                      sink: &mut dyn Sink,
                      doc: &mut Document<'_, R>,
                      page_w: f32,
                      page_h: f32,
                      cover_done: &mut bool|
     -> Result<(), DocError> {
        if let Some((img, iw, ih)) = &p.image {
            let page_area = page_w * page_h;
            let full = iw * ih >= 0.5 * page_area || (*iw >= 0.7 * page_w && *ih >= 0.5 * page_h);
            let fit = if full {
                Fit::inside(PAGE_W, PAGE_H)
            } else {
                let tw = ((iw / page_w) * PAGE_W as f32).clamp(64.0, limits::IMAGE_W as f32) as u32;
                Fit::inside(tw, limits::IMAGE_H)
            };
            match decode_image(doc, img, fit) {
                Ok(bm) => {
                    let id = sink.image(&bm)?;
                    w.push(&Token::Image { id, w: bm.w as u16, h: bm.h as u16 });
                    if !*cover_done && full {
                        if let (Ok(full), Ok(thumb)) = (
                            decode_image(doc, img, Fit::fill(limits::COVER_W, limits::COVER_H)),
                            decode_image(doc, img, Fit { fs: false, ..Fit::fill(limits::THUMB_W, limits::THUMB_H) }),
                        ) {
                            sink.cover(&full, &thumb)?;
                            *cover_done = true;
                        }
                    }
                }
                Err(DocError::Fs(e)) => return Err(DocError::Fs(e)),
                Err(_) => {}
            }
            return Ok(());
        }
        if p.is_blank() {
            return Ok(());
        }
        let code = matches!(p.kind, ParaKind::Code);
        w.para(p.kind);
        let mut cur = 0u8;
        let n = p.segs.len();
        for (i, (f, t)) in p.segs.iter().enumerate() {
            let mut t = t.as_str();
            if i == 0 && !code {
                t = t.trim_start();
            }
            if i + 1 == n {
                t = t.trim_end();
            }
            if t.is_empty() {
                continue;
            }
            if *f != cur {
                w.style(*f);
                cur = *f;
            }
            if code {
                // Listing lines keep their breaks.
                for (k, part) in t.split('\n').enumerate() {
                    if k > 0 {
                        w.push(&Token::Break);
                    }
                    w.text(part);
                }
            } else {
                w.text(t);
            }
        }
        if cur != 0 {
            w.style(0);
        }
        Ok(())
    };

    for (pi, page) in pages.iter().enumerate() {
        let ch = chapter_of(pi);
        if chapter_open != Some(ch) {
            if let Some(p) = carry.take() {
                flush_para(&mut w, &p, sink, &mut doc, last_geom.0, last_geom.1, &mut cover_done)?;
            }
            if chapter_open.is_some() {
                sink.chapter_bytes(w.as_bytes())?;
                sink.end_chapter(chapter_chars + w.char_count())?;
                w = Writer::new();
                chapter_chars = 0;
            }
            let (_, title, real) = &starts[ch as usize];
            sink.begin_chapter(ch, title.as_deref())?;
            if *real {
                if let Some(t) = title {
                    w.push(&Token::ChapterTitle { number: None, title: Some(t.clone()) });
                }
            }
            chapter_open = Some(ch);
            words.clear();
        }
        w.push(&Token::Anchor(alloc::format!("p{}", pi + 1)));

        let (ctm, pw, ph) = page_geometry(&mut doc, page);
        last_geom = (pw, ph);
        let resources = doc.page_attr(page, "Resources").unwrap_or(Obj::Null);
        let (items, text) = match page_content(&mut doc, page) {
            Ok(data) => {
                let mut interp = Interp { doc: &mut doc, fonts: &mut fonts, items: Vec::new(), text: String::new(), ops: 0 };
                let mut res = Res::new(resources);
                let init = GState {
                    ctm,
                    ts: TextState { font: None, size: 0.0, char_sp: 0.0, word_sp: 0.0, hscale: 1.0, leading: 0.0, rise: 0.0, render: 0 },
                };
                interp.run(&data, &mut res, init, 0);
                (interp.items, interp.text)
            }
            Err(DocError::Fs(e)) => return Err(DocError::Fs(e)),
            Err(_) => (Vec::new(), String::new()),
        };
        // Drop invisible OCR text when the page is a scan; keep it when it is all we have.
        let has_page_image =
            items.iter().any(|it| matches!(it.kind, ItemKind::Image(_)) && (it.x1 - it.x0) * (it.y1 - it.y0) >= 0.5 * pw * ph);
        let keep: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, it)| match &it.kind {
                ItemKind::Text { .. } => {
                    !it.text(&text).trim().is_empty()
                        && !(it.invisible && has_page_image)
                        && it.x1 > -pw
                        && it.x0 < 2.0 * pw
                        && it.y1 > -ph
                        && it.y0 < 2.0 * ph
                }
                ItemKind::Image(_) => true,
            })
            .map(|(i, _)| i)
            .collect();
        let body = body_size(&items);
        let mut blocks: Vec<Block> = Vec::new();
        xy_cut(&items, &text, keep, body, 0, &mut blocks);
        // The items and their text are done with: the lines own their strings now.
        drop(items);
        drop(text);
        for b in &blocks {
            for l in &b.lines {
                for s in &l.segs {
                    words.scan(&s.1);
                }
            }
        }

        // Running headers/footers and page numbers.
        let n_blocks = blocks.len();
        let mut paras: Vec<Para> = Vec::new();
        for (bi, b) in blocks.iter_mut().enumerate() {
            let edge = b.y0 > 0.9 * ph || b.y1 < 0.1 * ph;
            let small = b.lines.len() <= 2;
            let mut ps = block_paras(b, body, &words);
            if edge && (bi == 0 || bi + 1 == n_blocks) && small {
                let mut text = String::new();
                for p in &ps {
                    for s in &p.segs {
                        text.push_str(&s.1);
                    }
                    text.push(' ');
                }
                if is_page_number(&text) {
                    continue;
                }
                let norm = normalise_header(&text);
                if !norm.is_empty() && norm.len() < 120 {
                    if headers.contains(&norm) {
                        continue;
                    }
                    if headers.len() >= 16 {
                        headers.remove(0);
                    }
                    headers.push(norm);
                }
            }
            for p in &mut ps {
                p.block = bi as u32 + 1;
            }
            paras.append(&mut ps);
        }
        drop(blocks);

        // A paragraph continued from the previous page comes first (block 0), then
        // floats that interrupted a paragraph are moved after it.
        if let Some(c) = carry.take() {
            paras.insert(0, c);
        }
        join_interrupted(&mut paras, &words);
        let last = paras.pop();
        for p in &paras {
            flush_para(&mut w, p, sink, &mut doc, pw, ph, &mut cover_done)?;
        }
        drop(paras);
        // Keep the last paragraph open only if it might continue on the next page of this chapter.
        if let Some(p) = last {
            let next_same_chapter = pi + 1 < total && chapter_of(pi + 1) == ch;
            if next_same_chapter && p.image.is_none() && !p.ended && matches!(p.kind, ParaKind::Body) {
                carry = Some(p);
            } else {
                flush_para(&mut w, &p, sink, &mut doc, pw, ph, &mut cover_done)?;
            }
        }
        // Hand this page's QTX to the sink, so a chapter never sits in memory whole.
        sink.chapter_bytes(w.as_bytes())?;
        chapter_chars += w.char_count();
        w = Writer::new();
        sink.progress(pi as u32 + 1, total as u32);
    }
    if let Some(p) = carry.take() {
        flush_para(&mut w, &p, sink, &mut doc, last_geom.0, last_geom.1, &mut cover_done)?;
    }
    if chapter_open.is_some() {
        sink.chapter_bytes(w.as_bytes())?;
        sink.end_chapter(chapter_chars + w.char_count())?;
    }
    sink.toc(&toc)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::memsink::MemSink;
    use quire_qtx::Reader;

    #[test]
    fn encodings_are_complete() {
        assert_eq!(MAC_HIGH.chars().count(), 128);
        assert_eq!(base_char(BaseEnc::WinAnsi, 0x93), Some('“'));
        assert_eq!(base_char(BaseEnc::MacRoman, 0xD2), Some('“'));
        assert_eq!(base_char(BaseEnc::Standard, 0xAA), Some('“'));
        assert_eq!(glyph_char("quoteright"), Some('’'));
        assert_eq!(glyph_char("uni2014"), Some('—'));
        assert_eq!(glyph_char("a"), Some('a'));
        assert_eq!(glyph_char("g123"), None);
        assert_eq!(glyph_char("period.sc"), Some('.'));
    }

    fn lookup(tu: &ToUnicode, code: u32) -> Option<String> {
        let mut s = String::new();
        tu.push(code, &mut s).then_some(s)
    }

    #[test]
    fn tounicode_ranges() {
        let cmap = b"/CIDInit /ProcSet findresource begin begincmap 1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
            2 beginbfchar <0003> <0020> <0024> <0041> endbfchar\n\
            1 beginbfrange <0044> <0046> <0061> endbfrange\n\
            1 beginbfrange <0050> <0051> [<0078> <0079>] endbfrange\n\
            2 beginbfchar <0060> <FB01> <0061> <00660066006C> endbfchar\n\
            1 beginbfchar <0024> <0042> endbfchar endcmap";
        let cm = parse_cmap(cmap);
        assert_eq!(cm.codespace, alloc::vec![(2u8, 0u32, 0xFFFFu32)]);
        let tu = &cm.to_unicode;
        assert_eq!(lookup(tu, 3).as_deref(), Some(" "));
        assert_eq!(lookup(tu, 0x24).as_deref(), Some("B"), "later definition wins");
        assert_eq!(lookup(tu, 0x45).as_deref(), Some("b"));
        assert_eq!(lookup(tu, 0x51).as_deref(), Some("y"));
        assert_eq!(lookup(tu, 0x60).as_deref(), Some("fi"), "ligature expanded");
        assert_eq!(lookup(tu, 0x61).as_deref(), Some("ffl"), "three-unit result");
        assert_eq!(lookup(tu, 0x70), None);
    }

    #[test]
    fn matrices_compose() {
        let t = Mat::translate(10.0, 5.0);
        let s = Mat { a: 2.0, b: 0.0, c: 0.0, d: 2.0, e: 0.0, f: 0.0 };
        let m = t.mul(s);
        assert_eq!(m.apply(1.0, 1.0), (22.0, 12.0));
    }

    #[test]
    fn accents_compose() {
        let mut s = String::from("na¨ıve");
        compose_accents(&mut s);
        assert_eq!(s, "naïve");
        let mut s = String::from("Andr´e ˇCapek `a la mode");
        compose_accents(&mut s);
        assert_eq!(s, "André Čapek `a la mode", "a bare grave at a word start is left alone");
        let mut s = String::from("e\u{301}te\u{301}");
        compose_accents(&mut s);
        assert_eq!(s, "été", "combining marks after the base");
        let mut s = String::from("plain ascii text");
        compose_accents(&mut s);
        assert_eq!(s, "plain ascii text");
    }

    #[test]
    fn hyphen_healing_uses_seen_words() {
        let mut words = SeenWords::default();
        words.scan("The TraceMonkey VM and SunSpider, plus JavaScript.");
        assert!(words.joins("VM Trace-", "Monkey runs"));
        assert!(words.joins("Sun-", "Spider."));
        assert!(!words.joins("X-", "ray"));
        let mut p = Para {
            kind: ParaKind::Body,
            segs: alloc::vec![(0, String::from("the Trace-"))],
            image: None,
            indent: 0.0,
            ended: false,
            by_bold: false,
            lines: 1,
            block: 1,
        };
        let mut line = Line {
            x0: 0.0,
            x1: 0.0,
            base: 0.0,
            size: 0.0,
            segs: alloc::vec![(0, String::from("Monkey VM"))],
            image: None,
            img_box: (0.0, 0.0),
        };
        p.append_line(&mut line, &words);
        assert_eq!(p.segs[0].1, "the TraceMonkey VM");
        let mut line =
            Line { x0: 0.0, x1: 0.0, base: 0.0, size: 0.0, segs: alloc::vec![(0, String::from("Ray"))], image: None, img_box: (0.0, 0.0) };
        let mut p = Para {
            kind: ParaKind::Body,
            segs: alloc::vec![(0, String::from("an X-"))],
            image: None,
            indent: 0.0,
            ended: false,
            by_bold: false,
            lines: 1,
            block: 1,
        };
        p.append_line(&mut line, &words);
        assert_eq!(p.segs[0].1, "an X- Ray", "unknown compound keeps its hyphen");
    }

    /// A reader that counts card accesses, so the tests can bound the I/O per page.
    struct Counting<'a> {
        b: &'a [u8],
        calls: core::cell::Cell<u32>,
        bytes: core::cell::Cell<u64>,
    }
    impl quire_fs::ReadAt for Counting<'_> {
        fn len(&self) -> u64 {
            self.b.len() as u64
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> quire_fs::FsResult<usize> {
            self.calls.set(self.calls.get() + 1);
            let n = self.b.read_at(offset, buf)?;
            self.bytes.set(self.bytes.get() + n as u64);
            Ok(n)
        }
    }

    #[test]
    fn card_reads_per_page_are_bounded() {
        // (name, bytes, pages, most calls per page, most KB read in total).
        for (name, bytes, pages, max_calls, max_kb) in [
            ("tracemonkey", &include_bytes!("../../fixtures/tracemonkey.pdf")[..], 14u32, 60u32, 4096u64),
            ("basicapi", &include_bytes!("../../fixtures/basicapi.pdf")[..], 3, 40, 256),
            ("issue1002", &include_bytes!("../../fixtures/issue1002.pdf")[..], 1, 60, 256),
            ("alphatrans", &include_bytes!("../../fixtures/alphatrans.pdf")[..], 1, 60, 256),
        ] {
            let r = Counting { b: bytes, calls: Default::default(), bytes: Default::default() };
            let mut sink = MemSink::default();
            ingest(&r, name, &mut sink).unwrap();
            let (calls, kb) = (r.calls.get(), r.bytes.get() / 1024);
            std::println!("{name}: {calls} read_at calls, {kb} KB read, {} calls/page", calls / pages);
            assert!(calls / pages <= max_calls, "{name}: {calls} calls for {pages} pages");
            assert!(kb <= max_kb, "{name}: {kb} KB read from a {} KB file", bytes.len() / 1024);
        }
    }

    fn ingest_fixture(bytes: &[u8], name: &str) -> MemSink {
        let mut sink = MemSink::default();
        ingest(&bytes, name, &mut sink).unwrap_or_else(|e| panic!("{name}: {e}"));
        sink
    }

    #[test]
    fn tracemonkey_reflows_two_columns() {
        let s = ingest_fixture(include_bytes!("../../fixtures/tracemonkey.pdf"), "tracemonkey.pdf");
        let text = s.all_text();
        assert!(text.contains("Trace-based Just-in-Time Type Specialization for Dynamic"), "title: {}", &text[..text.len().min(300)]);
        assert!(
            text.contains("Dynamic languages such as JavaScript are more difficult to compile than statically typed ones"),
            "abstract start"
        );
        // Column text reads continuously (no interleaving of the two columns).
        assert!(text.contains("We present an alternative compilation technique for dynamically-typed languages"), "column text");
        assert!(text.contains("Trace-based Just-in-Time Type Specialization for Dynamic Languages"), "title joined across lines");
        assert!(text.contains("Design, Experimentation, Measurement, Performance."), "run-in heading not split into columns");
        assert!(text.contains("Categories and Subject Descriptors"));
        assert_eq!(s.chapters.len(), 2, "14 pages → 2 chapters of 10");
        assert!(s.meta.title.contains("Trace") || s.meta.title == "Tracemonkey", "{}", s.meta.title);
        // Hyphenation across lines is healed.
        assert!(!text.contains("com- pile"), "hyphen join");
    }

    #[test]
    fn tracemonkey_text_quality() {
        let s = ingest_fixture(include_bytes!("../../fixtures/tracemonkey.pdf"), "tracemonkey.pdf");
        let text = s.all_text();
        // Camel-case words split by a line-end hyphen are healed when seen elsewhere.
        assert!(text.contains("TraceMonkey"));
        assert!(!text.contains("Trace- Monkey"), "hyphen before a capital healed from context");
        assert!(text.contains("SunSpider") && !text.contains("Sun- Spider"));
        // TeX-style accents are composed; a dropped superscript leaves no stray space.
        assert!(text.contains("naïve"), "accent composed");
        assert!(!text.contains("na¨ıve"));
        assert!(text.contains("Brendan Eich,"), "space before comma removed: {}", &text[..text.len().min(400)]);
        // A paragraph interrupted by the copyright block, and by figures at the top of
        // the next column, continues after them.
        assert!(text.contains("client-side web programming and is used for the application logic"), "copyright float skipped");
        assert!(text.contains("dynamic compiler based on a set of industry benchmarks"), "figure floats skipped");
        // The state-machine diagram of tiny images and labels collapses to a caption.
        assert!(text.contains("diagram not shown"));
        assert!(!text.contains("edge cold/blacklisted"), "diagram labels dropped");
        // Tokens: no chapter title for synthetic chapter names; the paper title stays the
        // first heading; listings are code paragraphs with line breaks.
        let mut first_para = None;
        let mut code_paras = 0;
        let mut code_breaks = 0;
        let mut in_code = false;
        for (_, b, _) in &s.chapters {
            for tok in Reader::new(b) {
                match tok {
                    Token::ChapterTitle { .. } => panic!("synthetic chapter name emitted as a title"),
                    Token::Para(k) => {
                        if first_para.is_none() {
                            first_para = Some(k);
                        }
                        in_code = k == ParaKind::Code;
                        if in_code {
                            code_paras += 1;
                        }
                    }
                    Token::Break if in_code => code_breaks += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(first_para, Some(ParaKind::Heading(1)));
        assert!(code_paras >= 2, "listings are code paragraphs: {code_paras}");
        assert!(code_breaks >= 4, "code lines separated by breaks: {code_breaks}");
        assert!(text.contains("for (var i = 2; i < 100; ++i)"), "listing text kept");
        assert_eq!(s.chapters[0].0.as_deref(), Some("Pages 1–10"), "synthetic name still names the chapter");
    }

    #[test]
    fn basicapi_uses_outlines_and_identity_h() {
        let s = ingest_fixture(include_bytes!("../../fixtures/basicapi.pdf"), "basicapi.pdf");
        let text = s.all_text();
        assert!(text.contains("Table Of Content"), "{text}");
        assert!(!s.toc.is_empty(), "outline → toc");
        assert!(s.toc.iter().any(|t| t.title == "Chapter 1"), "{:?}", s.toc.iter().map(|t| &t.title).collect::<Vec<_>>());
        assert!(s.toc.iter().any(|t| t.title == "Paragraph 1.1" && t.depth == 1));
        assert_eq!(s.meta.title, "Basic API Test");
        assert!(!text.contains("page 1 / 3"), "footer page numbers dropped: {text}");
        assert!(s.chapters.len() >= 2);
        // Real outline titles are typeset as chapter openings.
        let titled = s.chapters.iter().filter(|(_, b, _)| Reader::new(b).any(|t| matches!(t, Token::ChapterTitle { .. }))).count();
        assert!(titled >= 1, "outline chapters carry a ChapterTitle");
    }

    #[test]
    fn alphatrans_yields_a_jpeg_image() {
        let s = ingest_fixture(include_bytes!("../../fixtures/alphatrans.pdf"), "alphatrans.pdf");
        assert!(!s.images.is_empty(), "DCTDecode image decoded");
        let bm = &s.images[0];
        assert!(bm.w > 32 && bm.h > 32);
    }

    #[test]
    fn issue1002_type1_widths() {
        let s = ingest_fixture(include_bytes!("../../fixtures/issue1002.pdf"), "issue1002.pdf");
        let text = s.all_text();
        assert!(text.split_whitespace().count() > 5, "{text}");
    }

    #[test]
    fn generated_pdf_with_xref_stream_objstm_and_raw_image() {
        // Build a PDF 1.5 file with an object stream, an xref stream, a Flate content
        // stream and an 8-bit grey raw image.
        use alloc::format;
        let content = b"BT /F1 24 Tf 72 700 Td (Hello Quire) Tj 0 -30 Td /F1 12 Tf [(Second) -300 (line)] TJ ET\n\
            q 200 0 0 100 72 400 cm /Im1 Do Q";
        let mut objs: Vec<(u32, Vec<u8>)> = Vec::new();
        // Object stream 5 holds objects 1 (catalog), 2 (pages), 3 (page), 4 (font).
        let inner: Vec<(u32, String)> = alloc::vec![
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".into()),
            (3, "<< /Type /Page /Parent 2 0 R /Contents 6 0 R /Resources << /Font << /F1 4 0 R >> /XObject << /Im1 7 0 R >> >> >>".into()),
            (4, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into()),
        ];
        let mut hdr = String::new();
        let mut body = String::new();
        for (n, s) in &inner {
            hdr.push_str(&format!("{n} {} ", body.len()));
            body.push_str(s);
            body.push('\n');
        }
        let stm = format!("{hdr}\n{body}");
        objs.push((
            5,
            format!("<< /Type /ObjStm /N {} /First {} /Length {} >>\nstream\n{}\nendstream", inner.len(), hdr.len() + 1, stm.len(), stm)
                .into_bytes(),
        ));
        let z = miniz_oxide::deflate::compress_to_vec_zlib(content, 6);
        let mut o6 = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", z.len()).into_bytes();
        o6.extend_from_slice(&z);
        o6.extend_from_slice(b"\nendstream");
        objs.push((6, o6));
        // 64×32 grey gradient with a black bar.
        let mut px = Vec::new();
        for y in 0..32u32 {
            for x in 0..64u32 {
                px.push(if (8..24).contains(&y) && x < 32 { 0 } else { (x * 4) as u8 });
            }
        }
        let zi = miniz_oxide::deflate::compress_to_vec_zlib(&px, 6);
        let mut o7 = format!("<< /Type /XObject /Subtype /Image /Width 64 /Height 32 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n", zi.len()).into_bytes();
        o7.extend_from_slice(&zi);
        o7.extend_from_slice(b"\nendstream");
        objs.push((7, o7));
        let mut file = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
        for (n, b) in &objs {
            offsets.insert(*n, file.len());
            file.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
            file.extend_from_slice(b);
            file.extend_from_slice(b"\nendobj\n");
        }
        // Xref stream object 8: W [1 4 2], entries 0..=8.
        let xref_off = file.len();
        let mut rows: Vec<u8> = Vec::new();
        let mut row = |t: u8, f2: u32, f3: u16| {
            rows.push(t);
            rows.extend_from_slice(&f2.to_be_bytes());
            rows.extend_from_slice(&f3.to_be_bytes());
        };
        row(0, 0, 0xFFFF);
        for i in 0..4u16 {
            row(2, 5, i);
        }
        for n in 5..=7u32 {
            row(1, offsets[&n] as u32, 0);
        }
        row(1, xref_off as u32, 0);
        let zx = miniz_oxide::deflate::compress_to_vec_zlib(&rows, 6);
        file.extend_from_slice(
            format!("8 0 obj\n<< /Type /XRef /Size 9 /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /Length {} >>\nstream\n", zx.len())
                .as_bytes(),
        );
        file.extend_from_slice(&zx);
        file.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref_off}\n%%EOF\n").as_bytes());

        let mut sink = MemSink::default();
        ingest(&&file[..], "gen.pdf", &mut sink).expect("ingest");
        let text = sink.all_text();
        assert!(text.contains("Hello Quire"), "{text}");
        assert!(text.contains("Second line"), "{text}");
        assert_eq!(sink.images.len(), 1, "raw grey image decoded");
        let bm = &sink.images[0];
        assert!(bm.w >= 64, "image scaled to layout width, got {}×{}", bm.w, bm.h);
        // The black bar region is dark, the right side light.
        let dark = (0..bm.h / 2).filter(|&y| bm.get(bm.w / 8, y + bm.h / 4)).count();
        let light = (0..bm.h).filter(|&y| bm.get(bm.w - 2, y)).count();
        assert!(dark > bm.h as usize / 4, "black bar present");
        assert!(light < bm.h as usize / 4, "gradient end is light");
    }

    #[test]
    fn code_listing_in_a_monospaced_face() {
        // Two Courier lines become one Code paragraph with a break; the Times line after
        // them is a separate body paragraph.
        let content = b"BT /F2 10 Tf 72 700 Td (int main\\(\\) {) Tj 0 -12 Td (  return 0;) Tj ET\n\
            BT /F1 10 Tf 72 660 Td (The listing above returns zero.) Tj ET";
        let pdf = alloc::format!(
            "%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >> endobj\n\
             4 0 obj << /Length {} >>\nstream\n{}\nendstream endobj\n\
             5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >> endobj\n\
             6 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Courier >> endobj\n\
             trailer << /Root 1 0 R >>\n%%EOF",
            content.len(),
            core::str::from_utf8(content).unwrap()
        );
        let s = ingest_fixture(pdf.as_bytes(), "code.pdf");
        let mut toks: Vec<Token> = Vec::new();
        for (_, b, _) in &s.chapters {
            toks.extend(Reader::new(b));
        }
        let code_at = toks.iter().position(|t| *t == Token::Para(ParaKind::Code)).expect("code paragraph: {toks:?}");
        assert!(toks[code_at..].contains(&Token::Break), "break between listing lines: {toks:?}");
        assert!(toks.contains(&Token::Para(ParaKind::Body)), "prose after the listing is body: {toks:?}");
        let text = s.all_text();
        assert!(text.contains("int main() {") && text.contains("return 0;"), "{text}");
    }
}

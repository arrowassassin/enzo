//! The baked font strikes and a small registry to find them.
//!
//! Families and sizes follow the design brief (§4): Literata for reading text, titles
//! and numerals; Atkinson Hyperlegible for labels and lists; JetBrains Mono for edge
//! labels, times and code.
#![no_std]
#![forbid(unsafe_code)]

pub use quire_gfx::Font;
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/packs.rs"));

/// Font family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Family {
    /// Serif reading face.
    #[default]
    Literata,
    /// Humanist sans for UI.
    Atkinson,
    /// Monospace for edge labels, times, code.
    Mono,
}

/// Style within a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Style {
    /// Regular weight.
    #[default]
    Regular,
    /// Bold.
    Bold,
    /// Italic.
    Italic,
    /// Bold italic.
    BoldItalic,
}

impl Style {
    /// Combine bold and italic flags.
    pub const fn from_flags(bold: bool, italic: bool) -> Style {
        match (bold, italic) {
            (false, false) => Style::Regular,
            (true, false) => Style::Bold,
            (false, true) => Style::Italic,
            (true, true) => Style::BoldItalic,
        }
    }
}

/// The reading sizes the layout engine offers, in pixels.
pub const READING_SIZES: [u16; 8] = [20, 22, 24, 26, 28, 31, 34, 38];

fn fam_name(f: Family) -> &'static str {
    match f {
        Family::Literata => "literata",
        Family::Atkinson => "atkinson",
        Family::Mono => "mono",
    }
}
fn sty_name(s: Style) -> &'static str {
    match s {
        Style::Regular => "regular",
        Style::Bold => "bold",
        Style::Italic => "italic",
        Style::BoldItalic => "bolditalic",
    }
}

fn find(name: &str) -> Option<&'static Font> {
    PACKS.iter().find(|(n, _)| *n == name).map(|(_, f)| f)
}

/// Exact strike lookup.
pub fn get(family: Family, style: Style, px: u16) -> Option<&'static Font> {
    let mut buf = heapless_name(fam_name(family), sty_name(style), px);
    find(buf.as_str_mut())
}

/// Best available strike: exact style, then regular, then the nearest size.
pub fn nearest(family: Family, style: Style, px: u16) -> &'static Font {
    if let Some(f) = get(family, style, px) {
        return f;
    }
    if let Some(f) = get(family, Style::Regular, px) {
        return f;
    }
    let prefix = fam_name(family);
    let mut best: Option<(&'static Font, u16)> = None;
    for (n, f) in PACKS {
        if n.starts_with(prefix) && !n.contains("dropcap") {
            let d = f.size().abs_diff(px);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((f, d));
            }
        }
    }
    best.map(|(f, _)| f).unwrap_or(&PACKS[0].1)
}

/// Reading sizes with their own drop-cap strike; other sizes use the nearest of these.
pub const DROPCAP_SIZES: [u16; 4] = [20, 26, 31, 38];

/// The drop-cap strike for a reading size. Packs are named after the reading size they
/// serve; the builder solves the em so the cap spans three lines.
pub fn dropcap(px: u16) -> Option<&'static Font> {
    let mut best = DROPCAP_SIZES[0];
    for s in DROPCAP_SIZES {
        if s.abs_diff(px) < best.abs_diff(px) {
            best = s;
        }
    }
    let mut buf = heapless_name("literata", "dropcap", best);
    find(buf.as_str_mut())
}

/// UI faces used everywhere, by role (brief §4).
pub mod ui {
    use super::*;
    /// 18 px small-cap labels and captions.
    pub fn label() -> &'static Font {
        nearest(Family::Atkinson, Style::Regular, 18)
    }
    /// 18 px bold label.
    pub fn label_bold() -> &'static Font {
        nearest(Family::Atkinson, Style::Bold, 18)
    }
    /// 22 px body.
    pub fn body() -> &'static Font {
        nearest(Family::Atkinson, Style::Regular, 22)
    }
    /// 22 px bold body.
    pub fn body_bold() -> &'static Font {
        nearest(Family::Atkinson, Style::Bold, 22)
    }
    /// 22 px italic body.
    pub fn body_italic() -> &'static Font {
        nearest(Family::Atkinson, Style::Italic, 22)
    }
    /// 26 px list title.
    pub fn list_title() -> &'static Font {
        nearest(Family::Atkinson, Style::Regular, 26)
    }
    /// 26 px bold list title.
    pub fn list_title_bold() -> &'static Font {
        nearest(Family::Atkinson, Style::Bold, 26)
    }
    /// 32 px serif screen title.
    pub fn title() -> &'static Font {
        nearest(Family::Literata, Style::Regular, 32)
    }
    /// 44 px poster numeral.
    pub fn poster() -> &'static Font {
        nearest(Family::Literata, Style::Regular, 44)
    }
    /// 56 px hero numeral.
    pub fn hero() -> &'static Font {
        nearest(Family::Literata, Style::Regular, 56)
    }
    /// 18 px mono for edge labels, times, table numbers.
    pub fn mono() -> &'static Font {
        nearest(Family::Mono, Style::Regular, 18)
    }
    /// 22 px mono.
    pub fn mono_body() -> &'static Font {
        nearest(Family::Mono, Style::Regular, 22)
    }
    /// 18 px serif caption.
    pub fn serif_small() -> &'static Font {
        nearest(Family::Literata, Style::Regular, 18)
    }
}

/// A tiny fixed buffer for pack names without allocating.
struct NameBuf {
    buf: [u8; 40],
    len: usize,
}
impl NameBuf {
    fn as_str_mut(&mut self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
    fn push(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }
}
fn heapless_name(fam: &str, sty: &str, px: u16) -> NameBuf {
    let mut n = NameBuf { buf: [0; 40], len: 0 };
    n.push(fam);
    n.push("-");
    n.push(sty);
    n.push("-");
    let mut digits = [0u8; 5];
    let mut i = 0;
    let mut v = px;
    if v == 0 {
        digits[0] = b'0';
        i = 1;
    }
    while v > 0 {
        digits[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    for d in digits[..i].iter().rev() {
        n.push(core::str::from_utf8(core::slice::from_ref(d)).unwrap());
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_exist_and_have_glyphs() {
        assert!(PACKS.len() > 40, "{}", PACKS.len());
        let f = get(Family::Literata, Style::Regular, 26).expect("literata 26");
        assert_eq!(f.size(), 26);
        assert!(f.ascent() > 15 && f.descent() < -3, "ascent {} descent {}", f.ascent(), f.descent());
        let a = f.glyph('a').unwrap();
        assert!(a.bitmap.w > 5 && a.bitmap.h > 5 && a.advance() > 5);
        assert!(f.has('é') && f.has('—') && f.has('“') && f.has('€'));
        assert!(f.kern_q('A', 'V') < 0, "kerning present: {}", f.kern_q('A', 'V'));
    }

    /// Font strikes are the largest thing in the firmware image; keep them inside a budget
    /// the 6 MB OTA slot can hold alongside code, and make any growth deliberate.
    #[test]
    fn font_packs_stay_within_the_flash_budget() {
        const BUDGET: usize = 3 * 1024 * 1024;
        let total: usize = PACKS.iter().map(|(_, f)| f.byte_len()).sum();
        assert!(total <= BUDGET, "font packs are {} KB, over the {} KB budget", total / 1024, BUDGET / 1024);
        assert!(total > 512 * 1024, "suspiciously small: {} KB", total / 1024);
    }

    #[test]
    fn registry_roles_resolve() {
        assert_eq!(ui::label().size(), 18);
        assert_eq!(ui::title().size(), 32);
        assert_eq!(ui::poster().size(), 44);
        assert_eq!(ui::hero().size(), 56);
        assert_eq!(ui::mono().size(), 18);
        // A three-line drop cap: the cap height of 'M' spans three 145 % lines, at each
        // size that has its own strike.
        for size in DROPCAP_SIZES {
            let cap = dropcap(size).expect("drop cap");
            let m = cap.glyph('M').expect("M");
            let three_lines = (size as f32 * 1.45 * 3.0) as i32;
            assert!((m.bitmap.h as i32 - three_lines).abs() <= 8, "{size}: cap height {} vs three lines {}", m.bitmap.h, three_lines);
        }
        // Sizes without their own strike fall back to the nearest.
        assert!(dropcap(24).is_some() && dropcap(34).is_some());
        assert_eq!(nearest(Family::Literata, Style::Bold, 27).size(), 26);
    }
}

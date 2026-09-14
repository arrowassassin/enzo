//! Rasterise the bundled TTFs into QFP1 packs at build time.
//!
//! Strikes (family × style × px): see `STRIKES`. Coverage is thresholded at 0.5 to 1 bit;
//! the "darker text" option dilates at draw time. Kerning pairs come from the font's kern
//! and GPOS tables via ab_glyph. Charset: Basic Latin, Latin-1, Latin Extended-A, the
//! punctuation and symbols the UI uses. Missing glyphs are simply absent from the pack.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use quire_gfx::font::build::{pack, GlyphSpec};
use std::fs;
use std::path::Path;
use ttf_parser::{
    gpos::{PairAdjustment, PositioningSubtable},
    Face, GlyphId, Tag,
};

#[derive(Clone, Copy)]
enum Fam {
    Literata,
    Atkinson,
    Mono,
}
#[derive(Clone, Copy)]
enum Sty {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

const READING: &[u16] = &[20, 22, 24, 26, 28, 31, 34, 38];
/// Reading sizes that get their own drop-cap strike; others use the nearest.
pub const DROPCAP_SIZES: [u16; 4] = [20, 26, 31, 38];

fn strikes() -> Vec<(Fam, Sty, u16, &'static str)> {
    let mut v = Vec::new();
    for &s in READING {
        for sty in [Sty::Regular, Sty::Bold, Sty::Italic, Sty::BoldItalic] {
            v.push((Fam::Literata, sty, s, "text"));
        }
    }
    // Drop caps are large and rarely switched; four strikes cover the eight reading sizes.
    for s in DROPCAP_SIZES {
        v.push((Fam::Literata, Sty::Regular, s, "dropcap"));
    }
    v.push((Fam::Literata, Sty::Regular, 18, "text"));
    v.push((Fam::Literata, Sty::Regular, 32, "text"));
    // Poster and hero numerals only ever show figures and short words.
    for s in [44u16, 56] {
        v.push((Fam::Literata, Sty::Regular, s, "numeral"));
    }
    // UI faces at the brief's sizes (label 18, body 22, list 26, title 32, mono 18).
    for s in [18u16, 22, 26] {
        v.push((Fam::Atkinson, Sty::Regular, s, "text"));
        v.push((Fam::Atkinson, Sty::Bold, s, "text"));
    }
    v.push((Fam::Atkinson, Sty::Italic, 22, "text"));
    for s in [18u16, 20, 22, 24] {
        v.push((Fam::Mono, Sty::Regular, s, "text"));
    }
    v.push((Fam::Mono, Sty::Bold, 18, "text"));
    v
}

fn file(f: Fam, s: Sty) -> &'static str {
    match (f, s) {
        (Fam::Literata, Sty::Regular) => "ttf/Literata-Regular.ttf",
        (Fam::Literata, Sty::Bold) => "ttf/Literata-Bold.ttf",
        (Fam::Literata, Sty::Italic) => "ttf/Literata-Italic.ttf",
        (Fam::Literata, Sty::BoldItalic) => "ttf/Literata-BoldItalic.ttf",
        (Fam::Atkinson, Sty::Regular) => "ttf/AtkinsonHyperlegible-Regular.ttf",
        (Fam::Atkinson, Sty::Bold) => "ttf/AtkinsonHyperlegible-Bold.ttf",
        (Fam::Atkinson, Sty::Italic) | (Fam::Atkinson, Sty::BoldItalic) => "ttf/AtkinsonHyperlegible-Italic.ttf",
        (Fam::Mono, Sty::Bold) | (Fam::Mono, Sty::BoldItalic) => "ttf/JetBrainsMono-Bold.ttf",
        (Fam::Mono, _) => "ttf/JetBrainsMono-Regular.ttf",
    }
}

fn name(f: Fam, s: Sty, px: u16, kind: &str) -> String {
    let fam = match f {
        Fam::Literata => "literata",
        Fam::Atkinson => "atkinson",
        Fam::Mono => "mono",
    };
    let sty = match s {
        Sty::Regular => "regular",
        Sty::Bold => "bold",
        Sty::Italic => "italic",
        Sty::BoldItalic => "bolditalic",
    };
    match kind {
        "dropcap" => format!("{fam}-dropcap-{px}"),
        _ => format!("{fam}-{sty}-{px}"),
    }
}

fn charset(kind: &str) -> Vec<char> {
    if kind == "dropcap" {
        return ('A'..='Z').chain(['‘', '“', '"', '\'', 'É', 'À', 'Ö', 'Ü', 'Ç']).collect();
    }
    if kind == "numeral" {
        // Figures, the punctuation that appears between them, and letters for short words
        // such as "Thursday" or "min". Everything else is set in a text strike.
        return (0x20u32..=0x7E).chain(0xC0..=0xFF).filter_map(char::from_u32).chain(['·', '–', '—', '%', '°']).collect();
    }
    // Basic Latin and Latin-1: English, French, German, Spanish, Italian, Portuguese,
    // Dutch and the Nordic languages. Latin Extended-A ships as an SD font pack.
    let mut v: Vec<char> = (0x20u32..=0x7E).chain(0xA0..=0xFF).filter_map(char::from_u32).collect();
    v.extend(
        [
            0x2010, 0x2011, 0x2012, 0x2013, 0x2014, 0x2018, 0x2019, 0x201A, 0x201C, 0x201D, 0x201E, 0x2020, 0x2021, 0x2022, 0x2026, 0x2030,
            0x2032, 0x2033, 0x2039, 0x203A, 0x20AC, 0x2122, 0x2190, 0x2191, 0x2192, 0x2193, 0x2212, 0x2713, 0x2605, 0x2606, 0x25CF, 0x25CB,
            0x25A0, 0x25A1, 0x00B7, 0x2028, 0x2044, 0x2116, 0x2117, 0x2135, 0x02C6, 0x02DC, 0x0394, 0x03A9, 0x03BC, 0x03C0, 0x1E9E,
        ]
        .into_iter()
        .filter_map(char::from_u32),
    );
    v
}

/// Kerning from the GPOS `kern` feature (Literata, Atkinson and JetBrains Mono carry no
/// legacy `kern` table). Returns the x-advance adjustment in font units.
fn gpos_kern(face: &Face, first: GlyphId, second: GlyphId) -> i16 {
    let Some(gpos) = face.tables().gpos else { return 0 };
    let kern = Tag::from_bytes(b"kern");
    let mut lookup_ids: Vec<u16> = Vec::new();
    for i in 0..gpos.features.len() {
        if let Some(f) = gpos.features.get(i) {
            if f.tag == kern {
                lookup_ids.extend(f.lookup_indices);
            }
        }
    }
    lookup_ids.sort_unstable();
    lookup_ids.dedup();
    for li in lookup_ids {
        let Some(lookup) = gpos.lookups.get(li) else { continue };
        for sub in lookup.subtables.into_iter::<PositioningSubtable>() {
            if let PositioningSubtable::Pair(pa) = sub {
                match pa {
                    PairAdjustment::Format1 { coverage, sets } => {
                        if let Some(idx) = coverage.get(first) {
                            if let Some(set) = sets.get(idx) {
                                if let Some((v1, _)) = set.get(second) {
                                    return v1.x_advance;
                                }
                            }
                        }
                    }
                    PairAdjustment::Format2 { coverage, classes, matrix } => {
                        if coverage.get(first).is_some() {
                            let c = (classes.0.get(first), classes.1.get(second));
                            if let Some((v1, _)) = matrix.get(c) {
                                if v1.x_advance != 0 {
                                    return v1.x_advance;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    0
}

/// The scale that makes the em exactly `em_px` pixels. ab_glyph's scales are height-based
/// and its point conversion differs per face, so calibrate on the face's own ascender:
/// whatever scale we ask for, the ascent it reports over the ascender in font units is
/// the em it actually used.
fn scale_for_em(font: &FontRef, face: &Face, em_px: f32) -> PxScale {
    let probe = PxScale::from(em_px);
    let asc_units = face.ascender() as f32;
    let upem = face.units_per_em() as f32;
    if asc_units <= 0.0 || upem <= 0.0 {
        return probe;
    }
    let asc_px = font.as_scaled(probe).ascent();
    let em_actual = asc_px * upem / asc_units;
    if em_actual <= 0.0 {
        return probe;
    }
    PxScale::from(em_px * em_px / em_actual)
}

/// Height in pixels of 'M' rendered at `em`.
fn cap_height(font: &FontRef, face: &Face, em: u16) -> f32 {
    let scale = scale_for_em(font, face, em as f32);
    let id = font.glyph_id('M');
    let g = id.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
    match font.outline_glyph(g) {
        Some(og) => {
            let b = og.px_bounds();
            b.max.y - b.min.y
        }
        None => 0.0,
    }
}

/// Em size whose cap height spans three lines of text at the default 145 % line height.
/// Outlines scale linearly, so one measurement plus one correction is exact enough.
fn solve_dropcap_em(font: &FontRef, face: &Face, reading_px: u16) -> u16 {
    let target = reading_px as f32 * 1.45 * 3.0;
    let probe = (reading_px * 4).max(24);
    let h = cap_height(font, face, probe);
    if h <= 1.0 {
        return probe;
    }
    let em = (probe as f32 * target / h).round().clamp(8.0, 400.0) as u16;
    // One correction pass in case of hinting or rounding at the new size.
    let h2 = cap_height(font, face, em);
    if h2 <= 1.0 {
        return em;
    }
    (em as f32 * target / h2).round().clamp(8.0, 400.0) as u16
}

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=ttf");
    let mut list = String::new();
    let mut font_cache: std::collections::HashMap<&'static str, Vec<u8>> = Default::default();
    for (fam, sty, px, kind) in strikes() {
        let path = file(fam, sty);
        let bytes = font_cache.entry(path).or_insert_with(|| fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}")));
        let font = FontRef::try_from_slice(bytes).expect("valid ttf");
        let face = Face::parse(bytes, 0).expect("ttf-parser face");
        let upem = face.units_per_em() as f32;
        // A drop cap is named after the reading size it serves; its em is solved by
        // measurement so the cap height spans exactly three lines at 145 % line height.
        let em_px = if kind == "dropcap" { solve_dropcap_em(&font, &face, px) } else { px };
        let scale: PxScale = scale_for_em(&font, &face, em_px as f32);
        let sf = font.as_scaled(scale);
        let chars = charset(kind);
        let mut glyphs = Vec::with_capacity(chars.len());
        for &c in &chars {
            let id = font.glyph_id(c);
            if id.0 == 0 && c != '\u{FFFD}' {
                continue; // .notdef
            }
            let adv = sf.h_advance(id);
            let advance_q = (adv * 4.0).round().clamp(0.0, 65535.0) as u16;
            let glyph = id.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
            match font.outline_glyph(glyph) {
                Some(og) => {
                    let b = og.px_bounds();
                    let w = (b.max.x - b.min.x).ceil() as u32;
                    let h = (b.max.y - b.min.y).ceil() as u32;
                    if w == 0 || h == 0 || w > 255 || h > 255 {
                        glyphs.push(GlyphSpec { cp: c as u32, advance_q, bearing_x: 0, top: 0, w: 0, h: 0, bits: Vec::new() });
                        continue;
                    }
                    let stride = (w as usize).div_ceil(8);
                    let mut bits = vec![0u8; stride * h as usize];
                    og.draw(|x, y, cov| {
                        if cov >= 0.5 && x < w && y < h {
                            bits[y as usize * stride + (x as usize >> 3)] |= 0x80 >> (x & 7);
                        }
                    });
                    glyphs.push(GlyphSpec {
                        cp: c as u32,
                        advance_q,
                        bearing_x: b.min.x.round().clamp(-127.0, 127.0) as i8,
                        top: (-b.min.y).round().clamp(-127.0, 127.0) as i8,
                        w: w as u8,
                        h: h as u8,
                        bits,
                    });
                }
                None => glyphs.push(GlyphSpec { cp: c as u32, advance_q, bearing_x: 0, top: 0, w: 0, h: 0, bits: Vec::new() }),
            }
        }
        // Kerning over the letters and common punctuation only (pairs elsewhere are rare).
        let kern_set: Vec<char> =
            (0x20u32..=0x7E).chain(0xC0..=0xFF).chain([0x2018, 0x2019, 0x201C, 0x201D]).filter_map(char::from_u32).collect();
        let mut kerns = Vec::new();
        if kind != "dropcap" {
            for &a in &kern_set {
                let ia = font.glyph_id(a);
                if ia.0 == 0 {
                    continue;
                }
                for &b in &kern_set {
                    let ib = font.glyph_id(b);
                    if ib.0 == 0 {
                        continue;
                    }
                    let mut k = sf.kern(ia, ib);
                    if k == 0.0 {
                        let (fa, fb) = (face.glyph_index(a), face.glyph_index(b));
                        if let (Some(fa), Some(fb)) = (fa, fb) {
                            k = gpos_kern(&face, fa, fb) as f32 * em_px as f32 / upem;
                        }
                    }
                    let kq = (k * 4.0).round() as i32;
                    if kq != 0 {
                        kerns.push((a as u16, b as u16, kq.clamp(-32768, 32767) as i16));
                    }
                }
            }
        }
        let data = pack(em_px, sf.ascent().round() as i16, sf.descent().round() as i16, sf.line_gap().round() as i16, glyphs, kerns);
        let n = name(fam, sty, px, kind);
        fs::write(Path::new(&out_dir).join(format!("{n}.qfp")), &data).unwrap();
        list.push_str(&format!("    (\"{n}\", Font::from_bytes(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{n}.qfp\")))),\n"));
    }
    fs::write(
        Path::new(&out_dir).join("packs.rs"),
        format!("/// Every baked strike, by name.\npub static PACKS: &[(&str, Font)] = &[\n{list}];\n"),
    )
    .unwrap();
}

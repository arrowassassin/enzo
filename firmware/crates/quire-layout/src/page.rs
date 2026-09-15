//! Pagination: fills pages with blocks and produces draw items.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use quire_gfx::{Font, Rect};
use quire_qtx::ParaKind;
use serde::{Deserialize, Serialize};
use unicode_linebreak::linebreaks;

use crate::lines::{atomize, break_lines, skip_to, Atom, BreakOpts, Line};
use crate::para::{block_chars, read_block, Block};
use crate::{Align, Geometry, ParaStyle, Profile};

/// A reading position that survives typography changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default, Hash)]
pub struct Pos {
    /// Byte offset of the block in the chapter stream.
    pub para: u32,
    /// Word (break segment) index within the block.
    pub word: u32,
    /// Character offset inside the word after a hyphenation split.
    pub part: u16,
}

impl Pos {
    /// The start of a chapter.
    pub const START: Pos = Pos { para: 0, word: 0, part: 0 };
}

/// Something to draw.
#[derive(Clone, Debug)]
pub enum DrawItem {
    /// A run of text.
    Text {
        /// Left x.
        x: i32,
        /// Baseline y.
        y: i32,
        /// Font.
        font: &'static Font,
        /// Text.
        text: String,
        /// Style flags (for underline, sup/sub, link).
        style: u8,
    },
    /// A filled rectangle (rules).
    Rule(Rect),
    /// An image placeholder to be filled from the chapter's image cache.
    Image {
        /// Image id.
        id: u16,
        /// Target rectangle.
        rect: Rect,
    },
    /// A drop cap glyph.
    DropCap {
        /// Left x.
        x: i32,
        /// Baseline y.
        y: i32,
        /// Font.
        font: &'static Font,
        /// The character.
        ch: char,
    },
}

/// One laid-out page.
#[derive(Clone, Debug)]
pub struct Page {
    /// Where the page starts.
    pub start: Pos,
    /// Where the next page starts, or `None` at the end of the chapter.
    pub next: Option<Pos>,
    /// Draw items.
    pub items: Vec<DrawItem>,
    /// Characters of text on the page.
    pub chars: u32,
    /// Number of text lines placed.
    pub lines: u32,
    /// Whether the page carries an image or a chapter opening (full refresh advised).
    pub has_image: bool,
}

/// Lays out pages of one chapter.
pub struct Paginator<'a> {
    qtx: &'a [u8],
    profile: Profile,
    geom: Geometry,
}

struct PlacedPara {
    lines: Vec<Line>,
    left: i32,
    hanging: i32,
    line_h: i32,
    dropcap: Option<(char, &'static Font)>,
    keep_with_next: bool,
    space_before: i32,
    space_after: i32,
    bullet: Option<String>,
}

impl<'a> Paginator<'a> {
    /// New paginator over a chapter.
    pub fn new(qtx: &'a [u8], profile: Profile, geom: Geometry) -> Self {
        Paginator { qtx, profile, geom }
    }

    /// The geometry in use.
    pub fn geometry(&self) -> &Geometry {
        &self.geom
    }

    /// Lay out the page beginning at `start`. Returns `None` when `start` is past the end.
    pub fn page_from(&self, start: Pos) -> Option<Page> {
        self.page_bounded(start, None)
    }

    /// Lay out the page beginning at `start`, ending it no later than `limit` (a page
    /// boundary the caller wants kept, see [`build_index_pinned`]): a paragraph holding the
    /// limit is cut there, a word split at a part gets its hyphen, and the page's `next` is
    /// the limit itself. A limit at or before `start` is ignored.
    pub fn page_bounded(&self, start: Pos, limit: Option<Pos>) -> Option<Page> {
        let limit = limit.filter(|l| *l > start);
        let text = self.geom.text;
        let bottom = text.bottom();
        let mut y = text.y;
        let mut items = Vec::new();
        let mut chars = 0u32;
        let mut lines_placed = 0u32;
        let mut has_image = false;
        let mut pos = start;
        let mut first_block = true;
        let mut prev_was_heading = false;
        let mut chapter_open = false;
        let mut next: Option<Pos> = None;
        let mut placed_anything = false;

        while let Some(b) = read_block(self.qtx, pos.para) {
            if placed_anything && limit.is_some_and(|l| pos >= l) {
                // The kept boundary (or, if it fell inside a skipped block, the block after it).
                next = Some(pos);
                break;
            }
            match &b.block {
                Block::Chapter { number, title } => {
                    let (h, its) = self.chapter_block(number.as_deref(), title.as_deref(), y);
                    if y + h > bottom && placed_anything {
                        next = Some(Pos { para: b.off, word: 0, part: 0 });
                        break;
                    }
                    items.extend(its);
                    y += h;
                    chars += block_chars(&b.block);
                    has_image = true;
                    chapter_open = true;
                    placed_anything = true;
                    pos = Pos { para: b.next, word: 0, part: 0 };
                }
                Block::Image { id, w, h } => {
                    let (rw, rh) = fit_image(*w as i32, *h as i32, text.w as i32, (text.h as i32) - 2 * self.geom.line_h);
                    let avail = bottom - y;
                    if rh > avail && placed_anything {
                        next = Some(Pos { para: b.off, word: 0, part: 0 });
                        break;
                    }
                    let x = text.x + (text.w as i32 - rw) / 2;
                    items.push(DrawItem::Image { id: *id, rect: Rect::new(x, y, rw as u32, rh as u32) });
                    y += rh + self.geom.line_h / 2;
                    has_image = true;
                    placed_anything = true;
                    pos = Pos { para: b.next, word: 0, part: 0 };
                }
                Block::Rule => {
                    let h = self.geom.line_h * 2;
                    if y + h > bottom && placed_anything {
                        next = Some(Pos { para: b.off, word: 0, part: 0 });
                        break;
                    }
                    let cx = text.x + text.w as i32 / 2;
                    let ry = y + h / 2 - 1;
                    for k in -1..=1 {
                        items.push(DrawItem::Rule(Rect::new(cx + k * 16 - 2, ry, 4, 4)));
                    }
                    y += h;
                    placed_anything = true;
                    pos = Pos { para: b.next, word: 0, part: 0 };
                }
                Block::Para { kind, runs } => {
                    let first_in_chapter = chapter_open || (b.off == 0 && first_block && pos.word == 0);
                    let resumed = pos.word > 0 || pos.part > 0;
                    let until = limit.filter(|l| l.para == b.off).map(|l| (l.word, l.part));
                    let mut pp = self.layout_para(
                        *kind,
                        runs,
                        first_in_chapter && !resumed,
                        prev_was_heading || chapter_open,
                        (pos.word, pos.part),
                        until,
                    );
                    chapter_open = false;
                    if resumed {
                        pp.space_before = 0;
                    }
                    if pp.lines.is_empty() {
                        pos = Pos { para: b.next, word: 0, part: 0 };
                        continue;
                    }
                    let mut yy = y;
                    if placed_anything {
                        yy += pp.space_before;
                    }
                    // Keep headings with at least two following lines.
                    if pp.keep_with_next && yy + pp.lines.len() as i32 * pp.line_h + 2 * self.geom.line_h > bottom && placed_anything {
                        next = Some(Pos { para: b.off, word: 0, part: 0 });
                        break;
                    }
                    let total = pp.lines.len();
                    let mut placed = 0usize;
                    for (i, line) in pp.lines.iter().enumerate() {
                        if yy + pp.line_h > bottom {
                            break;
                        }
                        // Orphan control: don't leave the paragraph's first line alone at the page bottom.
                        if i == 0 && total >= 3 && yy + 2 * pp.line_h > bottom && placed_anything {
                            break;
                        }
                        let baseline =
                            yy + (pp.line_h + line_font_ascent(line, &self.profile) + line_font_descent(line, &self.profile)) / 2;
                        let indent = if i < pp.dropcap.map_or(0, |_| 3) {
                            0
                        } else if i > 0 {
                            pp.hanging
                        } else {
                            0
                        };
                        if i == 0 {
                            if let Some(bl) = &pp.bullet {
                                let f = self.profile.font(0);
                                items.push(DrawItem::Text { x: pp.left - 24, y: baseline, font: f, text: bl.clone(), style: 0 });
                            }
                        }
                        for (xq, atom) in &line.atoms {
                            let x = pp.left + indent + ((*xq + 2) >> 2);
                            chars += atom.text.chars().count() as u32;
                            let (font, yb) = sup_sub(atom, baseline);
                            items.push(DrawItem::Text { x, y: yb, font, text: atom.text.clone(), style: atom.style });
                        }
                        yy += pp.line_h;
                        placed += 1;
                        lines_placed += 1;
                    }
                    if placed == 0 {
                        next = Some(Pos { para: b.off, word: pp.lines[0].first.0, part: pp.lines[0].first.1 });
                        break;
                    }
                    if let Some((ch, font)) = pp.dropcap {
                        // Baseline of the third line (or last if fewer).
                        let n = placed.min(3) as i32;
                        let base = y
                            + (if placed_anything { pp.space_before } else { 0 })
                            + (n - 1) * pp.line_h
                            + (pp.line_h + self.profile.font(0).ascent() + self.profile.font(0).descent()) / 2;
                        items.push(DrawItem::DropCap { x: pp.left, y: base, font, ch });
                    }
                    placed_anything = true;
                    y = yy;
                    prev_was_heading = matches!(kind, ParaKind::Heading(_));
                    if placed < total {
                        let l = &pp.lines[placed];
                        next = Some(Pos { para: b.off, word: l.first.0, part: l.first.1 });
                        break;
                    }
                    if until.is_some() {
                        next = limit;
                        break;
                    }
                    y += pp.space_after;
                    pos = Pos { para: b.next, word: 0, part: 0 };
                }
            }
            first_block = false;
            if pos.para as usize >= self.qtx.len() {
                break;
            }
        }
        if !placed_anything && next.is_none() && items.is_empty() {
            return None;
        }
        Some(Page { start, next, items, chars, lines: lines_placed, has_image })
    }

    fn chapter_block(&self, number: Option<&str>, title: Option<&str>, y0: i32) -> (i32, Vec<DrawItem>) {
        let text = self.geom.text;
        let mut items = Vec::new();
        let mut y = y0 + self.geom.line_h / 2;
        // The opening scales with the reading size so the title is never smaller than the
        // body: the numeral is two ems (at least the 56 px hero), the title an em and a
        // half or so (at least the 32 px title face).
        let size = self.profile.size;
        if let Some(n) = number {
            let f = quire_fonts::nearest(quire_fonts::Family::Literata, quire_fonts::Style::Regular, (size * 2).max(56));
            y += f.ascent();
            items.push(DrawItem::Text { x: text.x, y, font: f, text: n.to_string(), style: 0 });
            y += -f.descent() + 8;
        }
        if let Some(t) = title {
            let f = quire_fonts::nearest(quire_fonts::Family::Literata, quire_fonts::Style::Regular, (size + 6).max(32));
            let atoms =
                atomize(&[crate::para::Run { text: t.to_string(), style: 0, link: false, break_after: false }], &self.profile, false);
            let atoms: Vec<Atom> = atoms
                .into_iter()
                .map(|mut a| {
                    a.font = f;
                    a.width_q = quire_gfx::text::measure_text_q(f, &a.text);
                    a
                })
                .collect();
            let lines = break_lines(
                atoms,
                BreakOpts {
                    first_width_q: (text.w as i32) << 2,
                    width_q: (text.w as i32) << 2,
                    narrow_lines: 0,
                    justify: false,
                    hyphenate: false,
                    lang: self.profile.lang,
                },
            );
            let lh = (f.height() * 115 / 100).max(f.height());
            for line in &lines {
                let base = y + f.ascent();
                for (xq, a) in &line.atoms {
                    items.push(DrawItem::Text { x: text.x + ((*xq + 2) >> 2), y: base, font: f, text: a.text.clone(), style: 0 });
                }
                y += lh;
            }
            y += 8;
        }
        y += 8;
        items.push(DrawItem::Rule(Rect::new(text.x, y, 64, 4)));
        y += 4 + self.geom.line_h;
        (y - y0, items)
    }

    fn layout_para(
        &self,
        kind: ParaKind,
        runs: &[crate::para::Run],
        first_in_chapter: bool,
        after_heading: bool,
        start: (u32, u16),
        until: Option<(u32, u16)>,
    ) -> PlacedPara {
        let text = self.geom.text;
        let p = &self.profile;
        let lh = self.geom.line_h;
        let em = p.size as i32;
        let mut left = text.x;
        let mut width = text.w as i32;
        let mut hanging = 0;
        let mut justify = p.align == Align::Justify;
        let mut hyphenate = p.hyphenate && justify;
        let mut line_h = lh;
        let mut keep_with_next = false;
        let mut space_before = 0;
        let mut space_after = 0;
        let mut bullet = None;
        let mut first_indent = 0;
        let code = matches!(kind, ParaKind::Code);
        let mut dropcap: Option<(char, &'static Font)> = None;
        match kind {
            ParaKind::Body => {
                if p.para_style == ParaStyle::Indent && !after_heading && !first_in_chapter {
                    first_indent = em * 3 / 2;
                }
                if p.para_style == ParaStyle::Space {
                    space_after = lh / 2;
                }
                if first_in_chapter && p.drop_caps {
                    if let Some(first_char) = runs.iter().flat_map(|r| r.text.chars()).find(|c| !c.is_whitespace()) {
                        if first_char.is_alphabetic() {
                            if let Some(f) = quire_fonts::dropcap(p.size) {
                                if f.has(first_char) {
                                    dropcap = Some((first_char, f));
                                }
                            }
                        }
                    }
                }
            }
            ParaKind::Heading(level) => {
                let f = p.heading_font(level);
                line_h = (f.height() * 120 / 100).max(lh);
                keep_with_next = true;
                space_before = lh;
                space_after = lh / 2;
                justify = false;
                hyphenate = false;
            }
            ParaKind::Quote => {
                left += 24;
                width -= 48;
                space_before = lh / 2;
                space_after = lh / 2;
            }
            ParaKind::Verse => {
                left += 16;
                width -= 16;
                hanging = 24;
                justify = false;
                hyphenate = false;
                space_after = lh / 2;
            }
            ParaKind::ListItem { ordered, level, index } => {
                let ind = 16 + 24 * level as i32;
                left += ind + 24;
                width -= ind + 24;
                bullet = Some(if ordered { alloc::format!("{index}.") } else { "•".to_string() });
                space_after = lh / 4;
            }
            ParaKind::Code => {
                left += 16;
                width -= 16;
                justify = false;
                hyphenate = false;
                let f = p.font(quire_qtx::style::MONO);
                line_h = (f.height() * 130 / 100).max(f.height());
                space_before = lh / 2;
                space_after = lh / 2;
            }
            ParaKind::Caption | ParaKind::Centered => {
                justify = false;
                hyphenate = false;
                space_after = lh / 2;
            }
            ParaKind::TableRow => {
                hanging = 24;
                justify = false;
            }
        }
        let width = width.max(64);
        let resumed = start != (0, 0);
        let mut atoms = atomize(runs, p, code);
        if resumed {
            atoms = skip_to(atoms, start.0, start.1);
            dropcap = None;
            first_indent = 0;
        }
        if let Some((word, part)) = until {
            atoms = cut_at(atoms, word, part);
        }
        if let ParaKind::Heading(level) = kind {
            let f = p.heading_font(level);
            for a in &mut atoms {
                a.font = f;
                a.width_q = quire_gfx::text::measure_text_q(f, &a.text);
            }
        }
        if let ParaKind::Caption = kind {
            let f = quire_fonts::nearest(p.family, quire_fonts::Style::Italic, p.small_size());
            for a in &mut atoms {
                a.font = f;
                a.width_q = quire_gfx::text::measure_text_q(f, &a.text);
            }
        }
        let (first_w, narrow) = if let Some((ch, f)) = dropcap {
            let cap_w = f.glyph(ch).map(|g| g.advance() + 8).unwrap_or(0);
            (width - cap_w, 3)
        } else {
            (width - first_indent, 1)
        };
        let mut lines = break_lines(
            atoms,
            BreakOpts {
                first_width_q: first_w << 2,
                width_q: (width - hanging) << 2,
                narrow_lines: narrow,
                justify,
                hyphenate,
                lang: p.lang,
            },
        );
        // Apply first-line indent / drop-cap offset / centering as x shifts.
        let cap_shift = dropcap.map(|(ch, f)| f.glyph(ch).map(|g| g.advance() + 8).unwrap_or(0)).unwrap_or(0);
        for (i, line) in lines.iter_mut().enumerate() {
            let shift = if dropcap.is_some() && i < 3 {
                cap_shift
            } else if i == 0 {
                first_indent
            } else {
                0
            };
            if matches!(kind, ParaKind::Caption | ParaKind::Centered) {
                let lw = line.natural_q >> 2;
                let c = (width - lw) / 2;
                for a in &mut line.atoms {
                    a.0 += c << 2;
                }
            } else if shift != 0 {
                for a in &mut line.atoms {
                    a.0 += shift << 2;
                }
            }
        }
        PlacedPara { lines, left, hanging, line_h, dropcap, keep_with_next, space_before, space_after, bullet }
    }
}

fn fit_image(w: i32, h: i32, max_w: i32, max_h: i32) -> (i32, i32) {
    if w <= 0 || h <= 0 {
        return (0, 0);
    }
    let mut rw = w.min(max_w);
    let mut rh = (h as i64 * rw as i64 / w as i64) as i32;
    if rh > max_h.max(1) {
        rh = max_h.max(1);
        rw = (w as i64 * rh as i64 / h as i64) as i32;
    }
    (rw.max(1), rh.max(1))
}

fn line_font_ascent(line: &Line, p: &Profile) -> i32 {
    line.atoms.first().map(|(_, a)| a.font.ascent()).unwrap_or(p.font(0).ascent())
}
fn line_font_descent(line: &Line, p: &Profile) -> i32 {
    line.atoms.first().map(|(_, a)| a.font.descent()).unwrap_or(p.font(0).descent())
}

fn sup_sub(atom: &Atom, baseline: i32) -> (&'static Font, i32) {
    if atom.style & quire_qtx::style::SUP != 0 {
        (atom.font, baseline - atom.font.ascent() / 2)
    } else if atom.style & quire_qtx::style::SUB != 0 {
        (atom.font, baseline + atom.font.ascent() / 4)
    } else {
        (atom.font, baseline)
    }
}

/// Drop the atoms from `(word, part)` on; an atom the cut falls inside keeps its head with
/// a hyphen, as a hyphenation split would leave it.
fn cut_at(atoms: Vec<Atom>, word: u32, part: u16) -> Vec<Atom> {
    let mut out = Vec::with_capacity(atoms.len());
    for mut a in atoms {
        if a.word > word || (a.word == word && a.part >= part) {
            break;
        }
        if a.word == word {
            let keep = (part - a.part) as usize;
            if let Some((byte, _)) = a.text.char_indices().nth(keep) {
                let mut head = String::from(&a.text[..byte]);
                head.push('-');
                a.width_q = quire_gfx::text::measure_text_q(a.font, &head);
                a.text = head;
                a.hyphenable = false;
            }
        }
        out.push(a);
    }
    if let Some(last) = out.last_mut() {
        last.space_q = 0;
        last.hard_break = false;
    }
    out
}

/// Build the page index for a chapter: the start position of every page.
pub fn build_index(qtx: &[u8], profile: Profile, geom: Geometry) -> Vec<Pos> {
    let pg = Paginator::new(qtx, profile, geom);
    let mut out = Vec::new();
    let mut pos = Pos::START;
    let mut guard = 0u32;
    while let Some(page) = pg.page_from(pos) {
        out.push(pos);
        guard += 1;
        match page.next {
            Some(n) if n > pos && guard < 100_000 => pos = n,
            _ => break,
        }
    }
    out
}

/// The paragraph's words as `(word index, raw characters)`: one entry per line-break
/// segment that carries text, numbered the way the layout engine numbers words, with the
/// whitespace after a word (and any whitespace-only segments) folded into it. The lengths
/// sum to the paragraph's character count, so a word's running sum is its raw offset and
/// `chars_before` / `pos_at_chars` are exact inverses. No fonts, no measuring, one String.
fn word_chars(runs: &[crate::para::Run]) -> Vec<(u32, u32)> {
    let mut text = String::new();
    for r in runs {
        text.push_str(&r.text);
        if r.break_after {
            text.push('\n');
        }
    }
    let mut out: Vec<(u32, u32)> = Vec::new();
    let mut lead = 0u32;
    let mut start = 0usize;
    let mut word = 0u32;
    let mut take = |seg: &str, word: u32, out: &mut Vec<(u32, u32)>| {
        // The newline standing in for a hard break is not a character of the text.
        let raw = seg.chars().filter(|c| *c != '\n').count() as u32;
        if seg.trim_end_matches([' ', '\n', '\t', '\u{A0}']).is_empty() {
            match out.last_mut() {
                Some(last) => last.1 += raw,
                None => lead += raw,
            }
        } else {
            out.push((word, raw + core::mem::take(&mut lead)));
        }
    };
    for (idx, _) in linebreaks(&text) {
        take(&text[start..idx], word, &mut out);
        start = idx;
        word += 1;
    }
    if start < text.len() {
        take(&text[start..], word, &mut out);
    }
    out
}

/// Build the page index with a page boundary kept at `pin` (the position the reader is at
/// when the typography changes, so the new page begins on the same word). Pages before the
/// pin end at it; from the pin on the index is the ordinary one.
pub fn build_index_pinned(qtx: &[u8], profile: Profile, geom: Geometry, pin: Pos) -> Vec<Pos> {
    let pg = Paginator::new(qtx, profile, geom);
    let mut out = Vec::new();
    let mut pos = Pos::START;
    let mut guard = 0u32;
    loop {
        let limit = (pos < pin).then_some(pin);
        let Some(page) = pg.page_bounded(pos, limit) else { break };
        out.push(pos);
        guard += 1;
        match page.next {
            Some(n) if n > pos && guard < 100_000 => pos = n,
            _ => break,
        }
    }
    out
}

/// Characters of text before `pos` (progress numerator): every block before it plus one,
/// then the raw offset of the word inside its paragraph and the part into the word.
pub fn chars_before(qtx: &[u8], pos: Pos) -> u32 {
    let mut n = 0u32;
    for b in crate::para::blocks(qtx) {
        if b.off >= pos.para {
            if b.off == pos.para {
                if let Block::Para { runs, .. } = &b.block {
                    for (word, chars) in word_chars(runs) {
                        if word < pos.word {
                            n += chars;
                        } else {
                            if word == pos.word {
                                n += pos.part as u32;
                            }
                            break;
                        }
                    }
                }
            }
            break;
        }
        n += block_chars(&b.block) + 1;
    }
    n
}

/// The position at or before a character offset (for sync and "go to percent"): the
/// inverse of [`chars_before`] for any word start.
pub fn pos_at_chars(qtx: &[u8], target: u32) -> Pos {
    let mut n = 0u32;
    let mut last = Pos::START;
    for b in crate::para::blocks(qtx) {
        let c = block_chars(&b.block) + 1;
        if n + c > target {
            if let Block::Para { runs, .. } = &b.block {
                let mut m = n;
                for (word, chars) in word_chars(runs) {
                    if m + chars > target {
                        return Pos { para: b.off, word, part: 0 };
                    }
                    m += chars;
                }
            }
            return Pos { para: b.off, word: 0, part: 0 };
        }
        n += c;
        last = Pos { para: b.off, word: 0, part: 0 };
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use quire_qtx::{Token, Writer};

    fn sample() -> Vec<u8> {
        let mut w = Writer::new();
        w.push(&Token::ChapterTitle { number: Some("IV".to_string()), title: Some("Round trips".to_string()) });
        w.para(ParaKind::Body);
        w.text("  Leading blanks, double  spaces and a styled ");
        w.push(&Token::Style(quire_qtx::style::ITALIC));
        w.text("mid");
        w.push(&Token::Style(0));
        w.text("word run; then a very long stretch of ordinary prose that goes on for several lines so that pages break inside it, ");
        w.text("with numbers 1,234.56 and dashes—like this—and a non\u{A0}breaking space. ".repeat(40).as_str());
        w.para(ParaKind::Body);
        w.text("A second paragraph. ");
        w.push(&Token::Break);
        w.text("After a hard break, more words follow and follow and follow. ".repeat(30).as_str());
        w.finish()
    }

    /// Every page start maps to a character offset and back to the same word, at every
    /// reading size, and offsets grow with the page number.
    #[test]
    fn chars_before_and_pos_at_chars_are_inverse_at_every_page_start() {
        let qtx = sample();
        for size in quire_fonts::READING_SIZES {
            let p = Profile { size, ..Profile::default() };
            let g = p.geometry(quire_gfx::PANEL_W, quire_gfx::PANEL_H);
            let starts = build_index(&qtx, p, g);
            assert!(starts.len() > 3, "{size}: {} pages", starts.len());
            let mut prev = 0u32;
            for (i, pos) in starts.iter().enumerate() {
                let chars = chars_before(&qtx, *pos);
                assert!(i == 0 || chars > prev, "{size}: page {i} offset {chars} after {prev}");
                prev = chars;
                let back = pos_at_chars(&qtx, chars);
                assert_eq!((back.para, back.word), (pos.para, pos.word), "{size}: page {i} {pos:?} -> {chars} -> {back:?}");
            }
        }
        // A pinned index keeps every page start of the unpinned one after the pin, has the
        // pin itself as a page start, and its page before the pin ends exactly there.
        let p = Profile { size: 26, ..Profile::default() };
        let g = p.geometry(quire_gfx::PANEL_W, quire_gfx::PANEL_H);
        let plain = build_index(&qtx, p, g);
        let big = Profile { size: 34, ..Profile::default() };
        let gb = big.geometry(quire_gfx::PANEL_W, quire_gfx::PANEL_H);
        for pin in plain.iter().skip(1) {
            let pinned = build_index_pinned(&qtx, big, gb, *pin);
            assert!(pinned.contains(pin), "pin {pin:?} is a page start: {pinned:?}");
            let i = pinned.iter().position(|s| s == pin).unwrap();
            let before = Paginator::new(&qtx, big, gb).page_bounded(pinned[i - 1], Some(*pin)).unwrap();
            assert_eq!(before.next, Some(*pin));
            assert!(!before.items.is_empty());
            // From the pin on, the index is the ordinary one.
            let plain_big = build_index(&qtx, big, gb);
            let from_pin = Paginator::new(&qtx, big, gb).page_from(*pin).unwrap().next;
            assert!(from_pin.is_none_or(|n| pinned.contains(&n)) && plain_big.len() + 2 >= pinned.len());
        }
        // Word offsets are raw offsets: the sum over a paragraph is its character count.
        for b in crate::para::blocks(&qtx) {
            if let Block::Para { runs, .. } = &b.block {
                let total: u32 = word_chars(runs).iter().map(|(_, c)| *c).sum();
                assert_eq!(total, block_chars(&b.block));
            }
        }
    }
}

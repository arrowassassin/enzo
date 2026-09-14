//! Pagination: fills pages with blocks and produces draw items.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use quire_gfx::{Font, Rect};
use quire_qtx::ParaKind;
use serde::{Deserialize, Serialize};

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
                    let mut pp =
                        self.layout_para(*kind, runs, first_in_chapter && !resumed, prev_was_heading || chapter_open, (pos.word, pos.part));
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
        if let Some(n) = number {
            let f = quire_fonts::ui::hero();
            y += f.ascent();
            items.push(DrawItem::Text { x: text.x, y, font: f, text: n.to_string(), style: 0 });
            y += -f.descent() + 8;
        }
        if let Some(t) = title {
            let f = quire_fonts::ui::title();
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

/// Characters of text before `pos` (progress numerator).
pub fn chars_before(qtx: &[u8], pos: Pos) -> u32 {
    let mut n = 0u32;
    for b in crate::para::blocks(qtx) {
        if b.off >= pos.para {
            if b.off == pos.para {
                if let Block::Para { runs, .. } = &b.block {
                    let p = Profile::default();
                    let atoms = atomize(runs, &p, false);
                    for a in atoms {
                        if a.word < pos.word {
                            n += a.text.chars().count() as u32 + 1;
                        } else if a.word == pos.word {
                            n += pos.part as u32;
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

/// The position at or before a character offset (for sync and "go to percent").
pub fn pos_at_chars(qtx: &[u8], target: u32) -> Pos {
    let mut n = 0u32;
    let mut last = Pos::START;
    for b in crate::para::blocks(qtx) {
        let c = block_chars(&b.block) + 1;
        if n + c > target {
            if let Block::Para { runs, .. } = &b.block {
                let p = Profile::default();
                let atoms = atomize(runs, &p, false);
                let mut m = n;
                for a in atoms {
                    let w = a.text.chars().count() as u32 + 1;
                    if m + w > target {
                        return Pos { para: b.off, word: a.word, part: 0 };
                    }
                    m += w;
                }
            }
            return Pos { para: b.off, word: 0, part: 0 };
        }
        n += c;
        last = Pos { para: b.off, word: 0, part: 0 };
    }
    last
}

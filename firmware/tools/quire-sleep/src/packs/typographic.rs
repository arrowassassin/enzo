//! Typographic: book pages set in the device's own Literata — drop caps from the
//! drop-cap strike, italic quotations (public domain), small-cap attributions and
//! hairline rules. The clock takes the place of a running head or a folio.

use crate::canvas::{Canvas, Paint, Rect};
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::text;
use crate::{seed_for, Art, H, W};
use quire_fonts::{Family, Style};

/// The pack.
pub const PACK: Pack = Pack {
    id: "typographic",
    name: "Typographic",
    description: "Book pages in Literata: drop caps, public-domain quotations and hairline rules",
    count: PER_PACK,
    render,
};

struct Quote {
    text: &'static str,
    author: &'static str,
    source: &'static str,
}

const QUOTES: [Quote; 5] = [
    Quote {
        text: "We are such stuff as dreams are made on, and our little life is rounded with a sleep.",
        author: "William Shakespeare",
        source: "The Tempest, 1611",
    },
    Quote {
        text: "I have loved the stars too fondly to be fearful of the night.",
        author: "Sarah Williams",
        source: "The Old Astronomer, 1868",
    },
    Quote { text: "Only that day dawns to which we are awake.", author: "Henry David Thoreau", source: "Walden, 1854" },
    Quote {
        text: "Silently, one by one, in the infinite meadows of heaven, blossomed the lovely stars, the forget-me-nots of the angels.",
        author: "Henry Wadsworth Longfellow",
        source: "Evangeline, 1847",
    },
    Quote { text: "To sleep, perchance to dream.", author: "William Shakespeare", source: "Hamlet, 1603" },
];

const MARGIN: i32 = 56;
const CX: i32 = W as i32 / 2;

fn rule(c: &mut Canvas, y: i32, x0: i32, x1: i32, t: i32) {
    c.fill_rect(Rect::new(x0, y, x1 - x0, t), Paint::Ink);
}

/// A small printer's ornament: a diamond flanked by two dashes.
fn fleuron(c: &mut Canvas, cx: i32, cy: i32, p: Paint) {
    let (x, y) = (cx as f32, cy as f32);
    c.polygon(&[(x, y - 6.0), (x + 6.0, y), (x, y + 6.0), (x - 6.0, y)], p);
    c.line(x - 40.0, y, x - 14.0, y, 1.0, p);
    c.line(x + 14.0, y, x + 40.0, y, 1.0, p);
}

/// Set an italic block of lines, centred or left-aligned; returns the y after it.
#[allow(clippy::too_many_arguments)]
fn set_block(
    c: &mut Canvas,
    lines: &[String],
    font: &quire_fonts::Font,
    x: i32,
    mut y: i32,
    leading: i32,
    centered: bool,
    p: Paint,
) -> i32 {
    for l in lines {
        if centered {
            text::centered(c, font, CX, y, l, p, 0);
        } else {
            text::draw(c, font, x, y, l, p, 0);
        }
        y += leading;
    }
    y
}

fn attribution(c: &mut Canvas, q: &Quote, y: i32, centered: bool, p: Paint) {
    let label = quire_fonts::ui::label();
    let small = quire_fonts::ui::serif_small();
    let name = text::small_caps(q.author);
    if centered {
        text::centered(c, label, CX, y, &name, p, 3);
        text::centered(c, small, CX, y + 24, q.source, p, 0);
    } else {
        let w = text::width(label, &name, 3);
        text::draw(c, label, W as i32 - MARGIN - w, y, &name, p, 3);
        let w = text::width(small, q.source, 0);
        text::draw(c, small, W as i32 - MARGIN - w, y + 24, q.source, p, 0);
    }
}

/// A drop cap's vertical metrics: how many lines of `leading` its bitmap spans, and
/// how far the bitmap hangs below the baseline (the strike keeps `top` in an i8, so
/// tall caps carry part of their height under the baseline).
fn cap_metrics(font: &quire_fonts::Font, ch: char, leading: i32) -> (usize, i32) {
    let (h, top) = font.glyph(ch).map(|g| (g.bitmap.h as i32, g.top as i32)).unwrap_or((font.ascent(), font.ascent()));
    let lines = ((h + leading - 1) / leading).max(1) as usize;
    (lines, h - top)
}

/// Set a drop cap plus its text: `lines` beside the cap, the rest full width.
/// Returns the y after the last line.
fn drop_cap_block(c: &mut Canvas, q: &Quote, y0: i32, leading: i32) -> i32 {
    let cap_font = quire_fonts::dropcap(38).expect("drop cap strike");
    let ital = quire_fonts::nearest(Family::Literata, Style::Italic, 28);
    let first = q.text.chars().next().unwrap_or('A');
    let rest: String = q.text.chars().skip(1).collect();
    let cap_w = text::width(cap_font, &first.to_string(), 0);
    let (cap_lines, hang) = cap_metrics(cap_font, first, leading);
    let last_baseline = y0 + (cap_lines as i32 - 1) * leading;
    text::draw(c, cap_font, MARGIN, last_baseline - hang, &first.to_string(), Paint::Ink, 0);
    let narrow = W as i32 - 2 * MARGIN - cap_w - 14;
    let mut lines = text::wrap(ital, &rest, narrow, 0);
    let beside = lines.len().min(cap_lines);
    let side: Vec<String> = lines.drain(..beside).collect();
    let mut y = set_block(c, &side, ital, MARGIN + cap_w + 14, y0, leading, false, Paint::Ink);
    if !lines.is_empty() {
        let full: Vec<String> = text::wrap(ital, &lines.join(" "), W as i32 - 2 * MARGIN, 0);
        y = set_block(c, &full, ital, MARGIN, y, leading, false, Paint::Ink);
    }
    // A short quotation ends before the cap does.
    y.max(y0 + cap_lines as i32 * leading)
}

/// Layout A: clock as a running head, drop cap, ragged-right italic.
fn running_head(c: &mut Canvas, q: &Quote) -> ClockSlot {
    let slot = ClockSlot::centered(CX, 118, ClockStyle::Hero, Surface::Paper);
    rule(c, 56, MARGIN, W as i32 - MARGIN, 8);
    rule(c, 176, MARGIN, W as i32 - MARGIN, 1);
    rule(c, H as i32 - 46, MARGIN, W as i32 - MARGIN, 8);
    let y = drop_cap_block(c, q, 262, 40) + 24;
    rule(c, y, W as i32 - MARGIN - 120, W as i32 - MARGIN, 1);
    attribution(c, q, y + 34, false, Paint::Ink);
    fleuron(c, CX, H as i32 - 70, Paint::Ink);
    slot
}

/// Layout B: a giant opening quotation mark, centred lines, clock as the folio.
fn centred_quote(c: &mut Canvas, q: &Quote) -> ClockSlot {
    c.fill_rect(Rect::new(0, 672, W as i32, H as i32 - 672), Paint::Ink);
    c.fill_rect(Rect::new(MARGIN, 692, W as i32 - 2 * MARGIN, 1), Paint::Paper);
    let slot = ClockSlot::centered(CX, 740, ClockStyle::Poster, Surface::Ink);
    let cap_font = quire_fonts::dropcap(38).expect("drop cap strike");
    let ital = quire_fonts::nearest(Family::Literata, Style::Italic, 31);
    text::centered(c, cap_font, CX, 250, "\u{201C}", Paint::Ink, 0);
    let lines = text::wrap(ital, q.text, W as i32 - 2 * MARGIN - 20, 0);
    let leading = 46;
    let y0 = 300;
    let y = set_block(c, &lines, ital, 0, y0, leading, true, Paint::Ink);
    fleuron(c, CX, y + 4, Paint::Ink);
    attribution(c, q, y + 44, true, Paint::Ink);
    rule(c, 40, MARGIN, W as i32 - MARGIN, 2);
    slot
}

/// Layout C: an ink band carries the clock in paper; the page below is a plain text
/// page with a drop cap.
fn ink_band(c: &mut Canvas, q: &Quote) -> ClockSlot {
    c.fill_rect(Rect::new(0, 0, W as i32, 214), Paint::Ink);
    let slot = ClockSlot::centered(CX, 108, ClockStyle::Hero, Surface::Ink);
    c.fill_rect(Rect::new(MARGIN, 190, W as i32 - 2 * MARGIN, 1), Paint::Paper);
    c.fill_rect(Rect::new(MARGIN, 24, W as i32 - 2 * MARGIN, 1), Paint::Paper);
    let y = drop_cap_block(c, q, 300, 40);
    rule(c, y + 20, W as i32 - MARGIN - 120, W as i32 - MARGIN, 1);
    attribution(c, q, y + 54, false, Paint::Ink);
    slot
}

/// Layout D: the initial as a huge dithered watermark behind centred text.
fn watermark(c: &mut Canvas, q: &Quote) -> ClockSlot {
    let slot = ClockSlot::centered(CX, 690, ClockStyle::Hero, Surface::Paper);
    let cap_font = quire_fonts::dropcap(38).expect("drop cap strike");
    let first: String = q.text.chars().take(1).collect();
    // Render the cap at 1:1, then blow it up 3× as a light tone.
    let mut small = Canvas::sized(200, 220);
    text::draw(&mut small, cap_font, 10, 190, &first, Paint::Ink, 0);
    let bm = small.finish();
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..bm.h {
        for x in 0..bm.w {
            if bm.get(x, y) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 >= x0 {
        let scale = 3;
        let (gw, gh) = ((x1 - x0 + 1) * scale, (y1 - y0 + 1) * scale);
        let ox = (W - gw.min(W)) as i32 / 2;
        let oy = 330 - gh as i32 / 2;
        c.shade(Rect::new(ox, oy, gw as i32, gh as i32), |x, y| {
            let sx = x0 + (x - ox) as u32 / scale;
            let sy = y0 + (y - oy) as u32 / scale;
            if bm.get(sx, sy) {
                Some(Paint::Gray(0.72))
            } else {
                None
            }
        });
    }
    let ital = quire_fonts::nearest(Family::Literata, Style::Italic, 31);
    let lines = text::wrap(ital, q.text, W as i32 - 2 * MARGIN - 10, 0);
    let leading = 46;
    let block_h = lines.len() as i32 * leading;
    let y0 = 330 - block_h / 2 + 30;
    // Knock the text out of the watermark with a paper halo, then set it.
    for (k, l) in lines.iter().enumerate() {
        let w = text::width(ital, l, 0);
        c.fill_rect(Rect::new(CX - w / 2 - 6, y0 + k as i32 * leading - 30, w + 12, leading), Paint::Paper);
    }
    let y = set_block(c, &lines, ital, 0, y0, leading, true, Paint::Ink);
    c.fill_rect(Rect::new(CX - 150, y + 6, 300, 60), Paint::Paper);
    attribution(c, q, y + 30, true, Paint::Ink);
    rule(c, 52, MARGIN, W as i32 - MARGIN, 6);
    rule(c, 62, MARGIN, W as i32 - MARGIN, 1);
    rule(c, H as i32 - 52, MARGIN, W as i32 - MARGIN, 6);
    rule(c, H as i32 - 63, MARGIN, W as i32 - MARGIN, 1);
    slot
}

/// Layout E: a type specimen — the hero numerals in a strip, the quote below.
fn specimen(c: &mut Canvas, q: &Quote) -> ClockSlot {
    let slot = ClockSlot::centered(CX, 396, ClockStyle::Hero, Surface::Paper);
    let hero = quire_fonts::ui::hero();
    let title = quire_fonts::ui::title();
    c.fill_rect(Rect::new(0, 0, W as i32, 96), Paint::Ink);
    text::centered(c, quire_fonts::ui::label_bold(), CX, 56, "LITERATA  ·  56 PX  ·  NUMERALS", Paint::Paper, 3);
    text::centered(c, hero, CX, 170, "0123456789", Paint::Ink, 2);
    rule(c, 200, MARGIN, W as i32 - MARGIN, 1);
    text::centered(c, title, CX, 252, "ABCDEFGHIJKLM", Paint::Ink, 1);
    text::centered(c, title, CX, 292, "NOPQRSTUVWXYZ", Paint::Ink, 1);
    rule(c, 322, MARGIN, W as i32 - MARGIN, 1);
    rule(c, 470, MARGIN, W as i32 - MARGIN, 1);
    let ital = quire_fonts::nearest(Family::Literata, Style::Italic, 26);
    let lines = text::wrap(ital, q.text, W as i32 - 2 * MARGIN, 0);
    let y = set_block(c, &lines, ital, 0, 520, 38, true, Paint::Ink);
    attribution(c, q, y + 16, true, Paint::Ink);
    rule(c, H as i32 - 50, MARGIN, W as i32 - MARGIN, 2);
    slot
}

fn render(i: usize) -> Art {
    let _seed = seed_for(PACK.id, i);
    let q = &QUOTES[i % QUOTES.len()];
    let mut c = Canvas::new();
    let (slot, title) = match i {
        0 => (running_head(&mut c, q), "Running head"),
        1 => (centred_quote(&mut c, q), "Folio"),
        2 => (ink_band(&mut c, q), "Ink band"),
        3 => (watermark(&mut c, q), "Watermark"),
        _ => (specimen(&mut c, q), "Specimen"),
    };
    let credit = format!("{CREDIT}. Quotation: {}, {} (public domain)", q.author, q.source);
    Art { canvas: c, slot, title: title.into(), credit }
}

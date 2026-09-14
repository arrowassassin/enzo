//! The component library (brief §5): running head, rail, side labels, rows, tiles,
//! setting rows, dialog and working cards, empty states, covers, bars.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, measure_text, BitmapRef, BlitMode, Frame, Ink, Pattern, Rect, Rotation, TextStyle};

use crate::icons::{self, Icon};
use crate::text::{centered_baseline, draw_centered, draw_label, draw_right, ellipsis, label_style, line_h, small_caps, wrap};
use crate::theme::*;
use crate::{Key, KeyEvent};

/// Content x inset used by titled screens.
pub const INSET: i32 = 32;

/// Y where content starts under a running head.
pub const CONTENT_TOP: i32 = 92;

/// Running head for a screen: serif title, optional right-hand mono text, 4 px rule.
pub fn running_head(f: &mut Frame, title: &str, right: Option<&str>) {
    let font = quire_fonts::ui::title();
    let inverted = false;
    let w = f.width() as i32;
    draw_text(f, font, INSET, 28 + font.ascent(), &ellipsis(font, title, w - 2 * INSET - 90), TextStyle { inverted, ..TextStyle::INK });
    if let Some(r) = right {
        draw_right(f, quire_fonts::ui::mono(), w - INSET, 28 + font.ascent(), r, TextStyle::INK);
    }
    f.fill_rect(Rect::new(INSET, 72, (w - 2 * INSET) as u32, RULE_HEAVY), Ink::Black);
}

/// The reading page's running head: book title left, chapter right, 18 px small caps.
pub fn reading_head(f: &mut Frame, left: &str, right: &str, text_x: i32, text_right: i32, baseline: i32) {
    let font = quire_fonts::ui::label();
    let style = label_style(false);
    let r = small_caps(right);
    let rw = measure_text(font, &r, style);
    let avail = text_right - text_x - rw - 24;
    let l = ellipsis(font, &small_caps(left), avail.max(40));
    draw_text(f, font, text_x, baseline, &l, style);
    draw_text(f, font, text_right - rw, baseline, &r, style);
}

/// The edge-label rail at the bottom: four cells over the four keys.
pub fn rail(f: &mut Frame, labels: [&str; 4], focused: Option<usize>) {
    let w = f.width() as i32;
    let h = f.height() as i32;
    let y = h - RAIL_H;
    f.fill_rect(Rect::new(0, y, w as u32, RAIL_H as u32), Ink::White);
    f.fill_rect(Rect::new(0, y, w as u32, RULE_HEAVY), Ink::Black);
    let cell = w / 4;
    let font = quire_fonts::ui::mono();
    for (i, label) in labels.iter().enumerate() {
        let x = i as i32 * cell;
        let r = Rect::new(x, y + RULE_HEAVY as i32, cell as u32, (RAIL_H - RULE_HEAVY as i32) as u32);
        let inv = focused == Some(i);
        if inv {
            f.fill_rect(r, Ink::Black);
        }
        if i < 3 {
            f.fill_rect(Rect::new(x + cell - 1, y + RULE_HEAVY as i32, 1, (RAIL_H - RULE_HEAVY as i32) as u32), Ink::Black);
        }
        let cx = x + cell / 2;
        if label.is_empty() || *label == "—" {
            f.fill_rect(
                Rect::new(cx - 1, y + RULE_HEAVY as i32 + (RAIL_H - RULE_HEAVY as i32) / 2 - 1, 2, 2),
                if inv { Ink::White } else { Ink::Black },
            );
        } else {
            let text = ellipsis(font, label, cell - 8);
            draw_centered(
                f,
                font,
                cx,
                centered_baseline(font, y + RULE_HEAVY as i32, RAIL_H - RULE_HEAVY as i32),
                &text,
                TextStyle { inverted: inv, ..TextStyle::INK },
            );
        }
    }
}

/// Vertical positions of the two side labels (beside the Up and Down keys).
pub const SIDE_UP_Y: i32 = 500;
/// Down label top.
pub const SIDE_DOWN_Y: i32 = 592;
/// Side label box height.
pub const SIDE_H: i32 = 72;

/// Draw a rotated 18 px mono label at the right edge; `boxed` draws the 1 px frame.
fn side_label(f: &mut Frame, y: i32, label: &str, boxed: bool, inverted: bool) {
    let font = quire_fonts::ui::mono();
    let tw = measure_text(font, label, TextStyle::INK) + 12;
    let th = line_h(font) + 4;
    let mut tmp = Frame::new(tw as u32, th as u32);
    draw_text(&mut tmp, font, 6, 2 + font.ascent(), label, TextStyle::INK);
    let rot = tmp.rotated(Rotation::Cw90);
    let w = f.width() as i32;
    let x = w - 4 - th;
    let r = Rect::new(x, y, th as u32, tw as u32);
    if inverted {
        f.fill_rect(r, Ink::Black);
        f.blit(x, y, rot.as_bitmap(), BlitMode::Clear);
    } else {
        f.fill_rect(r, Ink::White);
        f.blit(x, y, rot.as_bitmap(), BlitMode::Or);
        if boxed {
            f.stroke_rect(r, 1, Ink::Black);
        }
    }
}

/// Side labels beside Up and Down (only when the side keys act).
pub fn side_labels(f: &mut Frame, up: Option<&str>, down: Option<&str>, boxed: bool) {
    if let Some(u) = up {
        side_label(f, SIDE_UP_Y, u, boxed, false);
    }
    if let Some(d) = down {
        side_label(f, SIDE_DOWN_Y, d, boxed, false);
    }
}

/// State of a list row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    /// Plain.
    Normal,
    /// Inverted.
    Focused,
    /// 4 px left bar.
    Selected,
    /// Under a dot screen.
    Disabled,
}

/// A list row: title (26 px, or 22 px with a subtitle), optional subtitle, right value in mono.
pub fn row(f: &mut Frame, y: i32, h: i32, title: &str, subtitle: Option<&str>, value: Option<&str>, state: RowState) {
    let w = f.width() as i32;
    let r = Rect::new(0, y, w as u32, h as u32);
    let inv = state == RowState::Focused;
    f.fill_rect(r, if inv { Ink::Black } else { Ink::White });
    let style = TextStyle { inverted: inv, ..TextStyle::INK };
    let vfont = quire_fonts::ui::mono();
    let mut right = w - ROW_PAD;
    if let Some(v) = value {
        let vw = measure_text(vfont, v, style);
        draw_text(f, vfont, w - ROW_PAD - vw, centered_baseline(vfont, y, h), v, style);
        right = w - ROW_PAD - vw - 16;
    }
    let avail = right - ROW_PAD;
    match subtitle {
        None => {
            let font = quire_fonts::ui::list_title();
            draw_text(f, font, ROW_PAD, centered_baseline(font, y, h), &ellipsis(font, title, avail), style);
        }
        Some(sub) => {
            let font = quire_fonts::ui::list_title();
            let sfont = quire_fonts::ui::label();
            let total = font.ascent() + font.descent() + 4 + sfont.ascent() + sfont.descent();
            let top = y + (h - total) / 2;
            draw_text(f, font, ROW_PAD, top + font.ascent(), &ellipsis(font, title, avail), style);
            draw_text(f, sfont, ROW_PAD, top + font.ascent() + font.descent() + 4 + sfont.ascent(), &ellipsis(sfont, sub, avail), style);
        }
    }
    if state == RowState::Selected {
        f.fill_rect(Rect::new(0, y, 4, h as u32), Ink::Black);
    }
    f.fill_rect(Rect::new(0, y + h - 1, w as u32, 1), Ink::Black);
    if state == RowState::Disabled {
        f.screen_rect(Rect::new(0, y, w as u32, (h - 1) as u32), Pattern::Dots50);
    }
}

/// A row with a thumbnail (88 px): thumb 48 × 72 at the left.
pub fn row_thumb(f: &mut Frame, y: i32, thumb: Option<BitmapRef<'_>>, title: &str, subtitle: &str, value: Option<&str>, state: RowState) {
    let w = f.width() as i32;
    let h = ROW_THUMB_H;
    let inv = state == RowState::Focused;
    f.fill_rect(Rect::new(0, y, w as u32, h as u32), if inv { Ink::Black } else { Ink::White });
    let tr = Rect::new(ROW_PAD, y + 8, 48, 72);
    match thumb {
        Some(bm) => {
            // Fit the thumbnail into 48 × 72 by nearest-neighbour sampling.
            for yy in 0..72 {
                for xx in 0..48 {
                    let sx = (xx as u32 * bm.w) / 48;
                    let sy = (yy as u32 * bm.h) / 72;
                    if bm.get(sx, sy) {
                        f.set(tr.x + xx, tr.y + yy, if inv { Ink::White } else { Ink::Black });
                    }
                }
            }
        }
        None => {
            f.pattern_rect(tr, Pattern::Hatch { pitch: 6 });
        }
    }
    f.stroke_rect(tr, 1, if inv { Ink::White } else { Ink::Black });
    let style = TextStyle { inverted: inv, ..TextStyle::INK };
    let vfont = quire_fonts::ui::label();
    let mut right = w - ROW_PAD;
    if let Some(v) = value {
        let vw = measure_text(vfont, v, style);
        draw_text(f, vfont, w - ROW_PAD - vw, centered_baseline(vfont, y, h), v, style);
        right = w - ROW_PAD - vw - 16;
    }
    let x = ROW_PAD + 48 + 16;
    let font = quire_fonts::ui::list_title();
    let sfont = quire_fonts::ui::label();
    let top = y + (h - (font.ascent() + font.descent() + 6 + sfont.ascent() + sfont.descent())) / 2;
    draw_text(f, font, x, top + font.ascent(), &ellipsis(font, title, right - x), style);
    draw_text(f, sfont, x, top + font.ascent() + font.descent() + 6 + sfont.ascent(), &ellipsis(sfont, subtitle, right - x), style);
    if state == RowState::Selected {
        f.fill_rect(Rect::new(0, y, 4, h as u32), Ink::Black);
    }
    f.fill_rect(Rect::new(0, y + h - 1, w as u32, 1), Ink::Black);
    if state == RowState::Disabled {
        f.screen_rect(Rect::new(0, y, w as u32, (h - 1) as u32), Pattern::Dots50);
    }
}

/// A section label row (18 px small caps) with a hairline beneath.
pub fn section_label(f: &mut Frame, y: i32, text: &str) -> i32 {
    draw_label(f, ROW_PAD, y + 22, text, false);
    y + 32
}

/// Paged focus over `n` items with `per_page` rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListNav {
    /// Focused index.
    pub focus: usize,
    /// Items.
    pub n: usize,
    /// Rows per page.
    pub per_page: usize,
}

impl ListNav {
    /// New.
    pub fn new(n: usize, per_page: usize) -> Self {
        ListNav { focus: 0, n, per_page: per_page.max(1) }
    }
    /// Current page.
    pub fn page(&self) -> usize {
        self.focus / self.per_page
    }
    /// Pages.
    pub fn pages(&self) -> usize {
        self.n.div_ceil(self.per_page).max(1)
    }
    /// Indexes on the current page.
    pub fn visible(&self) -> core::ops::Range<usize> {
        let s = self.page() * self.per_page;
        s..(s + self.per_page).min(self.n)
    }
    /// Apply a key: Up/Down move focus, Left/Right page. Returns true when handled.
    pub fn key(&mut self, ev: KeyEvent) -> bool {
        if self.n == 0 {
            return matches!(ev.key, Key::Up | Key::Down | Key::Left | Key::Right);
        }
        match ev.key {
            Key::Up => self.focus = if self.focus == 0 { self.n - 1 } else { self.focus - 1 },
            Key::Down => self.focus = (self.focus + 1) % self.n,
            Key::Left => {
                if self.page() == 0 {
                    self.focus = (self.pages() - 1) * self.per_page;
                } else {
                    self.focus -= self.per_page;
                }
            }
            Key::Right => {
                if self.page() + 1 >= self.pages() {
                    self.focus = 0;
                } else {
                    self.focus = (self.focus + self.per_page).min(self.n - 1);
                }
            }
            _ => return false,
        }
        true
    }
    /// Clamp after the item count changed.
    pub fn set_n(&mut self, n: usize) {
        self.n = n;
        if n == 0 {
            self.focus = 0;
        } else if self.focus >= n {
            self.focus = n - 1;
        }
    }
}

/// Rows that fit between `top` and the rail.
pub fn rows_between(top: i32, bottom: i32, row_h: i32) -> usize {
    ((bottom - top) / row_h).max(1) as usize
}

/// A cover grid cell: 152 × 228 in a 2 px frame (6 px on focus) plus two lines beneath.
#[allow(clippy::too_many_arguments)]
pub fn cover_cell(
    f: &mut Frame,
    x: i32,
    y: i32,
    thumb: Option<BitmapRef<'_>>,
    title: &str,
    author: &str,
    focused: bool,
    percent: Option<u8>,
) {
    let r = Rect::new(x, y, COVER_W, COVER_H);
    match thumb {
        Some(bm) => {
            f.fill_rect(r, Ink::White);
            let ox = x + (COVER_W as i32 - bm.w as i32) / 2;
            let oy = y + (COVER_H as i32 - bm.h as i32) / 2;
            f.blit(ox, oy, bm, BlitMode::Or);
        }
        None => typographic_cover(f, r, title, author),
    }
    f.stroke_rect(r, if focused { 6 } else { 2 }, Ink::Black);
    let tfont = quire_fonts::ui::body();
    let afont = quire_fonts::ui::label();
    draw_text(f, tfont, x, y + COVER_H as i32 + 8 + tfont.ascent(), &ellipsis(tfont, title, COVER_W as i32), TextStyle::INK);
    let line2 = match percent {
        Some(p) if p > 0 => alloc::format!("{author} · {p}%"),
        _ => String::from(author),
    };
    draw_text(
        f,
        afont,
        x,
        y + COVER_H as i32 + 8 + tfont.ascent() + tfont.descent() + 4 + afont.ascent(),
        &ellipsis(afont, &line2, COVER_W as i32),
        TextStyle::INK,
    );
}

/// The typographic cover: a white title plate over a 6 px hatch.
pub fn typographic_cover(f: &mut Frame, r: Rect, title: &str, author: &str) {
    f.fill_rect(r, Ink::White);
    f.pattern_rect(r, Pattern::Hatch { pitch: 6 });
    let tfont = quire_fonts::ui::body();
    let afont = quire_fonts::ui::label();
    let inner_w = r.w as i32 - 24;
    let lines = wrap(tfont, title, inner_w - 12);
    let lines: Vec<String> = lines.into_iter().take(4).collect();
    let lh = line_h(tfont);
    let ph = 12 + lines.len() as i32 * lh + 8 + line_h(afont) + 12;
    let plate = Rect::new(r.x + 12, r.y + 40.min(r.h as i32 / 6), inner_w as u32, ph.min(r.h as i32 - 48) as u32);
    f.fill_rect(plate, Ink::White);
    f.stroke_rect(plate, 1, Ink::Black);
    let cx = plate.x + plate.w as i32 / 2;
    let mut yy = plate.y + 12 + tfont.ascent();
    for l in &lines {
        draw_centered(f, tfont, cx, yy, l, TextStyle::INK);
        yy += lh;
    }
    draw_centered(f, afont, cx, yy + 8, &ellipsis(afont, author, inner_w - 12), TextStyle::INK);
}

/// Poster tiles: 44 px numeral over an 18 px small-cap label, 2-column grid with 2 px rules.
pub fn poster_tiles(f: &mut Frame, x: i32, y: i32, w: i32, tiles: &[(String, String)], cols: usize) -> i32 {
    let cols = cols.max(1);
    let cw = w / cols as i32;
    let nfont = quire_fonts::ui::poster();
    let lfont = quire_fonts::ui::label();
    let th = 24 + nfont.ascent() + nfont.descent() + 4 + line_h(lfont) + 20;
    let rows = tiles.len().div_ceil(cols);
    for (i, (value, label)) in tiles.iter().enumerate() {
        let (c, r) = (i % cols, i / cols);
        let tx = x + c as i32 * cw;
        let ty = y + r as i32 * th;
        let nf = if measure_text(nfont, value, TextStyle::INK) > cw - 40 { quire_fonts::ui::title() } else { nfont };
        draw_text(f, nf, tx + 28, ty + 24 + nfont.ascent(), value, TextStyle::INK);
        draw_label(f, tx + 28, ty + 24 + nfont.ascent() + nfont.descent() + 4 + lfont.ascent(), label, false);
        if c + 1 < cols && i + 1 < tiles.len() {
            f.fill_rect(Rect::new(tx + cw - 1, ty, RULE, th as u32), Ink::Black);
        }
        if r + 1 < rows {
            f.fill_rect(Rect::new(tx, ty + th - 1, cw as u32, RULE), Ink::Black);
        }
    }
    y + rows as i32 * th
}

/// A single poster numeral with its label, centred on `cx`.
pub fn poster_centered(f: &mut Frame, cx: i32, y: i32, value: &str, label: &str, hero: bool) -> i32 {
    let nfont = if hero { quire_fonts::ui::hero() } else { quire_fonts::ui::poster() };
    let lfont = quire_fonts::ui::label();
    draw_centered(f, nfont, cx, y + nfont.ascent(), value, TextStyle::INK);
    let ly = y + nfont.ascent() + nfont.descent() + 6 + lfont.ascent();
    draw_centered(f, lfont, cx, ly, &small_caps(label), label_style(false));
    ly + lfont.descent()
}

/// The value part of a setting row.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    /// A toggle.
    Toggle(bool),
    /// A stepper showing text ("26").
    Stepper(String),
    /// A choice showing text ("Justified ›").
    Choice(String),
    /// A slider: position 0–1000 and a value text.
    Slider(u16, String),
    /// Plain text (a status or a navigation row).
    Text(String),
    /// Navigation chevron only.
    Nav,
}

/// A setting row.
pub fn setting_row(f: &mut Frame, y: i32, h: i32, title: &str, value: &SettingValue, state: RowState) {
    let w = f.width() as i32;
    let inv = state == RowState::Focused;
    f.fill_rect(Rect::new(0, y, w as u32, h as u32), if inv { Ink::Black } else { Ink::White });
    let style = TextStyle { inverted: inv, ..TextStyle::INK };
    let font = quire_fonts::ui::body();
    let ink = if inv { Ink::White } else { Ink::Black };
    let paper = if inv { Ink::Black } else { Ink::White };
    let mut avail = w - 2 * ROW_PAD;
    match value {
        SettingValue::Toggle(on) => {
            let r = Rect::new(w - ROW_PAD - 56, y + (h - 28) / 2, 56, 28);
            f.stroke_rect(r, 2, ink);
            let kx = if *on { r.right() - 2 - 24 } else { r.x + 2 };
            f.fill_rect(Rect::new(kx, r.y + 2, 24, 24), ink);
            avail -= 72;
        }
        SettingValue::Stepper(v) => {
            let t = alloc::format!("‹ {v} ›");
            let tw = measure_text(font, &t, style);
            draw_text(f, font, w - ROW_PAD - tw, centered_baseline(font, y, h), &t, style);
            avail -= tw + 16;
        }
        SettingValue::Choice(v) => {
            let lf = quire_fonts::ui::label();
            let t = alloc::format!("{v} ›");
            let tw = measure_text(lf, &t, style);
            draw_text(f, lf, w - ROW_PAD - tw, centered_baseline(lf, y, h), &t, style);
            avail -= tw + 16;
        }
        SettingValue::Slider(pos, v) => {
            let lf = quire_fonts::ui::label();
            let tw = measure_text(lf, v, style);
            draw_text(f, lf, w - ROW_PAD - tw, centered_baseline(lf, y, h), v, style);
            let bar = Rect::new(w - ROW_PAD - tw - 14 - 180, y + h / 2 - 1, 180, 2);
            f.fill_rect(bar, ink);
            let kx = bar.x + ((*pos as i32).min(1000) * (180 - 16)) / 1000;
            f.fill_rect(Rect::new(kx, bar.y - 7, 16, 16), ink);
            avail -= tw + 14 + 180 + 16;
        }
        SettingValue::Text(v) => {
            let lf = quire_fonts::ui::label();
            let t = ellipsis(lf, v, (w - 2 * ROW_PAD) / 2);
            let tw = measure_text(lf, &t, style);
            draw_text(f, lf, w - ROW_PAD - tw, centered_baseline(lf, y, h), &t, style);
            avail -= tw + 16;
        }
        SettingValue::Nav => {
            icons::draw(f, Icon::ChevronRight, w - ROW_PAD - 24, y + (h - 24) / 2, ink);
            avail -= 40;
        }
    }
    let _ = paper;
    draw_text(f, font, ROW_PAD, centered_baseline(font, y, h), &ellipsis(font, title, avail), style);
    if state == RowState::Selected {
        f.fill_rect(Rect::new(0, y, 4, h as u32), Ink::Black);
    }
    f.fill_rect(Rect::new(0, y + h - 1, w as u32, 1), Ink::Black);
    if state == RowState::Disabled {
        f.screen_rect(Rect::new(0, y, w as u32, (h - 1) as u32), Pattern::Dots50);
    }
}

/// A full-width dialog card with two actions on Back and Confirm. Returns the card rect.
pub fn dialog(f: &mut Frame, title: &str, body: &str, cancel: &str, confirm: &str) -> Rect {
    let w = f.width() as i32;
    let font_t = quire_fonts::ui::title();
    let font_b = quire_fonts::ui::body();
    let inner = w - 2 * MARGIN - 2 * 28;
    let lines = wrap(font_b, body, inner);
    let lh = line_h(font_b);
    let h = 28 + font_t.ascent() + font_t.descent() + 12 + lines.len() as i32 * lh + 24 + RAIL_H + 2;
    let y = (f.height() as i32 - RAIL_H - h) / 2;
    let card = Rect::new(MARGIN, y, (w - 2 * MARGIN) as u32, h as u32);
    f.fill_rect(card, Ink::White);
    f.stroke_rect(card, 2, Ink::Black);
    draw_text(f, font_t, card.x + 28, card.y + 28 + font_t.ascent(), &ellipsis(font_t, title, inner), TextStyle::INK);
    let mut yy = card.y + 28 + font_t.ascent() + font_t.descent() + 12;
    for l in &lines {
        draw_text(f, font_b, card.x + 28, yy + font_b.ascent(), l, TextStyle::INK);
        yy += lh;
    }
    // The card's own rail: Cancel on Back, the action on Confirm.
    let ry = card.bottom() - RAIL_H - 2;
    f.fill_rect(Rect::new(card.x + 2, ry, card.w - 4, RULE_HEAVY), Ink::Black);
    let cell = (card.w as i32 - 4) / 4;
    let mono = quire_fonts::ui::mono();
    for i in 0..4 {
        let x = card.x + 2 + i * cell;
        let label = match i {
            1 => cancel,
            2 => confirm,
            _ => "",
        };
        if i < 3 {
            f.fill_rect(Rect::new(x + cell - 1, ry + RULE_HEAVY as i32, 1, (RAIL_H - RULE_HEAVY as i32) as u32), Ink::Black);
        }
        let cx = x + cell / 2;
        if label.is_empty() {
            f.fill_rect(Rect::new(cx - 1, ry + RULE_HEAVY as i32 + (RAIL_H - RULE_HEAVY as i32) / 2 - 1, 2, 2), Ink::Black);
        } else {
            draw_centered(f, mono, cx, centered_baseline(mono, ry + RULE_HEAVY as i32, RAIL_H - RULE_HEAVY as i32), label, TextStyle::INK);
        }
    }
    card
}

/// A stepped progress bar: 10 px steps with 2 px gaps inside a 2 px frame, 16 px tall.
pub fn stepped_bar(f: &mut Frame, r: Rect, permille: u32) {
    f.fill_rect(r, Ink::White);
    f.stroke_rect(r, 2, Ink::Black);
    let inner = Rect::new(r.x + 4, r.y + 4, r.w.saturating_sub(8), r.h.saturating_sub(8));
    let fill_w = (inner.w as u64 * permille.min(1000) as u64 / 1000) as i32;
    let mut x = inner.x;
    while x < inner.x + fill_w {
        let step = 10.min(inner.x + fill_w - x);
        f.fill_rect(Rect::new(x, inner.y, step as u32, inner.h), Ink::Black);
        x += 12;
    }
}

/// A working card: title, subtitle, stepped bar, mono status.
pub fn working_card(f: &mut Frame, title: &str, subtitle: &str, permille: u32, status: &str) -> Rect {
    let w = f.width() as i32;
    let ft = quire_fonts::ui::title();
    let fb = quire_fonts::ui::body();
    let fl = quire_fonts::ui::label();
    let h = 28 + ft.ascent() + ft.descent() + 8 + line_h(fb) + 20 + 16 + 10 + line_h(fl) + 28;
    let y = (f.height() as i32 - RAIL_H - h) / 2;
    let card = Rect::new(MARGIN, y, (w - 2 * MARGIN) as u32, h as u32);
    f.fill_rect(card, Ink::White);
    f.stroke_rect(card, 2, Ink::Black);
    let inner = card.w as i32 - 56;
    draw_text(f, ft, card.x + 28, card.y + 28 + ft.ascent(), &ellipsis(ft, title, inner), TextStyle::INK);
    let mut yy = card.y + 28 + ft.ascent() + ft.descent() + 8;
    draw_text(f, fb, card.x + 28, yy + fb.ascent(), &ellipsis(fb, subtitle, inner), TextStyle::INK);
    yy += line_h(fb) + 20;
    stepped_bar(f, Rect::new(card.x + 28, yy, inner as u32, 16), permille);
    yy += 16 + 10;
    draw_text(f, quire_fonts::ui::mono(), card.x + 28, yy + fl.ascent(), &ellipsis(quire_fonts::ui::mono(), status, inner), TextStyle::INK);
    card
}

/// An empty state: a serif line, an 18 px hint (the rail shows the fixing action).
pub fn empty_state(f: &mut Frame, y: i32, line: &str, hint: &str) {
    let w = f.width() as i32;
    let ft = quire_fonts::ui::title();
    let fl = quire_fonts::ui::label();
    let cx = w / 2;
    let mut yy = y;
    for l in wrap(ft, line, w - 2 * INSET) {
        draw_centered(f, ft, cx, yy + ft.ascent(), &l, TextStyle::INK);
        yy += line_h(ft);
    }
    yy += 8;
    for l in wrap(fl, hint, w - 2 * INSET) {
        draw_centered(f, fl, cx, yy + fl.ascent(), &l, TextStyle::INK);
        yy += line_h(fl);
    }
}

/// Screen an area with the 50 % dot pattern (the page beneath an overlay).
pub fn screen(f: &mut Frame, r: Rect) {
    f.screen_rect(r, SCREENED);
}

/// A tab line: names with the active one inverted, page number at the right, 2 px rule beneath.
pub fn tabs(f: &mut Frame, y: i32, names: &[&str], active: usize, focused: bool, right: Option<&str>) -> i32 {
    let font = quire_fonts::ui::label();
    let w = f.width() as i32;
    let mut x = INSET;
    let h = 32;
    for (i, n) in names.iter().enumerate() {
        let t = small_caps(n);
        let tw = measure_text(font, &t, label_style(false));
        let cell = Rect::new(x - 6, y, (tw + 12) as u32, h as u32);
        let inv = i == active;
        if inv {
            f.fill_rect(cell, Ink::Black);
            if focused {
                f.stroke_rect(cell, 3, Ink::Black);
            }
        }
        draw_text(f, font, x, centered_baseline(font, y, h), &t, label_style(inv));
        x += tw + 22;
    }
    if let Some(r) = right {
        draw_right(f, quire_fonts::ui::mono(), w - INSET, centered_baseline(quire_fonts::ui::mono(), y, h), r, TextStyle::INK);
    }
    f.fill_rect(Rect::new(INSET, y + h, (w - 2 * INSET) as u32, RULE), Ink::Black);
    y + h + RULE as i32
}

/// A battery and Wi-Fi status pair drawn at the top right (used by the Home layer and About).
pub fn status_icons(f: &mut Frame, x_right: i32, y: i32, battery: crate::Battery, wifi: bool) {
    let mut x = x_right - 24;
    icons::draw(f, Icon::Battery(icons::battery_level(battery.percent)), x, y, Ink::Black);
    if battery.charging {
        x -= 28;
        icons::draw(f, Icon::Charging, x, y, Ink::Black);
    }
    if wifi {
        x -= 28;
        icons::draw(f, Icon::Wifi, x, y, Ink::Black);
    }
}

/// A text field: 2 px rule beneath, caret, hint when empty.
pub fn text_field(f: &mut Frame, r: Rect, text: &str, hint: &str, focused: bool) {
    let font = quire_fonts::ui::list_title();
    f.fill_rect(r, Ink::White);
    let baseline = centered_baseline(font, r.y, r.h as i32);
    if text.is_empty() {
        draw_text(f, quire_fonts::ui::label(), r.x + 8, centered_baseline(quire_fonts::ui::label(), r.y, r.h as i32), hint, TextStyle::INK);
        if focused {
            f.fill_rect(Rect::new(r.x + 8, r.y + 8, 2, r.h.saturating_sub(16)), Ink::Black);
        }
    } else {
        let shown = tail_fit(font, text, r.w as i32 - 24);
        let end = draw_text(f, font, r.x + 8, baseline, &shown, TextStyle::INK);
        if focused {
            f.fill_rect(Rect::new(end + 2, r.y + 8, 2, r.h.saturating_sub(16)), Ink::Black);
        }
    }
    f.fill_rect(Rect::new(r.x, r.bottom() - 2, r.w, 2), Ink::Black);
}

/// The tail of `text` that fits `width` (for text fields).
pub fn tail_fit(font: &quire_gfx::Font, text: &str, width: i32) -> String {
    if measure_text(font, text, TextStyle::INK) <= width {
        return String::from(text);
    }
    let mut start = 0;
    let bytes = text.len();
    while start < bytes {
        start += 1;
        while start < bytes && !text.is_char_boundary(start) {
            start += 1;
        }
        if measure_text(font, &text[start..], TextStyle::INK) <= width {
            break;
        }
    }
    String::from(&text[start.min(bytes)..])
}

/// A 1-bit bar chart ("ink line"): solid bars, 2 px gaps, 2 px baseline, tallest labelled,
/// axis labels at first, middle and last.
pub fn ink_line(f: &mut Frame, r: Rect, values: &[u32], labels: (&str, &str, &str), unit: &str, hatched: Option<&[u32]>) {
    let n = values.len().max(1) as i32;
    let font = quire_fonts::ui::label();
    let mono = quire_fonts::ui::mono();
    let axis_h = line_h(mono) + 6;
    let label_h = line_h(font) + 4;
    let chart = Rect::new(r.x, r.y + label_h, r.w, r.h.saturating_sub((axis_h + label_h) as u32));
    let max = values.iter().copied().max().unwrap_or(0).max(1);
    let gap = 2;
    let bw = ((chart.w as i32 - gap * (n - 1)) / n).max(4);
    let base = chart.bottom() - 2;
    f.fill_rect(Rect::new(chart.x, base, chart.w, 2), Ink::Black);
    let mut tallest = (0usize, 0u32);
    for (i, v) in values.iter().enumerate() {
        let h = ((*v as u64 * (chart.h as u64 - 2)) / max as u64) as i32;
        let x = chart.x + i as i32 * (bw + gap);
        if h > 0 {
            f.fill_rect(Rect::new(x, base - h, bw as u32, h as u32), Ink::Black);
        }
        if let Some(hv) = hatched {
            if let Some(sv) = hv.get(i) {
                let sh = ((*sv as u64 * (chart.h as u64 - 2)) / max as u64) as i32;
                if sh > h {
                    f.pattern_rect(Rect::new(x, base - sh, bw as u32, (sh - h) as u32), Pattern::Hatch { pitch: 3 });
                }
            }
        }
        if *v > tallest.1 {
            tallest = (i, *v);
        }
    }
    if tallest.1 > 0 {
        let x = chart.x + tallest.0 as i32 * (bw + gap) + bw / 2;
        let t = alloc::format!("{}{unit}", tallest.1);
        let tw = measure_text(font, &t, TextStyle::INK);
        let lx = (x - tw / 2).clamp(r.x, r.right() - tw);
        draw_text(f, font, lx, r.y + font.ascent(), &t, TextStyle::INK);
    }
    let ay = base + 6 + mono.ascent();
    draw_text(f, mono, chart.x, ay, labels.0, TextStyle::INK);
    draw_centered(f, mono, chart.x + chart.w as i32 / 2, ay, labels.1, TextStyle::INK);
    draw_right(f, mono, chart.right(), ay, labels.2, TextStyle::INK);
}

/// A calendar heat map cell fill for a goal fraction: 0 / 25 / 50 / 100 %.
pub fn heat_fill(f: &mut Frame, r: Rect, fraction: u8) {
    f.fill_rect(r, Ink::White);
    match fraction {
        0 => {}
        1..=37 => f.pattern_rect(r, Pattern::Sparse),
        38..=87 => f.pattern_rect(r, Pattern::Dots50),
        _ => f.fill_rect(r, Ink::Black),
    }
}

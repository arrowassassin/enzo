//! The clock slot: a clean rectangle the firmware draws the live time into.
//!
//! Sizes come from the real font strikes, measured over every time of day, so a
//! slot chosen here fits `21:47` (or `00:00`) on the device with padding to spare.

use quire_fonts::Font;
use quire_gfx::{measure_text, TextStyle};
use serde::{Deserialize, Serialize};

use crate::canvas::{Canvas, Paint, Rect};

/// Which font the firmware draws the time with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClockStyle {
    /// 56 px Literata numerals (`quire_fonts::ui::hero`).
    Hero,
    /// 44 px Literata numerals (`quire_fonts::ui::poster`).
    Poster,
    /// 18 px Atkinson bold label with tracking (`quire_fonts::ui::label_bold`).
    Label,
}

impl ClockStyle {
    /// The strike the firmware uses for this style.
    pub fn font(self) -> &'static Font {
        match self {
            ClockStyle::Hero => quire_fonts::ui::hero(),
            ClockStyle::Poster => quire_fonts::ui::poster(),
            ClockStyle::Label => quire_fonts::ui::label_bold(),
        }
    }
    /// Letter spacing the firmware applies (labels are tracked like small caps).
    pub fn tracking(self) -> i32 {
        match self {
            ClockStyle::Label => 2,
            _ => 0,
        }
    }
    /// Padding around the numerals inside the slot: (horizontal, vertical).
    fn padding(self) -> (i32, i32) {
        match self {
            ClockStyle::Hero => (28, 18),
            ClockStyle::Poster => (24, 14),
            ClockStyle::Label => (16, 10),
        }
    }
    /// Slot size (w, h) that fits any `HH:MM` in this style with padding.
    pub fn slot_size(self) -> (i32, i32) {
        let font = self.font();
        let style = TextStyle { tracking: self.tracking(), ..TextStyle::INK };
        let mut max_w = 0;
        for h in 0..24 {
            for m in [0, 8, 10, 18, 33, 44, 47, 58] {
                max_w = max_w.max(measure_text(font, &format!("{h:02}:{m:02}"), style));
            }
        }
        let (asc, desc) = digit_extent(font);
        let (px, py) = self.padding();
        (max_w + 2 * px, asc + desc + 2 * py)
    }
}

/// Whether the slot is paper (the clock is drawn in ink) or ink (the clock in paper).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    /// Blank paper; draw the time in ink.
    Paper,
    /// Solid ink; draw the time in paper.
    Ink,
}

impl Surface {
    /// The paint that clears a slot of this surface.
    pub fn paint(self) -> Paint {
        match self {
            Surface::Paper => Paint::Paper,
            Surface::Ink => Paint::Ink,
        }
    }
}

/// The recorded slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockSlot {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
    /// Which font to use.
    pub style: ClockStyle,
    /// What the slot is filled with.
    pub on: Surface,
}

impl ClockSlot {
    /// A slot of the style's natural size centred at a point.
    pub fn centered(cx: i32, cy: i32, style: ClockStyle, on: Surface) -> ClockSlot {
        let (w, h) = style.slot_size();
        ClockSlot { x: cx - w / 2, y: cy - h / 2, w, h, style, on }
    }
    /// The slot as a canvas rectangle.
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
    /// Clear the slot on the canvas to its surface (call last, before finishing).
    pub fn clear(&self, c: &mut Canvas) {
        c.fill_rect(self.rect(), self.on.paint());
    }
    /// Baseline the firmware should use to centre the numerals vertically.
    pub fn baseline(&self) -> i32 {
        let (asc, desc) = digit_extent(self.style.font());
        self.y + (self.h - (asc + desc)) / 2 + asc
    }
    /// Draw a sample time into the slot exactly as the firmware would (used by the
    /// previews and the tests; the shipped PBM keeps the slot clean).
    pub fn draw_sample(&self, c: &mut Canvas, time: &str) {
        let font = self.style.font();
        let style = TextStyle { tracking: self.style.tracking(), ..TextStyle::INK };
        let w = measure_text(font, time, style);
        let x = self.x + (self.w - w) / 2;
        let paint = self.on.paint().inverse();
        crate::text::draw(c, font, x, self.baseline(), time, paint, self.style.tracking());
    }
}

/// Ascent above and descent below the baseline of the digits and colon, in pixels.
pub fn digit_extent(font: &Font) -> (i32, i32) {
    let (mut asc, mut desc) = (0, 0);
    for ch in "0123456789:".chars() {
        if let Some(g) = font.glyph(ch) {
            asc = asc.max(g.top as i32);
            desc = desc.max(g.bitmap.h as i32 - g.top as i32);
        }
    }
    (asc, desc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_sizes_are_sane() {
        let (w, h) = ClockStyle::Hero.slot_size();
        assert!((150..260).contains(&w) && (60..120).contains(&h), "hero {w}x{h}");
        let (w, h) = ClockStyle::Poster.slot_size();
        assert!((120..220).contains(&w) && (50..100).contains(&h), "poster {w}x{h}");
        let (w, h) = ClockStyle::Label.slot_size();
        assert!((60..140).contains(&w) && (25..50).contains(&h), "label {w}x{h}");
    }

    #[test]
    fn sample_stays_inside_the_slot() {
        for style in [ClockStyle::Hero, ClockStyle::Poster, ClockStyle::Label] {
            let slot = ClockSlot::centered(264, 300, style, Surface::Paper);
            let mut c = Canvas::new();
            slot.draw_sample(&mut c, "21:47");
            let r = slot.rect();
            for y in 0..c.h as i32 {
                for x in 0..c.w as i32 {
                    if c.is_ink(x, y) {
                        assert!(r.grow(-4).contains(x, y), "{style:?}: ink at {x},{y} outside {r:?}");
                    }
                }
            }
        }
    }
}

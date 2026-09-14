//! Text drawing and measuring on top of font packs.

use crate::font::Font;
use crate::frame::{BlitMode, Frame};

/// How to paint text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextStyle {
    /// Paint paper instead of ink (for text on an inverted row).
    pub inverted: bool,
    /// Dilate glyphs by one pixel ("darker text").
    pub darker: bool,
    /// Extra letter spacing in pixels (small caps labels use 1–2).
    pub tracking: i32,
}

impl TextStyle {
    /// Plain ink.
    pub const INK: TextStyle = TextStyle { inverted: false, darker: false, tracking: 0 };
    /// Paper on ink.
    pub const PAPER: TextStyle = TextStyle { inverted: true, darker: false, tracking: 0 };
}

/// Draw `s` with its baseline at `y` starting at `x`. Returns the pen x after the last glyph.
pub fn draw_text(frame: &mut Frame, font: &Font, x: i32, y: i32, s: &str, style: TextStyle) -> i32 {
    let mode = if style.inverted { BlitMode::Clear } else { BlitMode::Or };
    let mut pen_q = x << 2; // quarter pixels
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if let Some(p) = prev {
            pen_q += font.kern_q(p, c);
        }
        let g = font.glyph_or_fallback(c);
        let gx = (pen_q >> 2) + g.bearing_x as i32;
        let gy = y - g.top as i32;
        if g.bitmap.w > 0 {
            if style.darker {
                frame.blit_bold(gx, gy, g.bitmap, mode);
            } else {
                frame.blit(gx, gy, g.bitmap, mode);
            }
        }
        pen_q += g.advance_q as i32 + (style.tracking << 2);
        prev = Some(c);
    }
    pen_q >> 2
}

/// Draw `s` rotated 90° counter-clockwise, reading from bottom to top: the baseline is the
/// vertical line at `x` and the pen starts at `y` and moves upwards. Glyph bitmaps are
/// blitted transposed, so no scratch frame is needed. Returns the pen y after the last glyph.
pub fn draw_text_ccw(frame: &mut Frame, font: &Font, x: i32, y: i32, s: &str, style: TextStyle) -> i32 {
    let mode = if style.inverted { BlitMode::Clear } else { BlitMode::Or };
    let mut pen_q = 0i32;
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if let Some(p) = prev {
            pen_q += font.kern_q(p, c);
        }
        let g = font.glyph_or_fallback(c);
        if g.bitmap.w > 0 {
            let gx = (pen_q >> 2) + g.bearing_x as i32;
            let top = y - gx - g.bitmap.w as i32 + 1;
            frame.blit_ccw(x - g.top as i32, top, g.bitmap, mode);
            if style.darker {
                frame.blit_ccw(x - g.top as i32, top - 1, g.bitmap, mode);
            }
        }
        pen_q += g.advance_q as i32 + (style.tracking << 2);
        prev = Some(c);
    }
    y - (pen_q >> 2)
}

/// Width of `s` in pixels if drawn with `font` and `style`.
pub fn measure_text(font: &Font, s: &str, style: TextStyle) -> i32 {
    let mut pen_q = 0i32;
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if let Some(p) = prev {
            pen_q += font.kern_q(p, c);
        }
        let g = font.glyph_or_fallback(c);
        pen_q += g.advance_q as i32 + (style.tracking << 2);
        prev = Some(c);
    }
    (pen_q + 2) >> 2
}

/// Width in quarter pixels (used by the layout engine to avoid rounding drift).
pub fn measure_text_q(font: &Font, s: &str) -> i32 {
    let mut pen_q = 0i32;
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if let Some(p) = prev {
            pen_q += font.kern_q(p, c);
        }
        pen_q += font.glyph_or_fallback(c).advance_q as i32;
        prev = Some(c);
    }
    pen_q
}

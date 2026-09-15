//! Text on the canvas, set with the device's own font packs so the art matches the
//! firmware's typography.

use quire_fonts::Font;
use quire_gfx::{draw_text, measure_text, Frame, TextStyle};

use crate::canvas::{Canvas, Paint};

/// Draw `s` with its baseline at `y`, starting at `x`. Returns the pen x afterwards.
pub fn draw(c: &mut Canvas, font: &Font, x: i32, y: i32, s: &str, p: Paint, tracking: i32) -> i32 {
    let style = TextStyle { tracking, ..TextStyle::INK };
    let w = measure_text(font, s, style) + 4;
    let asc = font.ascent().max(1);
    let h = asc + (-font.descent()).max(0) + 4;
    let mut frame = Frame::new(w.max(1) as u32, h as u32);
    let pen = draw_text(&mut frame, font, 0, asc, s, style);
    for fy in 0..frame.height() as i32 {
        for fx in 0..frame.width() as i32 {
            if frame.get(fx, fy) {
                c.put(x + fx, y - asc + fy, p);
            }
        }
    }
    x + pen
}

/// Width of `s` in pixels.
pub fn width(font: &Font, s: &str, tracking: i32) -> i32 {
    measure_text(font, s, TextStyle { tracking, ..TextStyle::INK })
}

/// Draw `s` centred on `cx`.
pub fn centered(c: &mut Canvas, font: &Font, cx: i32, y: i32, s: &str, p: Paint, tracking: i32) {
    let w = width(font, s, tracking);
    draw(c, font, cx - w / 2, y, s, p, tracking);
}

/// Greedy word wrap to `max_w` pixels.
pub fn wrap(font: &Font, s: &str, max_w: i32, tracking: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if width(font, &cand, tracking) <= max_w || cur.is_empty() {
            cur = cand;
        } else {
            lines.push(std::mem::replace(&mut cur, word.to_string()));
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Small caps the way the UI sets labels: upper-case with tracking.
pub fn small_caps(s: &str) -> String {
    s.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_respects_width() {
        let f = quire_fonts::ui::body();
        let lines = wrap(f, "the quick brown fox jumps over the lazy dog again and again", 200, 0);
        assert!(lines.len() >= 3);
        for l in &lines {
            assert!(width(f, l, 0) <= 200, "{l}");
        }
    }

    #[test]
    fn text_lands_on_the_canvas() {
        let mut c = Canvas::sized(200, 60);
        let f = quire_fonts::ui::title();
        draw(&mut c, f, 4, 40, "Quire", Paint::Ink, 0);
        let bm = c.finish();
        assert!(crate::canvas::ink_density(&bm) > 0.01);
    }
}

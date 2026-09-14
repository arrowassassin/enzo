//! UI text helpers: wrapping, ellipsis, small caps, centred and right-aligned text.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, measure_text, Font, Frame, TextStyle};

use crate::theme::SMALLCAP_TRACKING;

/// Advance of `s` in quarter pixels when it follows `prev` (kerning included), and its
/// last character.
fn advance_q(font: &Font, prev: Option<char>, s: &str) -> (i32, Option<char>) {
    let mut q = 0i32;
    let mut p = prev;
    for c in s.chars() {
        if let Some(p) = p {
            q += font.kern_q(p, c);
        }
        q += font.glyph_or_fallback(c).advance_q as i32;
        p = Some(c);
    }
    (q, p)
}

/// Whole pixels of a quarter-pixel advance, rounded as `measure_text` rounds.
#[inline]
fn px(q: i32) -> i32 {
    (q + 2) >> 2
}

/// Greedy word wrap into lines that fit `width`. Measures incrementally (each glyph once)
/// and allocates one String per output line; a word wider than the line is split by
/// characters wherever it lands.
pub fn wrap(font: &Font, text: &str, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut line_q = 0i32;
        let mut last: Option<char> = None;
        for word in para.split_whitespace() {
            if !line.is_empty() {
                let (space_q, after_space) = advance_q(font, last, " ");
                let (word_q, word_last) = advance_q(font, after_space, word);
                if px(line_q + space_q + word_q) <= width {
                    line.push(' ');
                    line.push_str(word);
                    line_q += space_q + word_q;
                    last = word_last;
                    continue;
                }
                lines.push(core::mem::take(&mut line));
            }
            line.push_str(word);
            let (q, l) = advance_q(font, None, word);
            line_q = q;
            last = l;
            // A single word wider than the line: break it by characters, keeping the
            // longest prefix that fits.
            while px(line_q) > width && line.chars().count() > 1 {
                let mut q = 0i32;
                let mut prev: Option<char> = None;
                let mut cut = 0usize;
                for (i, c) in line.char_indices() {
                    let (a, _) = advance_q(font, prev, c.encode_utf8(&mut [0; 4]));
                    if px(q + a) > width {
                        break;
                    }
                    q += a;
                    prev = Some(c);
                    cut = i + c.len_utf8();
                }
                if cut == 0 {
                    break;
                }
                lines.push(String::from(&line[..cut]));
                line = String::from(&line[cut..]);
                let (rest_q, rest_last) = advance_q(font, None, &line);
                line_q = rest_q;
                last = rest_last;
            }
        }
        lines.push(line);
    }
    lines
}

/// Truncate with an ellipsis so the text fits `width` (one pass over the glyphs).
pub fn ellipsis(font: &Font, text: &str, width: i32) -> String {
    if measure_text(font, text, TextStyle::INK) <= width {
        return String::from(text);
    }
    let dots = "…";
    let dots_w = measure_text(font, dots, TextStyle::INK);
    let mut q = 0i32;
    let mut prev: Option<char> = None;
    let mut cut = 0usize;
    for (i, c) in text.char_indices() {
        let (a, _) = advance_q(font, prev, c.encode_utf8(&mut [0; 4]));
        if px(q + a) + dots_w > width {
            break;
        }
        q += a;
        prev = Some(c);
        cut = i + c.len_utf8();
    }
    let trimmed = text[..cut].trim_end();
    alloc::format!("{trimmed}{dots}")
}

/// Draw text right-aligned to `right`.
pub fn draw_right(frame: &mut Frame, font: &Font, right: i32, baseline: i32, s: &str, style: TextStyle) -> i32 {
    let w = measure_text(font, s, style);
    draw_text(frame, font, right - w, baseline, s, style)
}

/// Draw text centred on `cx`.
pub fn draw_centered(frame: &mut Frame, font: &Font, cx: i32, baseline: i32, s: &str, style: TextStyle) -> i32 {
    let w = measure_text(font, s, style);
    draw_text(frame, font, cx - w / 2, baseline, s, style)
}

/// Small-cap label: upper case with tracking, in the label face.
pub fn small_caps(s: &str) -> String {
    s.to_uppercase()
}

/// Style for small-cap labels.
pub fn label_style(inverted: bool) -> TextStyle {
    TextStyle { inverted, darker: false, tracking: SMALLCAP_TRACKING }
}

/// Draw a small-cap label.
pub fn draw_label(frame: &mut Frame, x: i32, baseline: i32, s: &str, inverted: bool) -> i32 {
    draw_text(frame, quire_fonts::ui::label(), x, baseline, &small_caps(s), label_style(inverted))
}

/// Draw wrapped body text from `y` (first baseline at `y + ascent`); returns the y after the last line.
#[allow(clippy::too_many_arguments)]
pub fn draw_wrapped(
    frame: &mut Frame,
    font: &Font,
    x: i32,
    y: i32,
    width: i32,
    line_h: i32,
    text: &str,
    style: TextStyle,
    max_lines: usize,
) -> i32 {
    let lines = wrap(font, text, width);
    let mut yy = y;
    for line in lines.iter().take(max_lines) {
        draw_text(frame, font, x, yy + font.ascent(), line, style);
        yy += line_h;
    }
    yy
}

/// Line height for a UI font (1.3).
pub fn line_h(font: &Font) -> i32 {
    (font.size() as i32 * 13 + 5) / 10
}

/// Baseline for text vertically centred in a row of `h` starting at `y`.
pub fn centered_baseline(font: &Font, y: i32, h: i32) -> i32 {
    y + (h - (font.ascent() + font.below())) / 2 + font.ascent()
}

/// Two-line title/subtitle pairs for the rows: ensure `s` fits or gets an ellipsis.
pub fn fit(font: &Font, s: &str, width: i32) -> String {
    ellipsis(font, s, width)
}

/// "1 / 9"-style page indicator.
pub fn page_indicator(page: usize, pages: usize) -> String {
    alloc::format!("{} / {}", page + 1, pages.max(1))
}

/// Break long text into pages of `lines_per_page` wrapped lines.
pub fn paginate(font: &Font, text: &str, width: i32, lines_per_page: usize) -> Vec<Vec<String>> {
    let lines = wrap(font, text, width);
    if lines.is_empty() {
        return alloc::vec![Vec::new()];
    }
    lines.chunks(lines_per_page.max(1)).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The incremental wrap and ellipsis agree with whole-string measurement.
    #[test]
    fn wrap_and_ellipsis_fit_their_width() {
        let font = quire_fonts::ui::body();
        let text = "Call me Ishmael. Some years ago—never mind how long precisely—having little or no money in my purse, \
                    and nothing particular to interest me on shore, I thought I would sail about a little and see the \
                    watery part of the world. Antidisestablishmentarianismxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx end.\nNext para";
        for width in [80, 137, 200, 333] {
            let lines = wrap(font, text, width);
            assert!(lines.len() > 2);
            for l in &lines {
                let w = measure_text(font, l, TextStyle::INK);
                assert!(w <= width || l.chars().count() == 1, "{l:?} is {w} px wide at {width}");
            }
            // Greedy: within a paragraph, adding the next line's first word would overflow.
            let para: Vec<String> = wrap(font, text.split('\n').next().unwrap(), width);
            for pair in para.windows(2) {
                let first = pair[1].split_whitespace().next().unwrap_or("");
                let joined = alloc::format!("{} {}", pair[0], first);
                assert!(measure_text(font, &joined, TextStyle::INK) > width, "{joined:?} fits {width}");
            }
            let joined: Vec<String> = lines.iter().map(|l| l.replace(' ', "")).collect();
            assert_eq!(joined.concat(), text.replace([' ', '\n'], ""), "no text lost at {width}");
            let e = ellipsis(font, text, width);
            assert!(measure_text(font, &e, TextStyle::INK) <= width, "{e:?} at {width}");
            assert!(e.ends_with('…'));
        }
        assert_eq!(ellipsis(font, "short", 500), "short");
    }
}

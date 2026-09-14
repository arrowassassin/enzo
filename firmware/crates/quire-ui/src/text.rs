//! UI text helpers: wrapping, ellipsis, small caps, centred and right-aligned text.

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, measure_text, Font, Frame, TextStyle};

use crate::theme::SMALLCAP_TRACKING;

/// Greedy word wrap into lines that fit `width`.
pub fn wrap(font: &Font, text: &str, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { String::from(word) } else { alloc::format!("{line} {word}") };
            if measure_text(font, &candidate, TextStyle::INK) <= width || line.is_empty() {
                line = candidate;
                // A single word wider than the line: break it by characters.
                while measure_text(font, &line, TextStyle::INK) > width && line.chars().count() > 1 {
                    let mut cut = line.len();
                    while cut > 0 {
                        cut -= 1;
                        if line.is_char_boundary(cut) && measure_text(font, &line[..cut], TextStyle::INK) <= width {
                            break;
                        }
                    }
                    if cut == 0 {
                        break;
                    }
                    lines.push(String::from(&line[..cut]));
                    line = String::from(&line[cut..]);
                }
            } else {
                lines.push(core::mem::take(&mut line));
                line = String::from(word);
            }
        }
        lines.push(line);
    }
    lines
}

/// Truncate with an ellipsis so the text fits `width`.
pub fn ellipsis(font: &Font, text: &str, width: i32) -> String {
    if measure_text(font, text, TextStyle::INK) <= width {
        return String::from(text);
    }
    let dots = "…";
    let dots_w = measure_text(font, dots, TextStyle::INK);
    let mut out = String::new();
    for c in text.chars() {
        let mut t = out.clone();
        t.push(c);
        if measure_text(font, &t, TextStyle::INK) + dots_w > width {
            break;
        }
        out = t;
    }
    let trimmed = out.trim_end();
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

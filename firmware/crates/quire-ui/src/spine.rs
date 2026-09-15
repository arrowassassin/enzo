//! The Spine: a book's fore-edge as a progress strip (brief §3).

use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, measure_text, Frame, Ink, Rect, TextStyle};

use crate::theme::SPINE_W;

/// The model behind a Spine: total pages, the chapter starts, the current page.
#[derive(Clone, Debug, Default)]
pub struct SpineModel {
    /// Total pages in the book (or an estimate).
    pub total: u32,
    /// Page numbers where chapters start.
    pub chapters: Vec<u32>,
    /// Current page (0-based).
    pub current: u32,
}

impl SpineModel {
    /// Fraction read in 1/1000.
    pub fn permille(&self) -> u32 {
        if self.total <= 1 {
            return 0;
        }
        (self.current as u64 * 1000 / (self.total as u64 - 1)).min(1000) as u32
    }
}

/// Draw the Spine in a strip `r` (12 px wide). `pitch` is the hairline pitch (4 px).
pub fn draw(f: &mut Frame, r: Rect, m: &SpineModel, ink: Ink) {
    let pitch = 4;
    let lines = (r.h as i32 / pitch).max(1);
    let read_lines = if m.total <= 1 { 0 } else { ((m.current as u64 * lines as u64) / (m.total as u64 - 1).max(1)) as i32 };
    let read_lines = read_lines.min(lines);
    for i in 0..lines {
        let y = r.y + i * pitch;
        let w = if i < read_lines { r.w } else { 6 };
        f.fill_rect(Rect::new(r.x, y, w, 1), ink);
    }
    // Chapter notches on the outer (right) side.
    for c in &m.chapters {
        if m.total <= 1 {
            break;
        }
        let y = r.y + ((*c as u64 * (r.h as u64 - 3)) / (m.total as u64 - 1).max(1)) as i32;
        f.fill_rect(Rect::new(r.right() - 3, y, 3, 3), ink);
    }
    // Position bar.
    let by = r.y + ((m.current as u64 * (r.h as u64 - 2)) / (m.total as u64 - 1).max(1)) as i32;
    f.fill_rect(Rect::new(r.x, by, r.w, 2), ink);
}

/// The skim label riding beside the position bar: "Ch 14 · 51%" / "2 h 03 left".
pub fn draw_skim_label(f: &mut Frame, r: Rect, m: &SpineModel, line1: &str, line2: &str) {
    let font = quire_fonts::ui::label();
    let w = measure_text(font, line1, TextStyle::INK).max(measure_text(font, line2, TextStyle::INK)) + 20;
    let h = 2 * crate::text::line_h(font) + 12;
    let by = r.y + ((m.current as u64 * (r.h as u64 - 2)) / (m.total as u64 - 1).max(1)) as i32;
    // Keep a breath of air under the running head above the strip.
    let y = (by - h / 2).clamp(r.y + 8, r.bottom() - h);
    let x = r.x - 12 - w;
    let card = Rect::new(x, y, w as u32, h as u32);
    f.fill_rect(card, Ink::White);
    f.stroke_rect(card, 2, Ink::Black);
    draw_text(f, font, x + 10, y + 6 + font.ascent(), line1, TextStyle::INK);
    draw_text(f, font, x + 10, y + 6 + crate::text::line_h(font) + font.ascent(), line2, TextStyle::INK);
    // A tick from the card to the bar.
    f.fill_rect(Rect::new(card.right(), by, 12, 1), Ink::Black);
}

/// A miniature Spine (sleep-screen band): 6 px wide, hairlines at 2 px pitch.
pub fn draw_mini(f: &mut Frame, r: Rect, m: &SpineModel, ink: Ink) {
    let pitch = 2;
    let lines = (r.h as i32 / pitch).max(1);
    let read_lines = if m.total <= 1 { 0 } else { ((m.current as u64 * lines as u64) / (m.total as u64 - 1).max(1)) as i32 };
    for i in 0..lines {
        let w = if i < read_lines { r.w } else { r.w / 2 };
        f.fill_rect(Rect::new(r.x, r.y + i * pitch, w, 1), ink);
    }
}

/// A goal Spine (60e): hairlines = the goal, solid fill = done, fills upward.
pub fn draw_goal(f: &mut Frame, r: Rect, done: u32, goal: u32, ink: Ink) {
    let goal = goal.max(1);
    let pitch = 4;
    let lines = (r.h as i32 / pitch).max(1);
    let done_lines = ((done.min(goal) as u64 * lines as u64) / goal as u64) as i32;
    for i in 0..lines {
        let y = r.bottom() - 1 - i * pitch;
        if i < done_lines {
            f.fill_rect(Rect::new(r.x, y - (pitch - 1), r.w, pitch as u32), ink);
        } else {
            f.fill_rect(Rect::new(r.x, y, r.w, 1), ink);
        }
    }
}

/// The default Spine rectangle on the reading page for a frame `w × h`: inside the right
/// margin, from below the running head to above the bottom margin.
pub fn strip_rect(w: u32, h: u32, top: i32, bottom_margin: i32) -> Rect {
    Rect::new(
        w as i32 - crate::theme::MARGIN + (crate::theme::MARGIN - SPINE_W) / 2,
        top,
        SPINE_W as u32,
        (h as i32 - bottom_margin - top).max(8) as u32,
    )
}

/// "Ch 14 · 51%" for the skim label.
pub fn chapter_percent_label(chapter: Option<u32>, permille: u32) -> String {
    match chapter {
        Some(c) => alloc::format!("Ch {c} · {}%", permille / 10),
        None => alloc::format!("{}%", permille / 10),
    }
}

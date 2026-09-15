//! 28 highlights and notes, grouped by chapter.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use quire_library::marks::MarkKind;

use crate::keyboard::KeyboardScreen;
use crate::text::{ellipsis, line_h, page_indicator, wrap};
use crate::theme::*;
use crate::widgets::{self, rail, running_head};
use crate::{Action, Ctx, Env, Key, KeyEvent, KeyKind, Refresh, Result_, Screen};

/// A measured mark block: (mark index, height, chapter header, quote lines, note lines).
type Block = (usize, i32, String, Vec<String>, Vec<String>);

/// The highlights screen.
pub struct Highlights {
    focus: usize,
    page: usize,
    /// Mark indexes per page (built at draw).
    layout: Vec<Vec<usize>>,
}

impl Highlights {
    /// New.
    pub fn new() -> Self {
        Highlights { focus: 0, page: 0, layout: Vec::new() }
    }
}

impl Default for Highlights {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Env> Screen<E> for Highlights {
    fn name(&self) -> &'static str {
        "28-highlights"
    }
    fn draw(&mut self, cx: &mut Ctx<E>, f: &mut Frame) -> Refresh {
        let Some(reader) = cx.reader.as_mut() else { return Refresh::Du };
        let marks: Vec<(usize, &quire_library::marks::Mark)> =
            reader.marks.items.iter().enumerate().filter(|(_, m)| m.kind != MarkKind::Bookmark).collect();
        let fs = quire_fonts::ui::serif_small();
        let fl = quire_fonts::ui::label();
        let w = f.width() as i32 - 2 * widgets::INSET;
        // Measure every mark into blocks, then page them.
        let mut blocks: Vec<Block> = Vec::new();
        let mut last_chapter: Option<usize> = None;
        for (i, m) in &marks {
            let ch = reader.book.toc_index_at_chars(m.chars);
            let header = if ch != last_chapter {
                last_chapter = ch;
                ch.and_then(|c| reader.book.toc.get(c)).map(|e| e.title.clone()).unwrap_or_else(|| String::from("Chapter"))
            } else {
                String::new()
            };
            let quote = wrap(fs, &alloc::format!("\u{201c}{}\u{201d}", m.excerpt), w);
            let note = if m.note.is_empty() { wrap(fl, "no note yet — Up adds one", w) } else { wrap(fl, &m.note, w) };
            let h = (if header.is_empty() { 0 } else { line_h(fl) + 8 })
                + quote.len() as i32 * line_h(fs)
                + note.len() as i32 * line_h(fl)
                + 20;
            blocks.push((*i, h, header, quote, note));
        }
        let avail = f.height() as i32 - RAIL_H - widgets::CONTENT_TOP - 8;
        self.layout.clear();
        let mut cur: Vec<usize> = Vec::new();
        let mut used = 0;
        for (bi, b) in blocks.iter().enumerate() {
            if used + b.1 > avail && !cur.is_empty() {
                self.layout.push(core::mem::take(&mut cur));
                used = 0;
            }
            cur.push(bi);
            used += b.1;
        }
        if !cur.is_empty() || self.layout.is_empty() {
            self.layout.push(cur);
        }
        // Keep the focus visible.
        let page = self.layout.iter().position(|p| p.contains(&self.focus)).unwrap_or(0);
        self.page = page;
        let title = alloc::format!("Highlights {}", if marks.is_empty() { String::new() } else { alloc::format!("{}", marks.len()) });
        running_head(f, title.trim(), Some(&page_indicator(page, self.layout.len())));
        if marks.is_empty() {
            widgets::empty_state(
                f,
                widgets::EMPTY_Y,
                "No highlights yet",
                "Long-press Confirm on the page, then grow a selection with Right.",
            );
            rail(f, ["", "Back", "", ""], None);
            return Refresh::Gc;
        }
        let mut y = widgets::CONTENT_TOP;
        for &bi in &self.layout[page] {
            let (_, h, header, quote, note) = &blocks[bi];
            if !header.is_empty() {
                crate::text::draw_label(f, widgets::INSET, y + fl.ascent() + 2, header, false);
                y += line_h(fl) + 8;
            }
            let focused = bi == self.focus;
            let block_h = quote.len() as i32 * line_h(fs) + note.len() as i32 * line_h(fl) + 12;
            if focused {
                f.fill_rect(Rect::new(widgets::INSET - 8, y - 4, (w + 16) as u32, block_h as u32), Ink::Black);
            }
            let style = TextStyle { inverted: focused, ..TextStyle::INK };
            for l in quote {
                draw_text(f, fs, widgets::INSET, y + fs.ascent(), l, style);
                y += line_h(fs);
            }
            for l in note {
                draw_text(f, fl, widgets::INSET, y + fl.ascent() + 2, &ellipsis(fl, l, w), style);
                y += line_h(fl);
            }
            y += 20;
            let _ = h;
        }
        widgets::side_labels(f, Some("Note"), Some("Delete"), true);
        rail(f, ["", "Back", "Open", ""], None);
        Refresh::Gc
    }
    fn key(&mut self, cx: &mut Ctx<E>, ev: KeyEvent) -> Action<E> {
        if ev.kind != KeyKind::Press {
            return Action::None;
        }
        let Some(reader) = cx.reader.as_mut() else { return Action::Pop };
        let ids: Vec<usize> = reader.marks.items.iter().enumerate().filter(|(_, m)| m.kind != MarkKind::Bookmark).map(|(i, _)| i).collect();
        let n = ids.len();
        match ev.key {
            Key::Back => Action::Pop,
            Key::Down | Key::Right => {
                if n > 0 {
                    self.focus = (self.focus + 1) % n;
                }
                Action::Redraw
            }
            Key::Left => {
                if n > 0 {
                    self.focus = (self.focus + n - 1) % n;
                }
                Action::Redraw
            }
            Key::Up => {
                // Add or edit the note on the phone or the keyboard.
                let Some(&mi) = ids.get(self.focus) else { return Action::None };
                let note = reader.marks.items[mi].note.clone();
                Action::Push(KeyboardScreen::new("Note", &note, "Type a note").boxed())
            }
            Key::Confirm => {
                let Some(&mi) = ids.get(self.focus) else { return Action::None };
                let m = &reader.marks.items[mi];
                let loc = quire_library::Loc { section: m.section, pos: m.pos, chars: m.chars };
                let fs = cx.env.fs();
                reader.push_return();
                reader.goto(fs, loc);
                reader.force_gc();
                Action::ToReader
            }
            Key::Power => Action::None,
        }
    }
    fn result(&mut self, cx: &mut Ctx<E>, r: Result_) -> Action<E> {
        if let Result_::Text(t) = r {
            let fs = cx.env.fs();
            if let Some(reader) = cx.reader.as_mut() {
                let ids: Vec<usize> =
                    reader.marks.items.iter().enumerate().filter(|(_, m)| m.kind != MarkKind::Bookmark).map(|(i, _)| i).collect();
                if let Some(&mi) = ids.get(self.focus) {
                    let m = &mut reader.marks.items[mi];
                    m.note = t;
                    m.kind = if m.note.is_empty() { MarkKind::Highlight } else { MarkKind::Note };
                    let _ = reader.marks.save(fs, &reader.book.dir);
                }
            }
        }
        Action::Redraw
    }
}

/// Delete the focused mark (long-Down from the side label).
pub fn delete_focused<E: Env>(cx: &mut Ctx<E>, focus: usize) -> Action<E> {
    let fs = cx.env.fs();
    if let Some(reader) = cx.reader.as_mut() {
        let ids: Vec<usize> = reader.marks.items.iter().enumerate().filter(|(_, m)| m.kind != MarkKind::Bookmark).map(|(i, _)| i).collect();
        if let Some(&mi) = ids.get(focus) {
            reader.marks.remove(mi);
            let _ = reader.marks.save(fs, &reader.book.dir);
        }
    }
    Action::Redraw
}

/// Boxed constructor for Jump.
pub fn boxed() -> Box<Highlights> {
    Box::new(Highlights::new())
}

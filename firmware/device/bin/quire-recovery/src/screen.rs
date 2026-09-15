//! The one recovery screen (design `99-recovery`): running head "Recovery" with the rule
//! at y=74, why the reader is here, the two slots with their versions, the card, a status
//! line for progress and errors, and the rail Retry / Card / Rollback. Drawn with the
//! 18 px label and mono faces only, so the app fits the factory partition.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use quire_board::flash::Flash;
use quire_board::image::app_desc;
use quire_board::ota::{self, AppSlot, Diagnosis, ImageState};
use quire_board::otadata::SLOT_COUNT;
use quire_gfx::{draw_text, measure_text, Font, Frame, Ink, Rect, TextStyle};

/// Content x inset, as on every titled screen.
pub const INSET: i32 = 32;
/// Y of the running head's 4 px rule.
const HEAD_RULE_Y: i32 = 74;
/// Where content starts under the head.
const CONTENT_TOP: i32 = 92;
/// The rail's height.
const RAIL_H: i32 = 40;
/// Heavy rule thickness.
const RULE_HEAVY: u32 = 4;
/// Top of the status block (progress and errors).
const STATUS_Y: i32 = 640;

/// The label face (Atkinson 18).
pub fn label() -> &'static Font {
    &quire_fonts::strikes::ATKINSON_REGULAR_18
}
/// The mono face (JetBrains Mono 18).
pub fn mono() -> &'static Font {
    &quire_fonts::strikes::MONO_REGULAR_18
}

fn line_h(font: &Font) -> i32 {
    (font.size() as i32 * 13 + 5) / 10
}

/// Greedy word wrap to `width` pixels; paragraphs split on `\n`.
pub fn wrap(font: &Font, text: &str, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { String::from(word) } else { format!("{line} {word}") };
            if measure_text(font, &candidate, TextStyle::INK) <= width || line.is_empty() {
                line = candidate;
            } else {
                lines.push(core::mem::replace(&mut line, String::from(word)));
            }
        }
        lines.push(line);
    }
    lines
}

/// Draw wrapped text from the top `y`; returns the y below the last line.
fn paragraph(f: &mut Frame, font: &Font, y: i32, text: &str, width: i32) -> i32 {
    let mut y = y;
    for l in wrap(font, text, width) {
        draw_text(f, font, INSET, y + font.ascent(), &l, TextStyle::INK);
        y += line_h(font);
    }
    y
}

/// The rail: four cells over the four keys, labels centred in the mono face.
fn rail(f: &mut Frame, labels: [&str; 4]) {
    let w = f.width() as i32;
    let h = f.height() as i32;
    let y = h - RAIL_H;
    f.fill_rect(Rect::new(0, y, w as u32, RAIL_H as u32), Ink::White);
    f.fill_rect(Rect::new(0, y, w as u32, RULE_HEAVY), Ink::Black);
    let cell = w / 4;
    let font = mono();
    let top = y + RULE_HEAVY as i32;
    let inner = RAIL_H - RULE_HEAVY as i32;
    for (i, label) in labels.iter().enumerate() {
        let x = i as i32 * cell;
        if i < 3 {
            f.fill_rect(Rect::new(x + cell - 1, top, 1, inner as u32), Ink::Black);
        }
        let cx = x + cell / 2;
        if label.is_empty() {
            f.fill_rect(Rect::new(cx - 1, top + inner / 2 - 1, 2, 2), Ink::Black);
        } else {
            let tw = measure_text(font, label, TextStyle::INK);
            let base = top + (inner - (font.ascent() + font.below())) / 2 + font.ascent();
            draw_text(f, font, cx - tw / 2, base, label, TextStyle::INK);
        }
    }
}

/// What the screen shows.
pub struct State {
    /// Why the reader is here.
    pub reason: String,
    /// One mono line per slot.
    pub slots: Vec<String>,
    /// The card line.
    pub card: String,
    /// Progress or the last error.
    pub status: String,
    /// The panel controller, for the foot line.
    pub panel: &'static str,
}

impl State {
    /// Read the otadata and both slot headers; `update` is the card file found, if any.
    pub fn gather(flash: &Flash, card: Option<&str>, update: Option<&str>, panel: &'static str) -> State {
        let data = ota::otadata(flash).ok();
        let reason = match data.map(|d| d.diagnosis()) {
            None => String::from("The partition table could not be read, so nothing is known about the installed firmware."),
            Some(Diagnosis::Empty) => {
                String::from("No firmware is selected. The reader was just flashed, or Recovery was chosen from Settings.")
            }
            Some(Diagnosis::Abandoned(s)) => {
                format!(
                    "The update in slot {} did not finish booting and was abandoned by the bootloader, and nothing else was bootable.",
                    s.letter()
                )
            }
            Some(Diagnosis::Unbootable(s)) => format!("Slot {} is selected, but its firmware could not be loaded.", s.letter()),
        };
        let selected = data.and_then(|d| d.selected());
        let mut slots = Vec::new();
        for slot in [AppSlot::Ota0, AppSlot::Ota1] {
            let head = ota::slot_head(flash, slot).ok();
            let mut line = format!("slot {}  ", slot.letter());
            match head.as_ref().and_then(|h| app_desc(h)) {
                Some(d) => line.push_str(&format!("{} {}  {}", d.project, d.version, d.date)),
                None => line.push_str("empty"),
            }
            if selected == Some(slot) {
                line.push_str("  selected");
            }
            let state = data.and_then(|d| {
                d.entries
                    .iter()
                    .filter(|e| e.written() && (e.seq - 1) % SLOT_COUNT == slot.index())
                    .max_by_key(|e| e.seq)
                    .map(|e| e.image_state())
            });
            if let Some(s) = state.filter(|s| *s != ImageState::Undefined) {
                line.push_str(&format!("  {}", s.name()));
            }
            slots.push(line);
        }
        let card = match (card, update) {
            (None, _) => String::from("card: none"),
            (Some(_), Some(p)) => format!("card: present  {p}"),
            (Some(_), None) => String::from("card: present  no update file"),
        };
        State { reason, slots, card, status: String::new(), panel }
    }

    /// Draw the whole screen.
    pub fn draw(&self, f: &mut Frame) {
        f.clear(Ink::White);
        let w = f.width() as i32;
        let width = w - 2 * INSET;
        let lf = label();
        let mf = mono();

        // Running head.
        let base = HEAD_RULE_Y - 8 - lf.below();
        draw_text(f, lf, INSET, base, "Recovery", TextStyle::INK);
        let right = format!("recovery {}", env!("CARGO_PKG_VERSION"));
        let rw = measure_text(mf, &right, TextStyle::INK);
        draw_text(f, mf, w - INSET - rw, base, &right, TextStyle::INK);
        f.fill_rect(Rect::new(INSET, HEAD_RULE_Y, width as u32, RULE_HEAVY), Ink::Black);

        // Why, then what the keys do.
        let mut y = CONTENT_TOP + 8;
        y = paragraph(f, lf, y, &self.reason, width);
        y += 12;
        y = paragraph(
            f,
            lf,
            y,
            "Retry boots the selected firmware again. Card installs the update file from the microSD card into the spare slot; books and settings stay. Rollback switches to the other slot.",
            width,
        );

        // The slots and the card.
        y += 24;
        for l in self.slots.iter().chain(core::iter::once(&self.card)) {
            for part in wrap(mf, l, width) {
                draw_text(f, mf, INSET, y + mf.ascent(), &part, TextStyle::INK);
                y += line_h(mf) + 4;
            }
        }
        y += 4;
        let foot = format!("panel {}", self.panel);
        draw_text(f, mf, INSET, y + mf.ascent(), &foot, TextStyle::INK);

        // Status: progress or the last error, above the rail.
        let mut sy = STATUS_Y;
        if !self.status.is_empty() {
            f.fill_rect(Rect::new(INSET, sy - 12, width as u32, 2), Ink::Black);
            for l in wrap(mf, &self.status, width).into_iter().take(3) {
                draw_text(f, mf, INSET, sy + mf.ascent(), &l, TextStyle::INK);
                sy += line_h(mf) + 2;
            }
        }

        rail(f, ["", "Retry", "Card", "Rollback"]);
    }
}

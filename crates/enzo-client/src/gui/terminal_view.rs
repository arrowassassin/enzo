//! Terminal grid painter for egui.
//!
//! Renders the [`Terminal`] cell grid with a monospace font, one `LayoutJob` per
//! row (grouping consecutive same-colour cells into runs) for efficiency, and
//! draws a phosphor block cursor. Returns the column/row count that fits the
//! available area so the caller can resize the PTY to match.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, FontFamily, FontId, Rect, Sense, Stroke, Vec2, pos2};

use super::theme;
use crate::terminal::{Cell, Terminal};

/// Result of painting the terminal: the visible grid size that fit the area.
pub struct TermFit {
    /// Columns that fit horizontally.
    pub cols: u16,
    /// Rows that fit vertically.
    pub rows: u16,
}

/// Paint `terminal` filling the current `ui` and return the fitted grid size.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "grid coords are small and positive"
)]
pub fn show(ui: &mut egui::Ui, terminal: &Terminal) -> TermFit {
    let font = FontId::new(13.5, FontFamily::Monospace);
    let (char_w, line_h) = ui.fonts(|f| {
        (
            f.glyph_width(&font, 'M').max(1.0),
            f.row_height(&font).max(1.0),
        )
    });

    let avail = ui.available_size();
    let rect = ui.allocate_space(avail).1;
    let painter = ui.painter_at(rect);

    // Solid terminal background.
    painter.rect_filled(rect, egui::CornerRadius::ZERO, theme::BG_SURFACE);

    let fit_cols = ((avail.x / char_w).floor() as u16).max(1);
    let fit_rows = ((avail.y / line_h).floor() as u16).max(1);

    let cols = terminal.cols();
    let rows = terminal.rows();
    let cells = terminal.cells();
    let (cursor_col, cursor_row) = terminal.cursor();

    let draw_rows = rows.min(fit_rows);
    let draw_cols = cols.min(fit_cols);

    for row in 0..draw_rows {
        let y = rect.top() + f32::from(row) * line_h;
        let mut job = LayoutJob::default();
        let mut any = false;

        // Group consecutive cells of the same colour into one run.
        let mut run = String::new();
        let mut run_color = theme::TERM_FG;
        let flush = |job: &mut LayoutJob, run: &mut String, color: Color32, font: &FontId| {
            if !run.is_empty() {
                job.append(
                    run,
                    0.0,
                    TextFormat {
                        font_id: font.clone(),
                        color,
                        ..Default::default()
                    },
                );
                run.clear();
            }
        };

        for col in 0..draw_cols {
            let cell: Cell = cells[row as usize * cols as usize + col as usize];
            let color = cell_fg(&cell);
            if cell.ch == ' ' {
                // Spaces still advance the run as spaces (keep alignment).
                if color != run_color {
                    flush(&mut job, &mut run, run_color, &font);
                    run_color = color;
                }
                run.push(' ');
                continue;
            }
            any = true;
            if color != run_color {
                flush(&mut job, &mut run, run_color, &font);
                run_color = color;
            }
            run.push(cell.ch);
        }
        flush(&mut job, &mut run, run_color, &font);

        if any {
            let galley = ui.fonts(|f| f.layout_job(job));
            painter.galley(pos2(rect.left(), y), galley, theme::TERM_FG);
        }
    }

    // Block cursor (phosphor), drawn if within the visible area.
    if cursor_col < draw_cols && cursor_row < draw_rows {
        let cx = rect.left() + f32::from(cursor_col) * char_w;
        let cy = rect.top() + f32::from(cursor_row) * line_h;
        let cursor_rect = Rect::from_min_size(pos2(cx, cy), Vec2::new(char_w, line_h));
        painter.rect(
            cursor_rect,
            egui::CornerRadius::ZERO,
            theme::TEAL.linear_multiply(0.35),
            Stroke::new(1.0, theme::TEAL),
            egui::StrokeKind::Inside,
        );
        // Re-draw the glyph under the cursor in the background colour for contrast.
        let cell = cells[cursor_row as usize * cols as usize + cursor_col as usize];
        if cell.ch != ' ' {
            painter.text(
                pos2(cx, cy),
                Align2::LEFT_TOP,
                cell.ch,
                font.clone(),
                theme::BG_SURFACE,
            );
        }
    }

    // Capture clicks so the terminal can take keyboard focus.
    ui.allocate_rect(rect, Sense::click());

    TermFit {
        cols: fit_cols,
        rows: fit_rows,
    }
}

/// Resolve a cell's effective foreground colour (handling reverse video).
fn cell_fg(cell: &Cell) -> Color32 {
    if cell.style.reverse {
        // Reverse: paint the (would-be) background colour as foreground.
        match cell.style.bg {
            crate::terminal::Color::Default => theme::BG_SURFACE,
            other => theme::term_color(other),
        }
    } else {
        theme::term_color(cell.style.fg)
    }
}

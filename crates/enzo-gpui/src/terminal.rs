//! Terminal surface chrome — context sidebar, tab strip and status bar.
//!
//! Renders real session/workspace state (no placeholder data): the live PTY
//! session, the working directory, the git branch and the ATP connection.

use gpui::{IntoElement, ParentElement, Styled, div, px};

use crate::theme;
use crate::widgets::{icon, pixel_header, text};

/// Basename of a path (for compact display), falling back to the whole string.
fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
}

/// Terminal context sidebar: the live session + workspace quick links.
pub fn sidebar(cwd: &str, branch: &str) -> impl IntoElement {
    let session = |label: String, active: bool| {
        let mut d = div().px(px(12.0)).py(px(6.0)).text_size(px(11.0));
        if active {
            d = d.bg(theme::PURPLE_BG).text_color(theme::PURPLE_FG);
        } else {
            d = d.text_color(theme::FG1);
        }
        d.child(gpui::SharedString::from(label))
    };
    let quick = |glyph: &str, label: String| {
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(5.0))
            .child(icon(glyph, 12.0, theme::MUTED))
            .child(text(&label, 11.0, theme::MUTED))
    };
    let mut col = div()
        .flex()
        .flex_col()
        .child(pixel_header("SESSIONS"))
        .child(session(format!("❯ {}", basename(cwd)), true))
        .child(div().h(px(6.0)))
        .child(pixel_header("QUICK"))
        .child(quick(theme::ICON_FOLDER, cwd.to_owned()));
    if !branch.is_empty() {
        col = col.child(quick(theme::ICON_GIT_BRANCH, branch.to_owned()));
    }
    col
}

/// Terminal tab strip (the live session + ATP connection status).
pub fn tab_bar(connected: bool) -> impl IntoElement {
    let tab = div()
        .px(px(8.0))
        .py(px(4.0))
        .bg(theme::BG_SURFACE)
        .rounded(px(3.0))
        .text_size(px(9.0))
        .font_family(theme::FONT_PIXEL)
        .text_color(theme::TEAL)
        .child("shell");
    let (atp_color, atp_label) = if connected {
        (theme::TEAL, "● ATP")
    } else {
        (theme::AMBER, "○ connecting…")
    };
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .py(px(8.0))
        .bg(theme::BG_BAR)
        .border_b_2()
        .border_color(theme::BORDER)
        .child(tab)
        .child(
            div()
                .ml_auto()
                .text_size(px(9.0))
                .font_family(theme::FONT_PIXEL)
                .text_color(atp_color)
                .child(atp_label),
        )
}

/// Terminal status bar (live connection + grid size).
pub fn status_bar(connected: bool, cols: u16, rows: u16) -> impl IntoElement {
    let cell = |s: String, c: gpui::Rgba| {
        div()
            .text_size(px(8.0))
            .font_family(theme::FONT_PIXEL)
            .text_color(c)
            .child(gpui::SharedString::from(s))
    };
    let (pty_color, pty_label) = if connected {
        (theme::TEAL, "● PTY".to_owned())
    } else {
        (theme::AMBER, "○ no daemon".to_owned())
    };
    div()
        .flex()
        .items_center()
        .gap(px(14.0))
        .px(px(14.0))
        .py(px(6.0))
        .bg(theme::BG_BAR)
        .border_t_2()
        .border_color(theme::BORDER)
        .child(cell(pty_label, pty_color))
        .child(cell("UTF-8".to_owned(), theme::FG1))
        .child(
            div()
                .ml_auto()
                .child(cell(format!("{cols}×{rows} · ⌘K"), theme::FAINT)),
        )
}

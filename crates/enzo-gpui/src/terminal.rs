//! Terminal surface — OSC-133 semantic command blocks, faithful to
//! `design/mockups/terminal.html`.

use gpui::{IntoElement, ParentElement, Styled, div, px};

use crate::theme;
use crate::widgets::{icon, pixel_header, text};

/// Terminal context sidebar: sessions + quick links.
pub fn sidebar() -> impl IntoElement {
    let session = |label: &str, active: bool| {
        let mut d = div().px(px(12.0)).py(px(6.0)).text_size(px(11.0));
        if active {
            d = d.bg(theme::PURPLE_BG).text_color(theme::PURPLE_FG);
        } else {
            d = d.text_color(theme::FG1);
        }
        d.child(gpui::SharedString::from(label.to_owned()))
    };
    let quick = |glyph: &str, label: &str| {
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(5.0))
            .child(icon(glyph, 12.0, theme::MUTED))
            .child(text(label, 11.0, theme::MUTED))
    };
    div()
        .flex()
        .flex_col()
        .child(pixel_header("SESSIONS"))
        .child(session("❯ enzo · main", true))
        .child(session("❯ api server", false))
        .child(session("❯ ssh prod ⚡", false))
        .child(div().h(px(6.0)))
        .child(pixel_header("QUICK"))
        .child(quick(theme::ICON_GIT_BRANCH, "main ✓"))
        .child(quick(theme::ICON_FOLDER, "~/github/enzo"))
}

/// Terminal tab strip (session tabs + ATP status).
pub fn tab_bar() -> impl IntoElement {
    let tab = |label: &str, active: bool| {
        let mut d = div()
            .px(px(8.0))
            .py(px(4.0))
            .text_size(px(9.0))
            .font_family(theme::FONT_PIXEL);
        if active {
            d = d
                .bg(theme::BG_SURFACE)
                .rounded(px(3.0))
                .text_color(theme::TEAL);
        } else {
            d = d.text_color(theme::FG1);
        }
        d.child(gpui::SharedString::from(label.to_owned()))
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
        .child(tab("main", true))
        .child(tab("api", false))
        .child(tab("+", false))
        .child(
            div()
                .ml_auto()
                .text_size(px(9.0))
                .font_family(theme::FONT_PIXEL)
                .text_color(theme::TEAL)
                .child("● ATP"),
        )
}

/// Terminal status bar.
pub fn status_bar() -> impl IntoElement {
    let cell = |s: &str, c: gpui::Rgba| {
        div()
            .text_size(px(8.0))
            .font_family(theme::FONT_PIXEL)
            .text_color(c)
            .child(gpui::SharedString::from(s.to_owned()))
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
        .child(cell("● PTY zsh", theme::TEAL))
        .child(cell("UTF-8", theme::FG1))
        .child(div().ml_auto().child(cell("120 FPS · ⌘K", theme::FAINT)))
}

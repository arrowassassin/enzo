//! Small shared UI helpers used across surfaces.

use gpui::{IntoElement, ParentElement, SharedString, Styled, div, px};

use crate::theme;

/// A Silkscreen pixel section header (8px, uppercase, purple) — e.g. "SESSIONS".
pub fn pixel_header(label: &str) -> impl IntoElement {
    div()
        .px(px(12.0))
        .pb(px(8.0))
        .text_size(px(8.0))
        .font_family(theme::FONT_PIXEL)
        .text_color(theme::PURPLE)
        .child(SharedString::from(label.to_owned()))
}

/// A Tabler icon glyph at `size`, in `color`.
pub fn icon(glyph: &str, size: f32, color: gpui::Rgba) -> impl IntoElement {
    div()
        .font_family(theme::FONT_ICON)
        .text_size(px(size))
        .text_color(color)
        .child(theme::icon(glyph))
}

/// A plain text span at `size`/`color` (JetBrains Mono).
pub fn text(s: &str, size: f32, color: gpui::Rgba) -> impl IntoElement {
    div()
        .text_size(px(size))
        .text_color(color)
        .child(SharedString::from(s.to_owned()))
}

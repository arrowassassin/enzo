//! Browser surface — a screenshot-driven view over the daemon's headless
//! browser (`browser.launch/navigate/screenshot`). Faithful to
//! `design/mockups/browser.html`: nav bar + page + (placeholder) devtools.

use std::sync::Arc;

use gpui::{
    Context, Entity, Image, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px,
};

use crate::EnzoApp;
use crate::text_input::TextInput;
use crate::theme;
use crate::widgets::icon;

/// Fixed daemon-side page id for the single browser tab.
pub const PAGE_ID: &str = "browser-0";

/// Headless viewport size; mouse coordinates map into this page-space.
pub const PAGE_W: u32 = 1280;
pub const PAGE_H: u32 = 800;

/// Browser surface state.
pub struct BrowserState {
    pub launched: bool,
    pub url: String,
    /// Latest decoded page screenshot.
    pub shot: Option<Arc<Image>>,
    pub loading: bool,
    /// Last launch/navigate/screenshot error, if any (shown in the page area).
    pub error: Option<String>,
}

impl BrowserState {
    pub fn new() -> Self {
        Self {
            launched: false,
            url: String::new(),
            shot: None,
            loading: false,
            error: None,
        }
    }
}

/// Nav bar: back/fwd/refresh + URL field + pick-to-AI badge.
pub fn tab_bar(
    b: &BrowserState,
    url_input: &Entity<TextInput>,
    cx: &mut Context<EnzoApp>,
) -> impl IntoElement {
    let _ = b;
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .py(px(8.0))
        .bg(theme::BG_BAR)
        .border_b_2()
        .border_color(theme::BORDER)
        .child(icon(theme::ICON_CHEVRON_RIGHT, 14.0, theme::FAINT)) // back (placeholder)
        .child(
            div()
                .id("br-refresh")
                .cursor_pointer()
                .child(icon("\u{eb13}", 14.0, theme::FG1)) // ti-refresh
                .on_click(cx.listener(|this, _, _, cx| this.refresh_browser(cx))),
        )
        .child(
            div()
                .key_context("BrowserUrl")
                .flex_1()
                .px(px(9.0))
                .py(px(4.0))
                .bg(theme::BG_SURFACE)
                .border_1()
                .border_color(theme::BORDER)
                .rounded(px(4.0))
                .text_size(px(11.0))
                .font_family(theme::FONT_MONO)
                .text_color(theme::BLUE)
                .child(url_input.clone()),
        )
        .child(
            div()
                .px(px(7.0))
                .py(px(4.0))
                .rounded(px(3.0))
                .bg(theme::BG_SURFACE)
                .text_size(px(8.0))
                .font_family(theme::FONT_PIXEL)
                .text_color(theme::PURPLE)
                .child("⊹ PICK → AI"),
        )
}

/// Status bar.
pub fn status_bar(b: &BrowserState) -> impl IntoElement {
    let cell = |s: String, c: gpui::Rgba| {
        div()
            .text_size(px(8.0))
            .font_family(theme::FONT_PIXEL)
            .text_color(c)
            .child(SharedString::from(s))
    };
    div()
        .flex()
        .items_center()
        .gap(px(14.0))
        .px(px(12.0))
        .py(px(6.0))
        .bg(theme::BG_BAR)
        .border_t_2()
        .border_color(theme::BORDER)
        .child(cell("● CDP · sandboxed".to_owned(), theme::TEAL))
        .child(cell(
            if b.url.is_empty() {
                "about:blank".to_owned()
            } else {
                b.url.clone()
            },
            theme::FG1,
        ))
        .child(div().ml_auto().child(cell("⌘K".to_owned(), theme::FAINT)))
}

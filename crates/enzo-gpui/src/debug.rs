//! Debug (DAP) surface state + UI — toolbar, call stack, variables, console.
//!
//! Backed by the daemon's `dap.*` pass-through (validated against debugpy).
//! Breakpoints are toggled at the editor cursor line (no pixel gutter), and the
//! current stop location, call stack and locals are shown in a bottom panel.

use gpui::{Context, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px};

use crate::EnzoApp;
use crate::atp::{DapScope, DapVar, StackFrame};
use crate::theme;
use crate::widgets::{icon, text};

/// Live debug-session state mirrored on the client.
pub struct DapState {
    /// DAP client id registered at the daemon.
    pub client_id: String,
    pub language: String,
    pub thread_id: Option<u64>,
    /// Current stop location `(path, line)` from the top frame.
    pub stopped_at: Option<(String, u32)>,
    pub frames: Vec<StackFrame>,
    pub scopes: Vec<DapScope>,
    pub variables: Vec<DapVar>,
    pub console: Vec<String>,
    /// True while running (not paused at a stop).
    pub running: bool,
    /// True once the program terminated/exited.
    pub ended: bool,
}

impl DapState {
    pub fn new(client_id: String, language: String) -> Self {
        Self {
            client_id,
            language,
            thread_id: None,
            stopped_at: None,
            frames: Vec::new(),
            scopes: Vec::new(),
            variables: Vec::new(),
            console: Vec::new(),
            running: true,
            ended: false,
        }
    }

    fn status(&self) -> (&'static str, gpui::Rgba) {
        if self.ended {
            ("● ended", theme::FAINT)
        } else if self.running {
            ("● running", theme::GREEN_LT)
        } else {
            ("● paused", theme::AMBER)
        }
    }
}

/// A debug toolbar button.
fn tool_btn(
    id: &'static str,
    glyph: &'static str,
    label: &'static str,
    enabled: bool,
    cx: &mut Context<EnzoApp>,
    f: impl Fn(&mut EnzoApp, &mut gpui::Window, &mut Context<EnzoApp>) + 'static,
) -> impl IntoElement {
    let color = if enabled { theme::FG1 } else { theme::FAINT };
    let mut b = div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(3.0))
        .px(px(7.0))
        .py(px(3.0))
        .rounded(px(3.0))
        .text_size(px(8.0))
        .font_family(theme::FONT_PIXEL)
        .text_color(color)
        .child(icon(glyph, 11.0, color))
        .child(label);
    if enabled {
        b = b
            .cursor_pointer()
            .bg(theme::BG_CARD)
            .on_click(cx.listener(move |this, _, window, cx| f(this, window, cx)));
    }
    b
}

/// The debug toolbar (rendered in the editor tab bar).
pub fn toolbar(dap: Option<&DapState>, has_file: bool, cx: &mut Context<EnzoApp>) -> impl IntoElement {
    let active = dap.map(|d| !d.ended).unwrap_or(false);
    let paused = dap.map(|d| !d.running && !d.ended).unwrap_or(false);
    let mut bar = div().flex().items_center().gap(px(5.0)).ml_auto();
    if active {
        bar = bar
            .child(tool_btn(
                "dbg-continue",
                theme::ICON_PLAYER_PLAY,
                "CONT",
                paused,
                cx,
                |this, window, cx| this.dbg_continue(window, cx),
            ))
            .child(tool_btn(
                "dbg-over",
                theme::ICON_CHEVRON_RIGHT,
                "OVER",
                paused,
                cx,
                |this, window, cx| this.dbg_step(crate::atp::DapStepKind::Over, window, cx),
            ))
            .child(tool_btn(
                "dbg-in",
                theme::ICON_CHEVRON_DOWN,
                "INTO",
                paused,
                cx,
                |this, window, cx| this.dbg_step(crate::atp::DapStepKind::In, window, cx),
            ))
            .child(tool_btn(
                "dbg-out",
                theme::ICON_CHEVRON_RIGHT,
                "OUT",
                paused,
                cx,
                |this, window, cx| this.dbg_step(crate::atp::DapStepKind::Out, window, cx),
            ))
            .child(tool_btn(
                "dbg-stop",
                theme::ICON_PLAYER_PLAY,
                "STOP",
                true,
                cx,
                |this, _w, cx| this.dbg_stop(cx),
            ));
    } else {
        bar = bar.child(tool_btn(
            "dbg-start",
            theme::ICON_PLAYER_PLAY,
            "DEBUG",
            has_file,
            cx,
            |this, window, cx| this.start_debug(window, cx),
        ));
    }
    bar.child(tool_btn(
        "dbg-bp",
        theme::ICON_PLAYER_PLAY,
        "◉ BP",
        has_file,
        cx,
        |this, _w, cx| this.toggle_breakpoint_at_cursor(cx),
    ))
}

fn section(title: &str) -> impl IntoElement {
    div()
        .px(px(10.0))
        .py(px(4.0))
        .text_size(px(8.0))
        .font_family(theme::FONT_PIXEL)
        .text_color(theme::PURPLE)
        .child(SharedString::from(title.to_owned()))
}

/// The bottom debug panel: status + call stack + locals + console.
pub fn panel(dap: &DapState, cx: &mut Context<EnzoApp>) -> impl IntoElement {
    let _ = cx;
    let (status, status_color) = dap.status();
    let loc = dap
        .stopped_at
        .as_ref()
        .map(|(p, l)| {
            let name = p.rsplit('/').next().unwrap_or(p);
            format!("↘ {name}:{l}")
        })
        .unwrap_or_default();
    let header = div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(10.0))
        .py(px(5.0))
        .border_b_1()
        .border_color(theme::BORDER)
        .child(text("DEBUG", 8.0, theme::TEAL))
        .child(text(&dap.language.to_uppercase(), 8.0, theme::FAINT))
        .child(text(status, 9.0, status_color))
        .child(text(&loc, 10.0, theme::FG1));

    // Call stack column.
    let mut stack = div().flex().flex_col().flex_1().child(section("CALL STACK"));
    for fr in dap.frames.iter().take(12) {
        let name = fr.path.rsplit('/').next().unwrap_or(&fr.path);
        stack = stack.child(
            div()
                .px(px(12.0))
                .py(px(1.0))
                .child(text(&format!("{} · {}:{}", fr.name, name, fr.line), 10.5, theme::FG2)),
        );
    }

    // Variables (locals) column.
    let mut vars = div().flex().flex_col().flex_1().child(section("VARIABLES"));
    if dap.variables.is_empty() {
        vars = vars.child(div().pl(px(12.0)).child(text("—", 10.0, theme::FAINT)));
    }
    for v in dap.variables.iter().take(40) {
        let ty = if v.ty.is_empty() {
            String::new()
        } else {
            format!(" : {}", v.ty)
        };
        vars = vars.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(12.0))
                .py(px(1.0))
                .child(text(&v.name, 10.5, theme::BLUE))
                .child(text(&format!("= {}{}", v.value, ty), 10.5, theme::FG2)),
        );
    }

    // Console column.
    let mut console = div().flex().flex_col().flex_1().child(section("CONSOLE"));
    for line in dap.console.iter().rev().take(12).rev() {
        console = console.child(
            div()
                .px(px(12.0))
                .py(px(1.0))
                .child(text(line.trim_end(), 10.5, theme::MUTED)),
        );
    }

    div()
        .h(px(200.0))
        .flex_none()
        .flex()
        .flex_col()
        .bg(theme::BG_SIDE)
        .border_t_2()
        .border_color(theme::BORDER)
        .child(header)
        .child(
            div()
                .flex()
                .flex_1()
                .overflow_hidden()
                .child(stack)
                .child(div().w(px(1.0)).bg(theme::BORDER))
                .child(vars)
                .child(div().w(px(1.0)).bg(theme::BORDER))
                .child(console),
        )
}

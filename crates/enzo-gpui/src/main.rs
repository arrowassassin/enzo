//! Enzo GPUI client.
//!
//! Boots a GPUI window rendering the workspace shell (icon dock → context
//! sidebar → surface) faithful to `design/mockups/*.html`, talking to the same
//! `enzo-daemon` over ATP as the legacy client. This module owns the root view
//! and the shared chrome; each surface lives in its own module.

use std::time::Duration;

use gpui::{
    AnyElement, App, Bounds, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render,
    SharedString, Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, size,
};
use gpui_platform::application;

mod atp;
mod database;
mod ide;
mod terminal;
mod terminal_state;
mod text_input;
mod theme;
mod widgets;

use std::path::Path;

use atp::{Atp, Command, Incoming};
use database::DbState;
use ide::IdeState;
use text_input::TextInput;
use widgets::text;

actions!(enzo, [RunQuery, CommitEdit, CancelEdit, SaveFile]);

/// Which top-level surface is displayed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Terminal,
    Editor,
    Browser,
    Database,
}

/// Root workspace view.
pub struct EnzoApp {
    surface: Surface,
    atp: Atp,
    connected: bool,
    db: DbState,
    sql_input: Entity<TextInput>,
    dialog_open: bool,
    dialog_name: Entity<TextInput>,
    dialog_path: Entity<TextInput>,
    next_conn: u32,
    cell_input: Entity<TextInput>,
    term: terminal_state::Terminal,
    term_id: String,
    term_focus: FocusHandle,
    ide: IdeState,
}

/// Terminal grid size (resize-to-fit comes later).
const TERM_COLS: u16 = 120;
const TERM_ROWS: u16 = 32;

impl EnzoApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let atp = atp::connect();
        let sql_input = cx
            .new(|cx| TextInput::new(cx, "type SQL — ⌘↵ to run", "SELECT name, email FROM users"));
        let dialog_name = cx.new(|cx| TextInput::new(cx, "my database", ""));
        let dialog_path = cx.new(|cx| TextInput::new(cx, "/path/to/db.sqlite", ""));
        let cell_input = cx.new(|cx| TextInput::new(cx, "", ""));

        // Spawn the first PTY session (buffered until the daemon connects).
        let term_id = "term-0".to_owned();
        let _ = atp.commands.send(Command::NewSession {
            id: term_id.clone(),
            cols: TERM_COLS,
            rows: TERM_ROWS,
        });

        // Open the first-run demo database (a real, seeded on-disk SQLite file).
        let demo_name = "SQLite · demo.db".to_owned();
        if let Some(path) = default_db_path() {
            let _ = atp.commands.send(Command::DbConnect {
                conn: "db-0".into(),
                path,
                seed: true,
            });
        }

        // Drain daemon events ~30ms while the entity is alive; only repaint when
        // something actually arrived (no idle wakeups).
        cx.spawn(async move |this, cx| {
            loop {
                let alive = this
                    .update(cx, |this, cx| {
                        if this.drain() {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
            }
        })
        .detach();

        Self {
            surface: Surface::Terminal,
            atp,
            connected: false,
            db: DbState::new("db-0", &demo_name),
            sql_input,
            dialog_open: false,
            dialog_name,
            dialog_path,
            next_conn: 1,
            cell_input,
            term: terminal_state::Terminal::new(TERM_COLS, TERM_ROWS),
            term_id,
            term_focus: cx.focus_handle(),
            ide: IdeState::new(),
        }
    }

    // ── IDE ───────────────────────────────────────────────────────────────
    /// Open a file: read it, then build a gpui-component code editor for it
    /// (ropey buffer + tree-sitter highlight + LSP, language per extension).
    fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.ide.open_file(path);
        if self.ide.error.is_none() {
            let language = self.ide.language.clone();
            let content = self.ide.content.clone();
            let editor = cx.new(|cx| {
                gpui_component::input::InputState::new(window, cx)
                    .code_editor(language)
                    .line_number(true)
                    .indent_guides(true)
                    .default_value(content)
            });
            self.ide.editor = Some(editor);
        } else {
            self.ide.editor = None;
        }
        cx.notify();
    }

    // ── Cell editing (PK-anchored) ────────────────────────────────────────
    /// Begin editing cell `(row, col)`: seed the editor and focus it.
    fn start_edit(&mut self, row: usize, col: usize, window: &mut Window, cx: &mut Context<Self>) {
        let val = self
            .db
            .rows
            .get(row)
            .and_then(|r| r.get(col))
            .cloned()
            .unwrap_or_default();
        self.db.editing = Some((row, col));
        self.cell_input.update(cx, |i, cx| i.set_text(&val, cx));
        let handle = self.cell_input.read(cx).handle();
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Commit the in-progress edit via a PK-anchored `db.table.update`.
    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        let Some((row, col)) = self.db.editing else {
            return;
        };
        self.db.editing = None;
        let (Some(conn), Some(table), Some(column)) = (
            self.db.active_conn_id().map(str::to_owned),
            self.db.browsing.clone(),
            self.db.columns.get(col).cloned(),
        ) else {
            cx.notify();
            return;
        };
        let value = self.cell_input.read(cx).text().to_owned();
        // Skip no-op edits: this both avoids needless writes and prevents a
        // committed-unchanged NULL cell (shown as "") from being rewritten as ''.
        let old = self
            .db
            .rows
            .get(row)
            .and_then(|r| r.get(col))
            .cloned()
            .unwrap_or_default();
        if value == old {
            cx.notify();
            return;
        }
        // Build the primary-key predicate from this row's PK column values.
        let mut pk = Vec::new();
        for name in &self.db.pk_columns {
            if let Some(idx) = self.db.columns.iter().position(|c| c == name)
                && let Some(v) = self.db.rows.get(row).and_then(|r| r.get(idx))
            {
                pk.push((name.clone(), v.clone()));
            }
        }
        if pk.is_empty() {
            cx.notify();
            return;
        }
        let _ = self.atp.commands.send(Command::DbUpdate {
            conn,
            table,
            cells: vec![(column, value.clone())],
            pk,
        });
        // Optimistic local update; a DbError will surface if the write fails.
        if let Some(cell) = self.db.rows.get_mut(row).and_then(|r| r.get_mut(col)) {
            *cell = value;
        }
        cx.notify();
    }

    fn on_commit_edit(&mut self, _: &CommitEdit, _: &mut Window, cx: &mut Context<Self>) {
        if self.db.editing.is_some() {
            self.commit_edit(cx);
        }
    }

    fn on_cancel_edit(&mut self, _: &CancelEdit, _: &mut Window, cx: &mut Context<Self>) {
        if self.db.editing.take().is_some() {
            cx.notify();
        }
    }

    /// `⌘S` — write the editor's buffer back to the open file.
    fn on_save_file(&mut self, _: &SaveFile, _: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(editor)) = (self.ide.open_path.clone(), self.ide.editor.clone())
        else {
            return;
        };
        let content = editor.read(cx).value().to_string();
        match std::fs::write(&path, content) {
            Ok(()) => self.ide.error = None,
            Err(e) => self.ide.error = Some(format!("save failed: {e}")),
        }
        cx.notify();
    }

    // ── Terminal ──────────────────────────────────────────────────────────
    /// Translate a keystroke to PTY bytes and forward it to the session.
    fn on_term_key(&mut self, ev: &KeyDownEvent, _: &mut Window, _cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = &ks.modifiers;
        let mut bytes: Vec<u8> = Vec::new();
        // Control combos first (so Ctrl-C → 0x03 regardless of key_char).
        if m.control {
            bytes = match ks.key.as_str() {
                "space" => vec![0],
                "[" => vec![0x1b],
                "\\" => vec![0x1c],
                "]" => vec![0x1d],
                k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphabetic() => {
                    vec![k.as_bytes()[0].to_ascii_lowercase() - b'a' + 1]
                }
                _ => Vec::new(),
            };
        }
        if bytes.is_empty() {
            if let Some(ch) = &ks.key_char {
                bytes = ch.clone().into_bytes();
                // Alt/Meta → ESC prefix (readline word motion, etc.).
                if m.alt {
                    let mut prefixed = vec![0x1b];
                    prefixed.append(&mut bytes);
                    bytes = prefixed;
                }
            } else {
                bytes = match ks.key.as_str() {
                    "enter" => vec![b'\r'],
                    "backspace" => vec![0x7f],
                    "tab" => vec![b'\t'],
                    "escape" => vec![0x1b],
                    "up" => vec![0x1b, b'[', b'A'],
                    "down" => vec![0x1b, b'[', b'B'],
                    "right" => vec![0x1b, b'[', b'C'],
                    "left" => vec![0x1b, b'[', b'D'],
                    "home" => vec![0x1b, b'[', b'H'],
                    "end" => vec![0x1b, b'[', b'F'],
                    "delete" => vec![0x1b, b'[', b'3', b'~'],
                    _ => return,
                };
            }
        }
        if bytes.is_empty() {
            return;
        }
        let _ = self.atp.commands.send(Command::Input {
            id: self.term_id.clone(),
            data: bytes,
        });
    }

    /// Terminal surface: OSC-133 command blocks when the shell emits marks,
    /// else the raw VT grid. Focusable; keystrokes go to the PTY.
    fn terminal_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Blocks once OSC-133 marks exist, except while a full-screen TUI is up
        // (vim/less use the alternate screen → render the raw grid).
        let body: gpui::AnyElement = if self.term.alt_screen() || self.term.blocks().is_empty() {
            self.terminal_grid().into_any_element()
        } else {
            terminal_blocks(self.term.blocks()).into_any_element()
        };
        div()
            .id("terminal")
            .track_focus(&self.term_focus)
            .key_context("Terminal")
            .on_key_down(cx.listener(Self::on_term_key))
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .child(body)
    }

    /// Raw VT grid render (per-cell ANSI colour via run-coalescing).
    fn terminal_grid(&self) -> impl IntoElement {
        use terminal_state::Cell;
        let cols = self.term.cols() as usize;
        let rows = self.term.rows() as usize;
        let cells = self.term.cells();
        let mut col = div()
            .flex()
            .flex_col()
            .size_full()
            .px(px(12.0))
            .py(px(10.0))
            .font_family(theme::FONT_MONO)
            .text_size(px(12.5))
            .line_height(px(17.0))
            .text_color(theme::FG2);
        // Style key used to coalesce a run of cells into one coloured span.
        let key = |c: &Cell| (c.style.fg, c.style.bg, c.style.bold, c.style.reverse);
        for r in 0..rows {
            let row = &cells[r * cols..r * cols + cols];
            // Trim trailing blank cells (keep coloured/reverse padding).
            let last = row
                .iter()
                .rposition(|c| {
                    c.ch != ' ' || c.style.reverse || c.style.bg != terminal_state::Color::Default
                })
                .map_or(0, |i| i + 1);
            if last == 0 {
                col = col.child(div().child(SharedString::from(" ")));
                continue;
            }
            let mut line = div().flex();
            let mut buf = String::new();
            let mut cur = key(&row[0]);
            for cell in &row[..last] {
                let k = key(cell);
                if k != cur && !buf.is_empty() {
                    line = line.child(run_span(cur, std::mem::take(&mut buf)));
                }
                cur = k;
                buf.push(cell.ch);
            }
            if !buf.is_empty() {
                line = line.child(run_span(cur, buf));
            }
            col = col.child(line);
        }
        col
    }

    // ── Connection dialog ─────────────────────────────────────────────────
    fn open_connection_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog_open = true;
        self.db.editing = None;
        let handle = self.dialog_path.read(cx).handle();
        window.focus(&handle, cx);
        cx.notify();
    }

    fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog_open = false;
        cx.notify();
    }

    /// Read the dialog fields, open a real connection by path, and close.
    fn save_connection(&mut self, cx: &mut Context<Self>) {
        let path = self.dialog_path.read(cx).text().trim().to_owned();
        if path.is_empty() {
            return;
        }
        let name = self.dialog_name.read(cx).text().trim().to_owned();
        let id = format!("db-{}", self.next_conn);
        self.next_conn += 1;
        let display = if name.is_empty() {
            format!("conn · {id}")
        } else {
            name
        };
        self.db.add_connection(id.clone(), display);
        let _ = self.atp.commands.send(Command::DbConnect {
            conn: id,
            path,
            seed: false,
        });
        self.dialog_open = false;
        self.dialog_name.update(cx, |i, cx| i.set_text("", cx));
        self.dialog_path.update(cx, |i, cx| i.set_text("", cx));
        cx.notify();
    }

    /// Drain queued daemon events into entity state. Returns `true` if any event
    /// was consumed (so the caller can repaint only when needed).
    fn drain(&mut self) -> bool {
        let mut any = false;
        while let Ok(msg) = self.atp.incoming.try_recv() {
            any = true;
            match msg {
                Incoming::Connected => self.connected = true,
                Incoming::Closed => {
                    self.connected = false;
                    self.db.running = false;
                }
                Incoming::DbConnected { conn, driver } => self.db.set_driver(&conn, driver),
                Incoming::DbTables { conn, tables } => self.db.set_tables(&conn, tables),
                Incoming::DbResult {
                    columns,
                    rows,
                    ms,
                    total,
                    page,
                    browsing,
                    pk_columns,
                } => self
                    .db
                    .apply_result(columns, rows, ms, total, page, browsing, pk_columns),
                Incoming::DbError { message } => self.db.apply_error(message),
                Incoming::Output { session_id, data } => {
                    if session_id == self.term_id {
                        self.term.process(&data);
                    }
                }
                // Highlighting is now done locally by gpui-component's editor.
                Incoming::Highlight { .. } => {}
                // agent prompt/block surfaces wired in a later segment
                Incoming::PromptShow { .. }
                | Incoming::BlockPush { .. }
                | Incoming::BlockClear { .. } => {}
            }
        }
        any
    }

    // ── Database actions ──────────────────────────────────────────────────
    /// Browse `table` in the active connection (sets SQL + pages via ATP).
    fn browse_table(&mut self, table: String, page: u64, cx: &mut Context<Self>) {
        let Some(conn) = self.db.active_conn_id().map(str::to_owned) else {
            return;
        };
        let q = format!("SELECT * FROM {table} LIMIT {};", database::PAGE_SIZE);
        self.sql_input.update(cx, |i, cx| i.set_text(&q, cx));
        self.db.running = true;
        let _ = self.atp.commands.send(Command::DbBrowse {
            conn,
            table,
            page,
            size: database::PAGE_SIZE,
        });
        cx.notify();
    }

    /// Run the SQL editor's contents against the active connection.
    fn run_query(&mut self, cx: &mut Context<Self>) {
        let Some(conn) = self.db.active_conn_id().map(str::to_owned) else {
            return;
        };
        let sql = self.sql_input.read(cx).text().trim().to_owned();
        if sql.is_empty() {
            return;
        }
        self.db.running = true;
        self.db.browsing = None;
        let _ = self.atp.commands.send(Command::DbQuery { conn, sql });
        cx.notify();
    }

    /// Page the currently-browsed table by `delta` (clamped), re-browsing.
    fn page_relative(&mut self, delta: i64, cx: &mut Context<Self>) {
        let Some(table) = self.db.browsing.clone() else {
            return;
        };
        let pages = self
            .db
            .total
            .map_or(1, |t| t.div_ceil(database::PAGE_SIZE).max(1));
        let last = pages.saturating_sub(1);
        let next = i64::try_from(self.db.page).unwrap_or(0) + delta;
        let next = next.clamp(0, i64::try_from(last).unwrap_or(0));
        let next = u64::try_from(next).unwrap_or(0);
        if next != self.db.page {
            self.browse_table(table, next, cx);
        }
    }

    /// `⌘↵` handler — runs the query when the Database surface is active.
    fn on_run_query(&mut self, _: &RunQuery, _: &mut Window, cx: &mut Context<Self>) {
        if self.surface == Surface::Database {
            self.run_query(cx);
        }
    }
}

impl Render for EnzoApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog = self.dialog_open.then(|| {
            database::connection_dialog(&self.dialog_name, &self.dialog_path, cx).into_any_element()
        });
        let key_context = match self.surface {
            Surface::Database => "Database",
            Surface::Terminal => "Terminal",
            Surface::Editor => "Editor",
            Surface::Browser => "Browser",
        };
        div()
            .relative()
            .flex()
            .size_full()
            .key_context(key_context)
            .bg(theme::BG_SURFACE)
            .font_family(theme::FONT_MONO)
            .text_color(theme::FG0)
            .on_action(cx.listener(Self::on_run_query))
            .on_action(cx.listener(Self::on_commit_edit))
            .on_action(cx.listener(Self::on_cancel_edit))
            .on_action(cx.listener(Self::on_save_file))
            .child(self.dock(cx))
            .child(self.sidebar(cx))
            .child(self.surface_column(cx))
            .children(dialog)
    }
}

// ── Shell chrome ────────────────────────────────────────────────────────────

impl EnzoApp {
    /// Left icon dock (46px); surface icons on top, robot/settings pinned bottom.
    fn dock(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .justify_between()
            .w(px(46.0))
            .h_full()
            .py(px(12.0))
            .bg(theme::BG_DOCK)
            .border_r_2()
            .border_color(theme::BORDER)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(16.0))
                    .child(self.dock_item(cx, "dock-term", theme::ICON_TERMINAL, Surface::Terminal))
                    .child(self.dock_item(cx, "dock-code", theme::ICON_CODE, Surface::Editor))
                    .child(self.dock_item(cx, "dock-web", theme::ICON_WORLD, Surface::Browser))
                    .child(self.dock_item(cx, "dock-db", theme::ICON_DATABASE, Surface::Database)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(16.0))
                    .child(dock_glyph(theme::ICON_ROBOT))
                    .child(dock_glyph(theme::ICON_SETTINGS)),
            )
    }

    /// One clickable dock icon that switches the active surface.
    fn dock_item(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        glyph: &str,
        target: Surface,
    ) -> impl IntoElement {
        let active = self.surface == target;
        div()
            .id(id)
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .size(px(30.0))
            .rounded(px(5.0))
            .when(active, |d| d.bg(theme::BG_BAR))
            .font_family(theme::FONT_ICON)
            .text_size(px(18.0))
            .text_color(if active { theme::TEAL } else { theme::FAINT })
            .child(theme::icon(glyph))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.surface = target;
                this.db.editing = None; // don't strand an in-progress cell edit
                // Focus the surface's primary input on entry.
                match target {
                    Surface::Database => {
                        let handle = this.sql_input.read(cx).handle();
                        window.focus(&handle, cx);
                    }
                    Surface::Terminal => window.focus(&this.term_focus, cx),
                    _ => {}
                }
                cx.notify();
            }))
    }

    /// Per-surface context sidebar.
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let inner: AnyElement = match self.surface {
            Surface::Terminal => terminal::sidebar().into_any_element(),
            Surface::Database => database::sidebar(&self.db, cx).into_any_element(),
            Surface::Editor => ide::sidebar(&self.ide, cx).into_any_element(),
            Surface::Browser => widgets::pixel_header("DEVTOOLS").into_any_element(),
        };
        div()
            .flex()
            .flex_none()
            .flex_col()
            .w(px(150.0))
            .h_full()
            .py(px(10.0))
            .bg(theme::BG_SIDE)
            .border_r_2()
            .border_color(theme::BORDER)
            .child(inner)
    }

    /// Surface column: tab bar → content → status bar (dispatched per surface).
    fn surface_column(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (tab_bar, content, status_bar): (AnyElement, AnyElement, AnyElement) =
            match self.surface {
                Surface::Terminal => (
                    terminal::tab_bar().into_any_element(),
                    self.terminal_view(cx).into_any_element(),
                    terminal::status_bar().into_any_element(),
                ),
                Surface::Database => (
                    database::tab_bar(&self.db, cx).into_any_element(),
                    database::content(&self.db, &self.sql_input, &self.cell_input, cx)
                        .into_any_element(),
                    database::status_bar(&self.db, cx).into_any_element(),
                ),
                Surface::Editor => (
                    ide::tab_bar(&self.ide).into_any_element(),
                    ide::content(&self.ide).into_any_element(),
                    ide::status_bar(&self.ide).into_any_element(),
                ),
                Surface::Browser => (
                    placeholder_bar("BROWSER"),
                    placeholder_body("◍ enter a URL to start the headless browser"),
                    placeholder_bar("about:blank"),
                ),
            };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(tab_bar)
            .child(div().flex_1().overflow_hidden().child(content))
            .child(status_bar)
    }
}

/// Render OSC-133 command blocks as cards (`design/mockups/terminal.html`).
fn terminal_blocks(blocks: &[terminal_state::Block]) -> impl IntoElement {
    let mut col = div()
        .flex()
        .flex_col()
        .size_full()
        .px(px(14.0))
        .py(px(12.0))
        .font_family(theme::FONT_MONO)
        .text_size(px(12.5))
        .line_height(px(18.0));
    for b in blocks {
        col = col.child(block_card(b));
    }
    col
}

/// One command block: coloured left rail by exit status, command line with an
/// exit badge (or live cursor while running), then output lines.
fn block_card(b: &terminal_state::Block) -> impl IntoElement {
    let (rail, badge_color, badge) = match (b.running, b.exit) {
        (true, _) => (theme::PURPLE_BG, theme::PURPLE_LT, String::new()),
        (false, Some(0)) => (theme::GREEN, theme::GREEN, "✓ EXIT 0".to_owned()),
        (false, Some(code)) => (theme::RED, theme::RED_LT, format!("✗ EXIT {code}")),
        (false, None) => (theme::BORDER, theme::FAINT, String::new()),
    };
    let mut cmd_row = div()
        .flex()
        .items_center()
        .child(div().text_color(theme::TEAL).child("❯ "))
        .child(
            div()
                .text_color(theme::FG0)
                .child(SharedString::from(b.command.clone())),
        );
    if b.running {
        cmd_row = cmd_row.child(div().ml(px(2.0)).w(px(7.0)).h(px(13.0)).bg(theme::TEAL));
    } else if !badge.is_empty() {
        cmd_row = cmd_row.child(
            div()
                .ml_auto()
                .text_size(px(8.0))
                .font_family(theme::FONT_PIXEL)
                .text_color(badge_color)
                .child(SharedString::from(badge)),
        );
    }
    let mut card = div()
        .border_l_3()
        .border_color(rail)
        .pl(px(10.0))
        .mb(px(12.0))
        .child(cmd_row);
    if !b.output.trim_end().is_empty() {
        let mut out = div().flex().flex_col().text_color(theme::MUTED);
        for line in b.output.trim_end().lines() {
            out = out.child(div().child(SharedString::from(line.to_owned())));
        }
        card = card.child(out);
    }
    card
}

/// A coloured run of terminal text. `key` is `(fg, bg, bold, reverse)`.
fn run_span(
    key: (terminal_state::Color, terminal_state::Color, bool, bool),
    text: String,
) -> impl IntoElement {
    use terminal_state::Color;
    let (fg, bg, bold, reverse) = key;
    let mut fgc = term_color(fg, bold);
    let mut bgc = match bg {
        Color::Default => None,
        c => Some(term_color(c, false)),
    };
    if reverse {
        let swap = Some(fgc);
        fgc = bgc.unwrap_or(theme::BG_SURFACE);
        bgc = swap;
    }
    let mut d = div().text_color(fgc).child(SharedString::from(text));
    if let Some(b) = bgc {
        d = d.bg(b);
    }
    d
}

/// Map a terminal cell colour to the Enzo-tuned xterm palette.
fn term_color(c: terminal_state::Color, bold: bool) -> gpui::Rgba {
    use terminal_state::Color;
    match c {
        Color::Default => theme::FG2,
        Color::Rgb(r, g, b) => {
            theme::rgb_hex((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
        }
        Color::Indexed(i) => indexed_color(i, bold),
    }
}

/// xterm-256 palette: 16 Enzo-tuned base colours, the 6×6×6 cube, and greys.
fn indexed_color(i: u8, bold: bool) -> gpui::Rgba {
    const P16: [gpui::Rgba; 16] = [
        theme::rgb_hex(0x16131f), // black
        theme::rgb_hex(0xe24b4a), // red
        theme::rgb_hex(0x97c459), // green
        theme::rgb_hex(0xef9f27), // yellow
        theme::rgb_hex(0x85b7eb), // blue
        theme::rgb_hex(0xafa9ec), // magenta
        theme::rgb_hex(0x5dcaa5), // cyan
        theme::rgb_hex(0xc9c4dc), // white
        theme::rgb_hex(0x5f5e6e), // bright black
        theme::rgb_hex(0xf09595), // bright red
        theme::rgb_hex(0xb6e27a), // bright green
        theme::rgb_hex(0xffc35a), // bright yellow
        theme::rgb_hex(0xa9cdf5), // bright blue
        theme::rgb_hex(0xc7c2f7), // bright magenta
        theme::rgb_hex(0x8fe0c4), // bright cyan
        theme::rgb_hex(0xe8e4f5), // bright white
    ];
    let idx = if bold && i < 8 { i + 8 } else { i };
    match idx {
        0..=15 => P16[idx as usize],
        16..=231 => {
            let n = idx - 16;
            let level = |v: u8| -> u32 { if v == 0 { 0 } else { u32::from(55 + v * 40) } };
            theme::rgb_hex((level(n / 36) << 16) | (level((n / 6) % 6) << 8) | level(n % 6))
        }
        _ => {
            let v = u32::from(8 + (idx - 232) * 10);
            theme::rgb_hex((v << 16) | (v << 8) | v)
        }
    }
}

/// A non-interactive dock glyph (robot / settings).
fn dock_glyph(glyph: &str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(30.0))
        .font_family(theme::FONT_ICON)
        .text_size(px(18.0))
        .text_color(theme::FAINT)
        .child(theme::icon(glyph))
}

/// A placeholder bar (used by not-yet-built surfaces).
fn placeholder_bar(label: &str) -> AnyElement {
    div()
        .flex()
        .items_center()
        .px(px(12.0))
        .py(px(8.0))
        .bg(theme::BG_BAR)
        .border_b_2()
        .border_color(theme::BORDER)
        .child(
            div()
                .text_size(px(8.0))
                .font_family(theme::FONT_PIXEL)
                .text_color(theme::FG1)
                .child(gpui::SharedString::from(label.to_owned())),
        )
        .into_any_element()
}

/// A centered placeholder body (used by not-yet-built surfaces).
fn placeholder_body(label: &str) -> AnyElement {
    div()
        .flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(text(label, 14.0, theme::FAINT))
        .into_any_element()
}

/// Map gpui-component's theme tokens onto the Enzo palette so the embedded code
/// editor matches the rest of the app.
fn apply_enzo_theme(cx: &mut App) {
    use gpui_component::{Theme, ThemeMode};
    Theme::change(ThemeMode::Dark, None, cx);
    let c = &mut Theme::global_mut(cx).colors;
    c.background = theme::BG_SURFACE.into();
    c.foreground = theme::FG0.into();
    c.border = theme::BORDER.into();
    c.muted = theme::BG_CARD.into();
    c.muted_foreground = theme::FAINT.into();
    c.accent = theme::BG_BAR.into();
    c.accent_foreground = theme::FG0.into();
    c.primary = theme::TEAL.into();
    c.primary_foreground = theme::GREEN_INK.into();
    c.secondary = theme::BG_BAR.into();
    c.secondary_foreground = theme::FG0.into();
    c.input = theme::BG_CARD.into();
    c.ring = theme::TEAL.into();
    c.selection = theme::PURPLE_BG.into();
    c.popover = theme::BG_BAR.into();
    c.popover_foreground = theme::FG0.into();
    c.scrollbar_thumb = theme::BORDER.into();
    c.sidebar = theme::BG_SIDE.into();
    c.sidebar_foreground = theme::FG1.into();
    c.sidebar_border = theme::BORDER.into();
}

fn main() {
    application()
        .with_assets(gpui_component_assets::Assets)
        .run(|cx: &mut App| {
            theme::install_fonts(cx);
            gpui_component::init(cx);
            apply_enzo_theme(cx);
            text_input::bind_keys(cx);
            // Scope these to the Database surface so the Terminal surface receives
            // Enter/Escape/etc. as raw PTY input instead of having the action layer
            // swallow them before on_key_down.
            let db = Some("Database");
            cx.bind_keys([
                gpui::KeyBinding::new("cmd-enter", RunQuery, db),
                gpui::KeyBinding::new("ctrl-enter", RunQuery, db),
                gpui::KeyBinding::new("enter", CommitEdit, db),
                gpui::KeyBinding::new("escape", CancelEdit, db),
                gpui::KeyBinding::new("cmd-s", SaveFile, Some("Editor")),
                gpui::KeyBinding::new("ctrl-s", SaveFile, Some("Editor")),
            ]);
            let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    let app = cx.new(EnzoApp::new);
                    // The app boots on the Terminal surface — focus it for typing.
                    let handle = app.read(cx).term_focus.clone();
                    window.focus(&handle, cx);
                    // gpui-component requires the window's root layer to be a `Root`.
                    cx.new(|cx| gpui_component::Root::new(gpui::AnyView::from(app), window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}

/// Path to the first-run demo database (`~/.enzo/demo.db`), creating `~/.enzo`.
fn default_db_path() -> Option<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let dir = std::path::Path::new(&home).join(".enzo");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("demo.db").to_string_lossy().into_owned())
}

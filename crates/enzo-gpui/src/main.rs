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
mod browser;
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

actions!(
    enzo,
    [RunQuery, CommitEdit, CancelEdit, SaveFile, Navigate]
);

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
    /// Open query buffers (Harlequin-style multi-tab SQL editing).
    db_tabs: Vec<database::QueryTab>,
    active_tab: usize,
    next_tab: u32,
    /// Executed-statement history (newest first), shown in the DB sidebar.
    db_history: Vec<database::HistEntry>,
    /// SQL awaiting its result, so history can record timing/row counts.
    pending_sql: Option<String>,
    dialog_open: bool,
    dialog_name: Entity<TextInput>,
    dialog_path: Entity<TextInput>,
    /// Driver selected in the connection dialog (`"sqlite"` | `"duckdb"`).
    dialog_driver: String,
    next_conn: u32,
    cell_input: Entity<TextInput>,
    term: terminal_state::Terminal,
    term_id: String,
    term_focus: FocusHandle,
    /// Block-cursor blink phase (toggled on a timer while the terminal is active).
    cursor_on: bool,
    ide: IdeState,
    /// Whether the Editor surface has been opened yet (drives first-entry file open).
    ide_opened: bool,
    /// Active AI-CLI approval prompt (id, title, body, actions), if any.
    agent_prompt: Option<AgentPrompt>,
    /// AI agent blocks composited in the terminal column (id → title, body).
    agent_blocks: Vec<AgentBlock>,
    browser: browser::BrowserState,
    url_input: Entity<TextInput>,
    git_commit_input: Entity<TextInput>,
}

/// An AI-CLI approval prompt awaiting a decision (`prompt.show`).
struct AgentPrompt {
    id: String,
    title: String,
    body: String,
    actions: Vec<String>,
}

/// An AI agent block pushed into the terminal column (`block.push`).
struct AgentBlock {
    id: String,
    title: String,
    body: String,
}

/// Terminal grid size (resize-to-fit comes later).
const TERM_COLS: u16 = 120;
const TERM_ROWS: u16 = 32;

impl EnzoApp {
    /// Build a SQL code-editor entity (multi-line, tree-sitter highlight, LSP-ready).
    fn new_sql_editor(seed: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<gpui_component::input::InputState> {
        let seed = seed.to_owned();
        cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx)
                .code_editor("sql")
                .line_number(true)
                .placeholder("SELECT …   —   ⌘↵ to run")
                .default_value(seed)
        })
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let atp = atp::connect();
        let first_tab = database::QueryTab {
            id: 1,
            title: "Query 1".to_owned(),
            editor: Self::new_sql_editor("SELECT * FROM users;", window, cx),
        };
        let dialog_name = cx.new(|cx| TextInput::new(cx, "my database", ""));
        let dialog_path = cx.new(|cx| TextInput::new(cx, "/path/to/db.sqlite", ""));
        let cell_input = cx.new(|cx| TextInput::new(cx, "", ""));
        let url_input = cx.new(|cx| TextInput::new(cx, "https://example.com", ""));
        let git_commit_input = cx.new(|cx| TextInput::new(cx, "commit message…", ""));

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
                driver: "sqlite".into(),
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

        // Block-cursor blink: flip ~every 530ms, repaint only while the terminal
        // surface is showing (so an idle Database/IDE view isn't woken).
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(530))
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        this.cursor_on = !this.cursor_on;
                        if this.surface == Surface::Terminal {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();

        // Fetch git status up front so the Terminal/IDE sidebars show the real
        // branch immediately (buffered until the daemon connects).
        let _ = atp.commands.send(Command::GitStatus {
            root: std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_owned()),
        });

        Self {
            surface: Surface::Terminal,
            atp,
            connected: false,
            db: DbState::new("db-0", &demo_name),
            db_tabs: vec![first_tab],
            active_tab: 0,
            next_tab: 1,
            db_history: Vec::new(),
            pending_sql: None,
            dialog_open: false,
            dialog_name,
            dialog_path,
            dialog_driver: "sqlite".to_owned(),
            next_conn: 1,
            cell_input,
            term: terminal_state::Terminal::new(TERM_COLS, TERM_ROWS),
            term_id,
            term_focus: cx.focus_handle(),
            cursor_on: true,
            ide: IdeState::new(),
            ide_opened: false,
            agent_prompt: None,
            agent_blocks: Vec::new(),
            browser: browser::BrowserState::new(),
            url_input,
            git_commit_input,
        }
    }

    // ── Query tabs (Harlequin multi-buffer) ───────────────────────────────
    /// The active query buffer's editor.
    fn active_editor(&self) -> &Entity<gpui_component::input::InputState> {
        &self.db_tabs[self.active_tab.min(self.db_tabs.len() - 1)].editor
    }

    /// Open a new empty query buffer and focus it.
    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.next_tab += 1;
        let id = self.next_tab;
        let editor = Self::new_sql_editor("", window, cx);
        editor.update(cx, |e, cx| e.focus(window, cx));
        self.db_tabs.push(database::QueryTab {
            id,
            title: format!("Query {id}"),
            editor,
        });
        self.active_tab = self.db_tabs.len() - 1;
        cx.notify();
    }

    /// Switch to buffer `idx` and focus its editor.
    fn switch_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if idx < self.db_tabs.len() {
            self.active_tab = idx;
            let editor = self.db_tabs[idx].editor.clone();
            editor.update(cx, |e, cx| e.focus(window, cx));
            cx.notify();
        }
    }

    /// Close buffer `idx` (always keeps at least one buffer open).
    fn close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.db_tabs.len() <= 1 || idx >= self.db_tabs.len() {
            return;
        }
        self.db_tabs.remove(idx);
        if self.active_tab >= self.db_tabs.len() {
            self.active_tab = self.db_tabs.len() - 1;
        }
        let editor = self.active_editor().clone();
        editor.update(cx, |e, cx| e.focus(window, cx));
        cx.notify();
    }

    // ── IDE entry ─────────────────────────────────────────────────────────
    /// Switch to the Editor surface: refresh git, and on first entry open a
    /// default file so the editor demonstrably renders rather than sitting blank.
    fn enter_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_refresh(cx);
        if !self.ide_opened {
            self.ide_opened = true;
            if self.ide.open_path.is_none()
                && let Some(path) = self.ide.default_file()
            {
                self.open_file(&path, window, cx);
            }
        }
    }

    // ── Git source control ────────────────────────────────────────────────
    fn git_refresh(&mut self, cx: &mut Context<Self>) {
        let _ = self.atp.commands.send(Command::GitStatus {
            root: self.ide.root(),
        });
        cx.notify();
    }

    fn git_stage(&mut self, file: String, unstage: bool, cx: &mut Context<Self>) {
        let _ = self.atp.commands.send(Command::GitStage {
            root: self.ide.root(),
            file,
            unstage,
        });
        cx.notify();
    }

    fn do_git_commit(&mut self, cx: &mut Context<Self>) {
        let message = self.git_commit_input.read(cx).text().trim().to_owned();
        if message.is_empty() {
            return;
        }
        let _ = self.atp.commands.send(Command::GitCommit {
            root: self.ide.root(),
            message,
        });
        self.git_commit_input.update(cx, |i, cx| i.set_text("", cx));
        cx.notify();
    }

    // ── Browser ───────────────────────────────────────────────────────────
    /// Navigate the headless browser to the URL bar's contents + grab a shot.
    fn browse(&mut self, cx: &mut Context<Self>) {
        let mut url = self.url_input.read(cx).text().trim().to_owned();
        if url.is_empty() {
            return;
        }
        if !url.contains("://") {
            url = format!("https://{url}");
        }
        self.browser.url = url.clone();
        self.browser.loading = true;
        self.browser.error = None;
        if !self.browser.launched {
            let _ = self.atp.commands.send(Command::BrowserLaunch {
                id: browser::PAGE_ID.into(),
                width: 1024,
                height: 720,
            });
            self.browser.launched = true;
        }
        let _ = self.atp.commands.send(Command::BrowserNavigate {
            id: browser::PAGE_ID.into(),
            url,
        });
        // Screenshot once the page has settled.
        let cmds = self.atp.commands.clone();
        cx.spawn(async move |_this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1300))
                .await;
            let _ = cmds.send(Command::BrowserShot {
                id: browser::PAGE_ID.into(),
            });
        })
        .detach();
        cx.notify();
    }

    fn refresh_browser(&mut self, cx: &mut Context<Self>) {
        if self.browser.launched {
            let _ = self.atp.commands.send(Command::BrowserShot {
                id: browser::PAGE_ID.into(),
            });
            cx.notify();
        }
    }

    fn on_navigate(&mut self, _: &Navigate, _: &mut Window, cx: &mut Context<Self>) {
        if self.surface == Surface::Browser {
            self.browse(cx);
        }
    }

    // ── AI agent loop (prompt.show / block.push) ──────────────────────────
    /// Respond to the active approval prompt and dismiss it.
    fn respond_prompt(&mut self, action: String, cx: &mut Context<Self>) {
        if let Some(p) = self.agent_prompt.take() {
            let _ = self
                .atp
                .commands
                .send(Command::PromptRespond { id: p.id, action });
            cx.notify();
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

    /// Terminal surface: the live VT grid (a real terminal viewport). Always
    /// renders the grid so interactive TUIs (claude, vim, menus) work; OSC-133
    /// semantic blocks, when present, are surfaced as AI blocks above it.
    /// Clicking anywhere refocuses the PTY so keystrokes always reach it.
    fn terminal_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("terminal")
            .track_focus(&self.term_focus)
            .key_context("Terminal")
            .on_key_down(cx.listener(Self::on_term_key))
            // Refocus the PTY on click — otherwise interacting with other chrome
            // steals focus and keys (Enter on a TUI menu) silently go nowhere.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.term_focus, cx);
                    cx.notify();
                }),
            )
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .py(px(8.0))
            .children(self.agent_blocks.iter().map(agent_block_card))
            .child(self.terminal_grid())
    }

    /// Raw VT grid render (per-cell ANSI colour via run-coalescing), with a
    /// blinking block cursor at the PTY cursor position.
    fn terminal_grid(&self) -> impl IntoElement {
        use terminal_state::Cell;
        let cols = self.term.cols() as usize;
        let rows = self.term.rows() as usize;
        let cells = self.term.cells();
        let (cur_col, cur_row) = self.term.cursor();
        let cur_col = cur_col as usize;
        let cur_row = cur_row as usize;
        let show_cursor = self.surface == Surface::Terminal && self.cursor_on;
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
            let cursor_here = show_cursor && r == cur_row;
            // Trim trailing blank cells (keep coloured/reverse padding), but
            // always extend to the cursor column so the caret shows at EOL.
            let mut last = row
                .iter()
                .rposition(|c| {
                    c.ch != ' ' || c.style.reverse || c.style.bg != terminal_state::Color::Default
                })
                .map_or(0, |i| i + 1);
            if cursor_here {
                last = last.max(cur_col + 1);
            }
            if last == 0 {
                col = col.child(div().child(SharedString::from(" ")));
                continue;
            }
            let mut line = div().flex();
            let mut buf = String::new();
            let mut cur = key(&row[0]);
            for (ci, cell) in row[..last].iter().enumerate() {
                if cursor_here && ci == cur_col {
                    if !buf.is_empty() {
                        line = line.child(run_span(cur, std::mem::take(&mut buf)));
                    }
                    line = line.child(cursor_span(cell.ch));
                    cur = key(cell);
                    continue;
                }
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

    fn set_dialog_driver(&mut self, driver: &str, cx: &mut Context<Self>) {
        self.dialog_driver = driver.to_owned();
        cx.notify();
    }

    /// Read the dialog fields, open a real connection by path, and close.
    fn save_connection(&mut self, cx: &mut Context<Self>) {
        let path = self.dialog_path.read(cx).text().trim().to_owned();
        if path.is_empty() {
            return;
        }
        let name = self.dialog_name.read(cx).text().trim().to_owned();
        let driver = self.dialog_driver.clone();
        let id = format!("db-{}", self.next_conn);
        self.next_conn += 1;
        let display = if name.is_empty() {
            format!("{driver} · {id}")
        } else {
            name
        };
        self.db.add_connection(id.clone(), display);
        let _ = self.atp.commands.send(Command::DbConnect {
            conn: id,
            path,
            driver,
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
                } => {
                    if let Some(sql) = self.pending_sql.take() {
                        self.push_history(sql, ms, rows.len(), true);
                    }
                    self.db
                        .apply_result(columns, rows, ms, total, page, browsing, pk_columns);
                }
                Incoming::DbColumns {
                    conn,
                    table,
                    columns,
                } => {
                    if self.db.active_conn_id() == Some(conn.as_str()) {
                        self.db.set_table_columns(table, columns);
                    }
                }
                Incoming::DbError { message } => {
                    if let Some(sql) = self.pending_sql.take() {
                        self.push_history(sql, 0, 0, false);
                    }
                    self.db.apply_error(message);
                }
                Incoming::Output { session_id, data } => {
                    if session_id == self.term_id {
                        self.term.process(&data);
                    }
                }
                // Highlighting is now done locally by gpui-component's editor.
                Incoming::Highlight { .. } => {}
                Incoming::BrowserShot { png } => {
                    self.browser.shot = Some(std::sync::Arc::new(gpui::Image::from_bytes(
                        gpui::ImageFormat::Png,
                        png,
                    )));
                    self.browser.loading = false;
                    self.browser.error = None;
                }
                Incoming::BrowserError { message } => {
                    self.browser.loading = false;
                    // Allow a later retry to relaunch (e.g. once Chrome is present).
                    self.browser.launched = false;
                    self.browser.error = Some(message);
                }
                Incoming::GitStatus { branch, entries } => {
                    self.ide.git_branch = branch;
                    self.ide.git_entries = entries;
                }
                Incoming::PromptShow {
                    id,
                    title,
                    body,
                    actions,
                } => {
                    self.agent_prompt = Some(AgentPrompt {
                        id,
                        title,
                        body,
                        actions,
                    });
                }
                Incoming::BlockPush { id, title, body } => {
                    if let Some(b) = self.agent_blocks.iter_mut().find(|b| b.id == id) {
                        b.title = title;
                        b.body = body;
                    } else {
                        self.agent_blocks.push(AgentBlock { id, title, body });
                    }
                }
                Incoming::BlockClear { id } => self.agent_blocks.retain(|b| b.id != id),
            }
        }
        any
    }

    // ── Query history + export ────────────────────────────────────────────
    /// Record an executed statement (newest first, bounded, deduped).
    fn push_history(&mut self, sql: String, ms: u64, rows: usize, ok: bool) {
        let sql = sql.trim().to_owned();
        if sql.is_empty() {
            return;
        }
        if self.db_history.first().map(|h| h.sql.as_str()) == Some(sql.as_str()) {
            // Update the timing of an immediate re-run rather than duplicating.
            if let Some(h) = self.db_history.first_mut() {
                h.ms = ms;
                h.rows = rows;
                h.ok = ok;
            }
            return;
        }
        self.db_history.insert(
            0,
            database::HistEntry {
                sql,
                ms,
                rows,
                ok,
            },
        );
        self.db_history.truncate(200);
    }

    /// Open a history entry's SQL in a fresh query buffer.
    fn open_history(&mut self, sql: String, window: &mut Window, cx: &mut Context<Self>) {
        self.add_tab(window, cx);
        let editor = self.active_editor().clone();
        editor.update(cx, |e, cx| e.set_value(sql, window, cx));
        cx.notify();
    }

    /// Export the current result set to `~/.enzo/exports/` as CSV or JSON.
    fn export_results(&mut self, json: bool, cx: &mut Context<Self>) {
        if self.db.columns.is_empty() {
            return;
        }
        let body = if json {
            export_json(&self.db.columns, &self.db.rows)
        } else {
            export_csv(&self.db.columns, &self.db.rows)
        };
        let ext = if json { "json" } else { "csv" };
        match write_export(ext, &body) {
            Ok(path) => self.db.export_msg = Some(format!("exported → {path}")),
            Err(e) => self.db.export_msg = Some(format!("export failed: {e}")),
        }
        cx.notify();
    }

    // ── Database actions ──────────────────────────────────────────────────
    /// Toggle the schema catalog row for `table`, fetching its columns on first
    /// expand (lazily, via `db.schema.columns`).
    fn toggle_table(&mut self, table: String, cx: &mut Context<Self>) {
        if self.db.expanded.remove(&table) {
            cx.notify();
            return;
        }
        self.db.expanded.insert(table.clone());
        if !self.db.table_columns.contains_key(&table)
            && let Some(conn) = self.db.active_conn_id().map(str::to_owned)
        {
            let _ = self.atp.commands.send(Command::DbColumns { conn, table });
        }
        cx.notify();
    }

    /// Browse `table` in the active connection (sets SQL + pages via ATP).
    fn browse_table(&mut self, table: String, page: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(conn) = self.db.active_conn_id().map(str::to_owned) else {
            return;
        };
        let q = format!("SELECT * FROM {table} LIMIT {};", database::PAGE_SIZE);
        let editor = self.active_editor().clone();
        editor.update(cx, |e, cx| e.set_value(q.clone(), window, cx));
        self.pending_sql = Some(q);
        self.db.running = true;
        let _ = self.atp.commands.send(Command::DbBrowse {
            conn,
            table,
            page,
            size: database::PAGE_SIZE,
        });
        cx.notify();
    }

    /// Run the active query buffer's SQL against the active connection.
    fn run_query(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(conn) = self.db.active_conn_id().map(str::to_owned) else {
            return;
        };
        let sql = self.active_editor().read(cx).value().trim().to_owned();
        if sql.is_empty() {
            return;
        }
        self.pending_sql = Some(sql.clone());
        self.db.running = true;
        self.db.browsing = None;
        let _ = self.atp.commands.send(Command::DbQuery { conn, sql });
        cx.notify();
    }

    /// Page the currently-browsed table by `delta` (clamped), re-browsing.
    fn page_relative(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
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
            self.browse_table(table, next, window, cx);
        }
    }

    /// `⌘↵` handler — runs the query when the Database surface is active.
    fn on_run_query(&mut self, _: &RunQuery, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface == Surface::Database {
            self.run_query(window, cx);
        }
    }
}

impl Render for EnzoApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog = self.dialog_open.then(|| {
            database::connection_dialog(
                &self.dialog_name,
                &self.dialog_path,
                &self.dialog_driver,
                cx,
            )
            .into_any_element()
        });
        let prompt = self
            .agent_prompt
            .as_ref()
            .map(|p| self.agent_prompt_overlay(p, cx).into_any_element());
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
            .on_action(cx.listener(Self::on_navigate))
            .child(self.dock(cx))
            .child(self.sidebar(cx))
            .child(self.surface_column(cx))
            .children(dialog)
            .children(prompt)
    }
}

impl EnzoApp {
    /// AI-CLI approval card: title, body, and one button per action.
    fn agent_prompt_overlay(&self, p: &AgentPrompt, cx: &mut Context<Self>) -> impl IntoElement {
        let mut buttons = div().flex().gap(px(10.0)).pt(px(4.0));
        for action in &p.actions {
            let (bg, fg) = match action.as_str() {
                "accept" => (theme::GREEN, theme::GREEN_INK),
                "reject" => (theme::RED, theme::FG0),
                _ => (theme::PURPLE_BG, theme::PURPLE_FG),
            };
            let act = action.clone();
            buttons = buttons.child(
                div()
                    .id(SharedString::from(format!("prompt-{action}")))
                    .cursor_pointer()
                    .px(px(16.0))
                    .py(px(9.0))
                    .rounded(px(5.0))
                    .bg(bg)
                    .text_size(px(9.0))
                    .font_family(theme::FONT_PIXEL)
                    .text_color(fg)
                    .child(SharedString::from(action.to_uppercase()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.respond_prompt(act.clone(), cx);
                    })),
            );
        }
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x0e0c14cc))
            .child(
                div()
                    .w(px(460.0))
                    .bg(theme::BG_SURFACE)
                    .border_3()
                    .border_color(theme::PURPLE_BG)
                    .rounded(px(12.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .px(px(16.0))
                            .py(px(10.0))
                            .bg(theme::BG_BAR)
                            .border_b_2()
                            .border_color(theme::BORDER)
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .px(px(6.0))
                                    .py(px(2.0))
                                    .rounded(px(3.0))
                                    .bg(theme::PURPLE_BG)
                                    .text_size(px(8.0))
                                    .font_family(theme::FONT_PIXEL)
                                    .text_color(theme::PURPLE_FG)
                                    .child("AI"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .font_family(theme::FONT_PIXEL)
                                    .text_color(theme::FG1)
                                    .child(SharedString::from(p.title.clone())),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .px(px(20.0))
                            .py(px(16.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(theme::FG2)
                                    .child(SharedString::from(p.body.clone())),
                            )
                            .child(buttons),
                    ),
            )
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
                        let editor = this.active_editor().clone();
                        editor.update(cx, |e, cx| e.focus(window, cx));
                    }
                    Surface::Terminal => window.focus(&this.term_focus, cx),
                    Surface::Browser => {
                        let handle = this.url_input.read(cx).handle();
                        window.focus(&handle, cx);
                    }
                    Surface::Editor => this.enter_editor(window, cx),
                }
                cx.notify();
            }))
    }

    /// Per-surface context sidebar.
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let inner: AnyElement = match self.surface {
            Surface::Terminal => {
                terminal::sidebar(&self.ide.root(), &self.ide.git_branch).into_any_element()
            }
            Surface::Database => {
                database::sidebar(&self.db, &self.db_history, cx).into_any_element()
            }
            Surface::Editor => {
                ide::sidebar(&self.ide, &self.git_commit_input, cx).into_any_element()
            }
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
                    terminal::tab_bar(self.connected).into_any_element(),
                    self.terminal_view(cx).into_any_element(),
                    terminal::status_bar(self.connected, self.term.cols(), self.term.rows())
                        .into_any_element(),
                ),
                Surface::Database => (
                    database::tab_bar(&self.db_tabs, self.active_tab, &self.db, cx)
                        .into_any_element(),
                    database::content(&self.db, self.active_editor(), &self.cell_input, cx)
                        .into_any_element(),
                    database::status_bar(&self.db, cx).into_any_element(),
                ),
                Surface::Editor => (
                    ide::tab_bar(&self.ide).into_any_element(),
                    ide::content(&self.ide).into_any_element(),
                    ide::status_bar(&self.ide).into_any_element(),
                ),
                Surface::Browser => (
                    browser::tab_bar(&self.browser, &self.url_input, cx).into_any_element(),
                    browser::content(&self.browser).into_any_element(),
                    browser::status_bar(&self.browser).into_any_element(),
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

/// An AI agent block (`block.push`) composited in the terminal column.
fn agent_block_card(b: &AgentBlock) -> impl IntoElement {
    div()
        .border_l_3()
        .border_color(theme::PURPLE_BG)
        .pl(px(10.0))
        .mb(px(10.0))
        .font_family(theme::FONT_MONO)
        .text_size(px(12.5))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .px(px(5.0))
                        .py(px(2.0))
                        .rounded(px(3.0))
                        .bg(theme::PURPLE_BG)
                        .text_size(px(8.0))
                        .font_family(theme::FONT_PIXEL)
                        .text_color(theme::PURPLE_FG)
                        .child("AI"),
                )
                .child(
                    div()
                        .text_color(theme::PURPLE_LT)
                        .child(SharedString::from(b.title.clone())),
                ),
        )
        .child(
            div()
                .mt(px(4.0))
                .px(px(9.0))
                .py(px(6.0))
                .bg(theme::BG_CARD)
                .border_1()
                .border_color(theme::BORDER)
                .rounded(px(5.0))
                .text_size(px(11.5))
                .text_color(theme::FG2)
                .child(SharedString::from(b.body.clone())),
        )
}

/// The block cursor: a reverse-video cell at the PTY cursor position.
fn cursor_span(ch: char) -> impl IntoElement {
    let ch = if ch == ' ' || ch == '\0' { ' ' } else { ch };
    div()
        .bg(theme::TEAL)
        .text_color(theme::BG_SURFACE)
        .child(SharedString::from(ch.to_string()))
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
    // Dark-appropriate syntax colours for the code editor (the default is light).
    Theme::global_mut(cx).highlight_theme =
        gpui_component::highlighter::HighlightTheme::default_dark();
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
                // Cell-edit commit/cancel are scoped to the editing cell so they
                // don't steal Enter (newline) from the multi-line SQL editor.
                gpui::KeyBinding::new("enter", CommitEdit, Some("DbCell")),
                gpui::KeyBinding::new("escape", CancelEdit, Some("DbCell")),
                gpui::KeyBinding::new("cmd-s", SaveFile, Some("Editor")),
                gpui::KeyBinding::new("ctrl-s", SaveFile, Some("Editor")),
                gpui::KeyBinding::new("enter", Navigate, Some("Browser")),
            ]);
            let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    let app = cx.new(|cx| EnzoApp::new(window, cx));
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

/// Render a result set as CSV (RFC-4180 quoting).
fn export_csv(columns: &[String], rows: &[Vec<String>]) -> String {
    fn field(s: &str) -> String {
        if s.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_owned()
        }
    }
    let mut out = String::new();
    out.push_str(&columns.iter().map(|c| field(c)).collect::<Vec<_>>().join(","));
    out.push('\n');
    for row in rows {
        out.push_str(&row.iter().map(|c| field(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

/// Render a result set as a JSON array of objects.
fn export_json(columns: &[String], rows: &[Vec<String>]) -> String {
    let objs: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let map: serde_json::Map<String, serde_json::Value> = columns
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    (
                        c.clone(),
                        serde_json::Value::String(row.get(i).cloned().unwrap_or_default()),
                    )
                })
                .collect();
            serde_json::Value::Object(map)
        })
        .collect();
    serde_json::to_string_pretty(&objs).unwrap_or_else(|_| "[]".to_owned())
}

/// Write an export to `~/.enzo/exports/result-<epoch>.<ext>`, returning the path.
fn write_export(ext: &str, body: &str) -> std::io::Result<String> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| std::io::Error::other("no home dir"))?;
    let dir = std::path::Path::new(&home).join(".enzo").join("exports");
    std::fs::create_dir_all(&dir)?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("result-{ts}.{ext}"));
    std::fs::write(&path, body)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Path to the first-run demo database (`~/.enzo/demo.db`), creating `~/.enzo`.
fn default_db_path() -> Option<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let dir = std::path::Path::new(&home).join(".enzo");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("demo.db").to_string_lossy().into_owned())
}

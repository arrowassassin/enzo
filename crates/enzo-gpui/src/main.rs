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
mod debug;
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
    [
        RunQuery,
        CommitEdit,
        CancelEdit,
        SaveFile,
        Navigate,
        PaletteToggle,
        PaletteUp,
        PaletteDown,
        PaletteExec,
        PaletteClose,
    ]
);

/// A command-palette entry (⌘K).
#[derive(Clone, Copy)]
enum PaletteCmd {
    Go(Surface),
    AddConnection,
    GitRefresh,
    StartDebug,
    ToggleBreakpoint,
    SaveFile,
    RunQuery,
}

/// The palette's command catalogue (label, command), filtered by the query.
const PALETTE: &[(&str, PaletteCmd)] = &[
    ("Go: Terminal", PaletteCmd::Go(Surface::Terminal)),
    ("Go: Editor / IDE", PaletteCmd::Go(Surface::Editor)),
    ("Go: Browser", PaletteCmd::Go(Surface::Browser)),
    ("Go: Database", PaletteCmd::Go(Surface::Database)),
    ("Database: New connection…", PaletteCmd::AddConnection),
    ("Database: Run query (⌘↵)", PaletteCmd::RunQuery),
    ("Editor: Save file (⌘S)", PaletteCmd::SaveFile),
    ("Debug: Start", PaletteCmd::StartDebug),
    ("Debug: Toggle breakpoint at cursor", PaletteCmd::ToggleBreakpoint),
    ("Git: Refresh status", PaletteCmd::GitRefresh),
];

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
    /// Context-sidebar width (px), adjustable via the drag handle.
    sidebar_width: f32,
    /// True while the user is dragging the sidebar resize handle.
    resizing_sidebar: bool,
    /// Language servers already started (by server id), so we start each once.
    lsp_started: std::collections::HashSet<String>,
    /// The currently-open IDE document's `(server_id, uri)`, if LSP is active.
    lsp_open: Option<(String, String)>,
    /// Document version counter for `didChange`.
    lsp_version: i64,
    /// Debounce generation for coalescing rapid edits into one `didChange`.
    lsp_change_gen: u64,
    /// Active debug session, if any.
    dap: Option<debug::DapState>,
    /// Breakpoints per file path → 1-based line numbers.
    breakpoints: std::collections::HashMap<String, std::collections::BTreeSet<u32>>,
    /// Monotonic counter for unique DAP client ids.
    dap_seq: u32,
    /// Command palette (⌘K) state.
    palette_open: bool,
    palette_input: Entity<TextInput>,
    palette_sel: usize,
    /// Active AI-CLI approval prompt (id, title, body, actions), if any.
    agent_prompt: Option<AgentPrompt>,
    /// AI agent blocks composited in the terminal column (id → title, body).
    agent_blocks: Vec<AgentBlock>,
    browser: browser::BrowserState,
    url_input: Entity<TextInput>,
    git_commit_input: Entity<TextInput>,
    /// Focus handle for the live browser page (keyboard forwarding).
    browser_focus: FocusHandle,
    /// Last painted bounds of the browser viewport, for window→page coordinate
    /// mapping when forwarding mouse input.
    browser_bounds: std::rc::Rc<std::cell::Cell<Option<Bounds<gpui::Pixels>>>>,
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

/// Width of the left icon dock (used to map mouse-x to sidebar width).
const DOCK_WIDTH: f32 = 46.0;

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
        let palette_input = cx.new(|cx| TextInput::new(cx, "type a command…", ""));

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
                        if this.drain(cx) {
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
            sidebar_width: 170.0,
            resizing_sidebar: false,
            lsp_started: std::collections::HashSet::new(),
            lsp_open: None,
            lsp_version: 0,
            lsp_change_gen: 0,
            dap: None,
            breakpoints: std::collections::HashMap::new(),
            dap_seq: 0,
            palette_open: false,
            palette_input,
            palette_sel: 0,
            agent_prompt: None,
            agent_blocks: Vec::new(),
            browser: browser::BrowserState::new(),
            url_input,
            git_commit_input,
            browser_focus: cx.focus_handle(),
            browser_bounds: std::rc::Rc::new(std::cell::Cell::new(None)),
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

    // ── Debugger (DAP) ────────────────────────────────────────────────────
    /// Start a debug session for the open file's language.
    fn start_debug(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.ide.open_path.clone() else {
            return;
        };
        let language = self.ide.language.clone();
        let Some((cmd, args, adapter_id)) = dap_adapter_for(&language) else {
            let mut s = debug::DapState::new(String::new(), language.clone());
            s.ended = true;
            s.console.push(format!("no debug adapter configured for '{language}'"));
            self.dap = Some(s);
            cx.notify();
            return;
        };
        let program = path.display().to_string();
        let cwd = self.ide.root();
        self.dap_seq += 1;
        let client_id = format!("dap-{}", self.dap_seq);
        let launch = dap_launch_args(&language, &program, &cwd);
        let _ = self.atp.commands.send(Command::DapStart {
            id: client_id.clone(),
            cmd: cmd.to_owned(),
            args: args.into_iter().map(str::to_owned).collect(),
            adapter_id: adapter_id.to_owned(),
            launch,
        });
        self.dap = Some(debug::DapState::new(client_id, language));
        cx.notify();
    }

    /// `(client_id, thread_id)` of the active, stopped session.
    fn dap_thread(&self) -> Option<(String, u64)> {
        self.dap
            .as_ref()
            .and_then(|d| d.thread_id.map(|t| (d.client_id.clone(), t)))
    }

    fn dbg_continue(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some((id, thread_id)) = self.dap_thread() {
            let _ = self
                .atp
                .commands
                .send(Command::DapContinue { id, thread_id });
            if let Some(d) = self.dap.as_mut() {
                d.running = true;
                d.stopped_at = None;
            }
            cx.notify();
        }
    }

    fn dbg_step(&mut self, kind: atp::DapStepKind, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some((id, thread_id)) = self.dap_thread() {
            let _ = self.atp.commands.send(Command::DapStep {
                id,
                thread_id,
                kind,
            });
            cx.notify();
        }
    }

    fn dbg_stop(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self
            .dap
            .as_ref()
            .map(|d| d.client_id.clone())
            .filter(|id| !id.is_empty())
        {
            let _ = self.atp.commands.send(Command::DapStop { id });
        }
        self.dap = None;
        cx.notify();
    }

    /// Toggle a breakpoint at the editor's cursor line.
    fn toggle_breakpoint_at_cursor(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(editor)) = (self.ide.open_path.clone(), self.ide.editor.clone())
        else {
            return;
        };
        let line = editor.read(cx).cursor_position().line + 1; // DAP lines are 1-based
        let path_str = path.display().to_string();
        let lines: Vec<u32> = {
            let set = self.breakpoints.entry(path_str.clone()).or_default();
            if !set.remove(&line) {
                set.insert(line);
            }
            set.iter().copied().collect()
        };
        if let Some(id) = self
            .dap
            .as_ref()
            .filter(|d| !d.ended)
            .map(|d| d.client_id.clone())
        {
            let _ = self.atp.commands.send(Command::DapSetBreakpoints {
                id,
                path: path_str,
                lines,
            });
        }
        cx.notify();
    }

    // ── Command palette (⌘K) ──────────────────────────────────────────────
    /// Indices of palette commands matching the current query (subsequence
    /// fuzzy match, case-insensitive).
    fn palette_matches(&self, cx: &Context<Self>) -> Vec<usize> {
        let q = self.palette_input.read(cx).text().to_lowercase();
        PALETTE
            .iter()
            .enumerate()
            .filter(|(_, (label, _))| q.is_empty() || fuzzy_match(&label.to_lowercase(), &q))
            .map(|(i, _)| i)
            .collect()
    }

    fn on_palette_toggle(&mut self, _: &PaletteToggle, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = !self.palette_open;
        if self.palette_open {
            self.palette_sel = 0;
            self.palette_input.update(cx, |i, cx| i.set_text("", cx));
            let h = self.palette_input.read(cx).handle();
            window.focus(&h, cx);
        }
        cx.notify();
    }

    fn on_palette_close(&mut self, _: &PaletteClose, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            self.palette_open = false;
            cx.notify();
        }
    }

    fn on_palette_up(&mut self, _: &PaletteUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            self.palette_sel = self.palette_sel.saturating_sub(1);
            cx.notify();
        }
    }

    fn on_palette_down(&mut self, _: &PaletteDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            let n = self.palette_matches(cx).len();
            if n > 0 {
                self.palette_sel = (self.palette_sel + 1).min(n - 1);
            }
            cx.notify();
        }
    }

    fn on_palette_exec(&mut self, _: &PaletteExec, window: &mut Window, cx: &mut Context<Self>) {
        if !self.palette_open {
            return;
        }
        let matches = self.palette_matches(cx);
        if let Some(&idx) = matches.get(self.palette_sel.min(matches.len().saturating_sub(1))) {
            let cmd = PALETTE[idx].1;
            self.palette_open = false;
            self.run_palette_cmd(cmd, window, cx);
        }
    }

    fn run_palette_cmd(&mut self, cmd: PaletteCmd, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            PaletteCmd::Go(s) => {
                self.surface = s;
                match s {
                    Surface::Terminal => window.focus(&self.term_focus, cx),
                    Surface::Database => {
                        let e = self.active_editor().clone();
                        e.update(cx, |e, cx| e.focus(window, cx));
                    }
                    Surface::Browser => {
                        let h = self.url_input.read(cx).handle();
                        window.focus(&h, cx);
                    }
                    Surface::Editor => self.enter_editor(window, cx),
                }
            }
            PaletteCmd::AddConnection => {
                self.surface = Surface::Database;
                self.open_connection_dialog(window, cx);
            }
            PaletteCmd::GitRefresh => self.git_refresh(cx),
            PaletteCmd::StartDebug => {
                self.surface = Surface::Editor;
                self.start_debug(window, cx);
            }
            PaletteCmd::ToggleBreakpoint => self.toggle_breakpoint_at_cursor(cx),
            PaletteCmd::SaveFile => self.on_save_file(&SaveFile, window, cx),
            PaletteCmd::RunQuery => {
                self.surface = Surface::Database;
                self.run_query(window, cx);
            }
        }
        cx.notify();
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
                width: browser::PAGE_W,
                height: browser::PAGE_H,
            });
            self.browser.launched = true;
            self.start_screencast();
        }
        let _ = self.atp.commands.send(Command::BrowserNavigate {
            id: browser::PAGE_ID.into(),
            url,
        });
        // One screenshot as a fallback in case screencast doesn't start.
        let cmds = self.atp.commands.clone();
        cx.spawn(async move |_this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = cmds.send(Command::BrowserShot {
                id: browser::PAGE_ID.into(),
            });
        })
        .detach();
        cx.notify();
    }

    /// Start a live CDP screencast so the page streams frames continuously.
    fn start_screencast(&self) {
        let id = browser::PAGE_ID.to_owned();
        let _ = self.atp.commands.send(Command::BrowserInput {
            id: id.clone(),
            method: "Page.enable".into(),
            params: serde_json::json!({}),
        });
        let _ = self.atp.commands.send(Command::BrowserInput {
            id,
            method: "Page.startScreencast".into(),
            params: serde_json::json!({
                "format": "jpeg", "quality": 60,
                "maxWidth": browser::PAGE_W, "maxHeight": browser::PAGE_H,
                "everyNthFrame": 1
            }),
        });
    }

    /// Forward a CDP input event to the page.
    fn browser_cdp(&self, method: &str, params: serde_json::Value) {
        let _ = self.atp.commands.send(Command::BrowserInput {
            id: browser::PAGE_ID.into(),
            method: method.to_owned(),
            params,
        });
    }

    /// Map a window-space point to page-space `(x, y)` using the last viewport
    /// bounds, scaled to the headless page size.
    fn browser_page_xy(&self, p: gpui::Point<gpui::Pixels>) -> Option<(f32, f32)> {
        let b = self.browser_bounds.get()?;
        let w = f32::from(b.size.width);
        let h = f32::from(b.size.height);
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let x = ((f32::from(p.x) - f32::from(b.origin.x)) / w * browser::PAGE_W as f32)
            .clamp(0.0, browser::PAGE_W as f32);
        let y = ((f32::from(p.y) - f32::from(b.origin.y)) / h * browser::PAGE_H as f32)
            .clamp(0.0, browser::PAGE_H as f32);
        Some((x, y))
    }

    fn on_browser_mouse_down(
        &mut self,
        ev: &gpui::MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.browser_focus, cx);
        if let Some((x, y)) = self.browser_page_xy(ev.position) {
            self.browser_cdp(
                "Input.dispatchMouseEvent",
                serde_json::json!({
                    "type": "mousePressed", "x": x, "y": y,
                    "button": "left", "buttons": 1, "clickCount": 1
                }),
            );
        }
    }

    fn on_browser_mouse_up(
        &mut self,
        ev: &gpui::MouseUpEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        if let Some((x, y)) = self.browser_page_xy(ev.position) {
            self.browser_cdp(
                "Input.dispatchMouseEvent",
                serde_json::json!({
                    "type": "mouseReleased", "x": x, "y": y,
                    "button": "left", "buttons": 0, "clickCount": 1
                }),
            );
        }
    }

    fn on_browser_scroll(
        &mut self,
        ev: &gpui::ScrollWheelEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        let Some((x, y)) = self.browser_page_xy(ev.position) else {
            return;
        };
        let (dx, dy) = match ev.delta {
            gpui::ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            gpui::ScrollDelta::Lines(p) => (p.x * 40.0, p.y * 40.0),
        };
        self.browser_cdp(
            "Input.dispatchMouseEvent",
            serde_json::json!({
                "type": "mouseWheel", "x": x, "y": y,
                "deltaX": -dx, "deltaY": -dy
            }),
        );
    }

    fn on_browser_key(&mut self, ev: &KeyDownEvent, _: &mut Window, _: &mut Context<Self>) {
        let ks = &ev.keystroke;
        // Printable text → keyDown with text; named keys → a rawKeyDown.
        if let Some(text) = &ks.key_char {
            self.browser_cdp(
                "Input.dispatchKeyEvent",
                serde_json::json!({ "type": "keyDown", "text": text }),
            );
        } else {
            let key = match ks.key.as_str() {
                "enter" => "Enter",
                "backspace" => "Backspace",
                "tab" => "Tab",
                "escape" => "Escape",
                "up" => "ArrowUp",
                "down" => "ArrowDown",
                "left" => "ArrowLeft",
                "right" => "ArrowRight",
                _ => return,
            };
            self.browser_cdp(
                "Input.dispatchKeyEvent",
                serde_json::json!({ "type": "rawKeyDown", "key": key }),
            );
        }
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
                    .code_editor(language.clone())
                    .line_number(true)
                    .indent_guides(true)
                    .default_value(content.clone())
            });
            // Forward edits to the language server (debounced didChange).
            cx.subscribe(
                &editor,
                |this, _editor, ev: &gpui_component::input::InputEvent, cx| {
                    if matches!(ev, gpui_component::input::InputEvent::Change) {
                        this.on_editor_changed(cx);
                    }
                },
            )
            .detach();
            self.ide.editor = Some(editor);
            self.start_lsp_for(&language, path, &content);
        } else {
            self.ide.editor = None;
            self.lsp_open = None;
        }
        cx.notify();
    }

    // ── Language server (intellisense) ────────────────────────────────────
    /// Start the language server for `language` (once) and open `path` on it.
    fn start_lsp_for(&mut self, language: &str, path: &Path, text: &str) {
        let Some((server_id, cmd, args)) = lsp_server_for(language) else {
            self.lsp_open = None;
            return;
        };
        let server_id = server_id.to_owned();
        let uri = path_to_uri(&path.display().to_string());
        self.lsp_version = 1;
        if self.lsp_started.insert(server_id.clone()) {
            let _ = self.atp.commands.send(Command::LspStart {
                id: server_id.clone(),
                cmd: cmd.to_owned(),
                args: args.into_iter().map(str::to_owned).collect(),
                root_uri: path_to_uri(&self.ide.root()),
            });
        }
        let _ = self.atp.commands.send(Command::LspDidOpen {
            id: server_id.clone(),
            uri: uri.clone(),
            language_id: language.to_owned(),
            version: 1,
            text: text.to_owned(),
        });
        self.lsp_open = Some((server_id, uri));
    }

    /// An editor edit happened — schedule a debounced `didChange`.
    fn on_editor_changed(&mut self, cx: &mut Context<Self>) {
        if self.lsp_open.is_none() {
            return;
        }
        self.lsp_change_gen = self.lsp_change_gen.wrapping_add(1);
        let generation = self.lsp_change_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.lsp_change_gen == generation {
                    this.flush_lsp_change(cx);
                }
            });
        })
        .detach();
    }

    /// Send the current document text to the server (`didChange`).
    fn flush_lsp_change(&mut self, cx: &mut Context<Self>) {
        let (Some((server_id, uri)), Some(editor)) =
            (self.lsp_open.clone(), self.ide.editor.clone())
        else {
            return;
        };
        self.lsp_version += 1;
        let text = editor.read(cx).value().to_string();
        let _ = self.atp.commands.send(Command::LspDidChange {
            id: server_id,
            uri,
            version: self.lsp_version,
            text,
        });
    }

    /// Apply diagnostics to the open editor (if the uri matches).
    fn apply_diagnostics(&mut self, uri: &str, items: Vec<atp::DiagItem>, cx: &mut Context<Self>) {
        let matches = self.lsp_open.as_ref().is_some_and(|(_, u)| u == uri);
        if !matches {
            return;
        }
        let Some(editor) = self.ide.editor.clone() else {
            return;
        };
        editor.update(cx, |state, cx| {
            if let Some(set) = state.diagnostics_mut() {
                set.clear();
                for item in &items {
                    use gpui_component::highlighter::DiagnosticSeverity;
                    let severity = match item.severity {
                        1 => DiagnosticSeverity::Error,
                        2 => DiagnosticSeverity::Warning,
                        3 => DiagnosticSeverity::Info,
                        _ => DiagnosticSeverity::Hint,
                    };
                    let start = gpui_component::input::Position::new(item.start_line, item.start_col);
                    let end = gpui_component::input::Position::new(item.end_line, item.end_col);
                    set.push(
                        gpui_component::highlighter::Diagnostic::new(start..end, item.message.clone())
                            .with_severity(severity),
                    );
                }
            }
            cx.notify();
        });
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
        let bytes = keystroke_to_pty_bytes(&ks.key, ks.key_char.as_deref(), &ks.modifiers);
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

    /// Live browser page: streams CDP screencast frames and forwards mouse,
    /// scroll and keyboard input to the headless page. A transparent `canvas`
    /// records the viewport bounds each frame for window→page coordinate mapping.
    fn browser_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds_cell = self.browser_bounds.clone();
        let body: AnyElement = if let Some(err) = &self.browser.error {
            div()
                .flex()
                .flex_col()
                .size_full()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(widgets::text(&format!("✗ {err}"), 13.0, theme::RED_LT))
                .child(widgets::text(
                    "the daemon's headless browser needs a Chrome/Chromium install",
                    11.0,
                    theme::FAINT,
                ))
                .into_any_element()
        } else if let Some(shot) = &self.browser.shot {
            div()
                .relative()
                .size_full()
                .child(gpui::img(gpui::ImageSource::Image(shot.clone())).size_full())
                .child(
                    gpui::canvas(move |b, _, _| bounds_cell.set(Some(b)), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(widgets::text(
                    if self.browser.loading {
                        "◍ loading…"
                    } else {
                        "◍ enter a URL to start the headless browser"
                    },
                    14.0,
                    theme::FAINT,
                ))
                .into_any_element()
        };
        div()
            .id("browser-page")
            .track_focus(&self.browser_focus)
            .key_context("BrowserPage")
            .size_full()
            .overflow_hidden()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(Self::on_browser_mouse_down),
            )
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::on_browser_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_browser_scroll))
            .on_key_down(cx.listener(Self::on_browser_key))
            .child(body)
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
    fn drain(&mut self, cx: &mut Context<Self>) -> bool {
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
                Incoming::BrowserFrame { jpeg, session_id } => {
                    self.browser.shot = Some(std::sync::Arc::new(gpui::Image::from_bytes(
                        gpui::ImageFormat::Jpeg,
                        jpeg,
                    )));
                    self.browser.loading = false;
                    self.browser.error = None;
                    // Ack so Chrome keeps streaming frames.
                    let _ = self.atp.commands.send(Command::BrowserInput {
                        id: browser::PAGE_ID.into(),
                        method: "Page.screencastFrameAck".into(),
                        params: serde_json::json!({ "sessionId": session_id }),
                    });
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
                Incoming::LspDiagnostics { uri, items } => self.apply_diagnostics(&uri, items, cx),
                // ── DAP events ──
                Incoming::DapInitialized => {
                    if let Some(id) = self.dap.as_ref().map(|d| d.client_id.clone()) {
                        for (path, lines) in &self.breakpoints {
                            let _ = self.atp.commands.send(Command::DapSetBreakpoints {
                                id: id.clone(),
                                path: path.clone(),
                                lines: lines.iter().copied().collect(),
                            });
                        }
                        let _ = self.atp.commands.send(Command::DapConfigDone { id });
                    }
                }
                Incoming::DapStopped { thread_id, .. } => {
                    let id = if let Some(d) = self.dap.as_mut() {
                        d.thread_id = Some(thread_id);
                        d.running = false;
                        Some(d.client_id.clone())
                    } else {
                        None
                    };
                    if let Some(id) = id {
                        let _ = self.atp.commands.send(Command::DapStackTrace { id, thread_id });
                    }
                }
                Incoming::DapStackTraceResult { frames } => {
                    let next = if let Some(d) = self.dap.as_mut() {
                        d.frames = frames.clone();
                        frames.first().map(|top| {
                            d.stopped_at = Some((top.path.clone(), top.line));
                            (d.client_id.clone(), top.id)
                        })
                    } else {
                        None
                    };
                    if let Some((id, frame_id)) = next {
                        let _ = self.atp.commands.send(Command::DapScopes { id, frame_id });
                    }
                }
                Incoming::DapScopesResult { scopes } => {
                    let next = if let Some(d) = self.dap.as_mut() {
                        d.scopes = scopes.clone();
                        scopes.first().map(|s| (d.client_id.clone(), s.reference))
                    } else {
                        None
                    };
                    if let Some((id, reference)) = next {
                        let _ = self.atp.commands.send(Command::DapVariables { id, reference });
                    }
                }
                Incoming::DapVariablesResult { reference, vars } => {
                    if let Some(d) = self.dap.as_mut() {
                        let _ = reference;
                        d.variables = vars;
                    }
                }
                Incoming::DapContinued => {
                    if let Some(d) = self.dap.as_mut() {
                        d.running = true;
                        d.stopped_at = None;
                    }
                }
                Incoming::DapOutput { category: _, text } => {
                    if let Some(d) = self.dap.as_mut() {
                        d.console.push(text);
                    }
                }
                Incoming::DapTerminated => {
                    if let Some(d) = self.dap.as_mut() {
                        d.running = false;
                        d.ended = true;
                        d.stopped_at = None;
                    }
                }
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
        let palette = self
            .palette_open
            .then(|| self.palette_overlay(cx).into_any_element());
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
            .on_action(cx.listener(Self::on_palette_toggle))
            .on_action(cx.listener(Self::on_palette_close))
            .on_action(cx.listener(Self::on_palette_up))
            .on_action(cx.listener(Self::on_palette_down))
            .on_action(cx.listener(Self::on_palette_exec))
            // Sidebar resize drag: track motion/release at the root so the
            // pointer can leave the 4px handle without dropping the drag.
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _, cx| {
                if this.resizing_sidebar {
                    let w = f32::from(ev.position.x) - DOCK_WIDTH;
                    this.sidebar_width = w.clamp(130.0, 520.0);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.resizing_sidebar {
                        this.resizing_sidebar = false;
                        cx.notify();
                    }
                }),
            )
            .child(self.dock(cx))
            .child(self.sidebar(cx))
            .child(self.resize_handle(cx))
            .child(self.surface_column(cx))
            .children(dialog)
            .children(prompt)
            .children(palette)
    }
}

impl EnzoApp {
    /// Command palette (⌘K): query field + fuzzy-filtered command list.
    fn palette_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let matches = self.palette_matches(cx);
        let sel = self.palette_sel.min(matches.len().saturating_sub(1));
        let mut list = div().flex().flex_col().py(px(4.0));
        if matches.is_empty() {
            list = list.child(
                div()
                    .px(px(16.0))
                    .py(px(10.0))
                    .child(widgets::text("no matching commands", 12.0, theme::FAINT)),
            );
        }
        for (row, &idx) in matches.iter().enumerate() {
            let (label, cmd) = PALETTE[idx];
            let active = row == sel;
            let mut item = div()
                .id(SharedString::from(format!("pal-{idx}")))
                .cursor_pointer()
                .px(px(16.0))
                .py(px(7.0))
                .text_size(px(12.5))
                .text_color(if active { theme::TEAL } else { theme::FG2 });
            if active {
                item = item.bg(theme::BG_BAR);
            }
            item = item.child(SharedString::from(label)).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.palette_open = false;
                    this.run_palette_cmd(cmd, window, cx);
                },
            ));
            list = list.child(item);
        }
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(90.0))
            .bg(gpui::rgba(0x0e0c14cc))
            .child(
                div()
                    .w(px(520.0))
                    .bg(theme::BG_SURFACE)
                    .border_3()
                    .border_color(theme::PURPLE_BG)
                    .rounded(px(10.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .key_context("Palette")
                            .px(px(16.0))
                            .py(px(11.0))
                            .bg(theme::BG_BAR)
                            .border_b_2()
                            .border_color(theme::BORDER)
                            .text_size(px(13.0))
                            .font_family(theme::FONT_MONO)
                            .text_color(theme::FG0)
                            .child(self.palette_input.clone()),
                    )
                    .child(list),
            )
    }

    /// AI-CLI approval card: title, body, and one button per action.
    fn agent_prompt_overlay(&self, p: &AgentPrompt, cx: &mut Context<Self>) -> impl IntoElement {
        // Buttons wrap so an arbitrary number of options (multiselect) fit.
        let mut buttons = div().flex().flex_wrap().gap(px(8.0)).pt(px(4.0));
        for (i, action) in p.actions.iter().enumerate() {
            let lower = action.to_ascii_lowercase();
            let (bg, fg) = if lower.contains("accept")
                || lower.starts_with("yes")
                || lower == "y"
                || lower.contains("allow")
                || lower.contains("approve")
            {
                (theme::GREEN, theme::GREEN_INK)
            } else if lower.contains("reject")
                || lower.starts_with("no")
                || lower == "n"
                || lower.contains("deny")
                || lower.contains("cancel")
            {
                (theme::RED, theme::FG0)
            } else {
                (theme::PURPLE_BG, theme::PURPLE_FG)
            };
            let act = action.clone();
            buttons = buttons.child(
                div()
                    .id(SharedString::from(format!("prompt-{i}")))
                    .cursor_pointer()
                    .px(px(14.0))
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
                    .w(px(520.0))
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
                            .gap(px(10.0))
                            .px(px(20.0))
                            .py(px(16.0))
                            .child(
                                // Body rendered as Markdown (code blocks, lists,
                                // emphasis) instead of flat text.
                                div()
                                    .max_h(px(380.0))
                                    .overflow_hidden()
                                    .text_size(px(12.5))
                                    .text_color(theme::FG2)
                                    .child(gpui_component::text::markdown(p.body.clone())),
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
            .w(px(self.sidebar_width))
            .h_full()
            .py(px(10.0))
            .bg(theme::BG_SIDE)
            .overflow_hidden()
            .child(inner)
    }

    /// Draggable divider that resizes the context sidebar.
    fn resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("sidebar-resize")
            .w(px(4.0))
            .h_full()
            .flex_none()
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .bg(if self.resizing_sidebar {
                theme::TEAL
            } else {
                theme::BORDER
            })
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.resizing_sidebar = true;
                    cx.notify();
                }),
            )
    }

    /// Editor tab bar: open-file chip + debug toolbar.
    fn editor_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.ide.open_path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || "no file".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(7.0))
            .bg(theme::BG_BAR)
            .border_b_2()
            .border_color(theme::BORDER)
            .child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(3.0))
                    .bg(theme::BG_SURFACE)
                    .text_size(px(8.0))
                    .font_family(theme::FONT_PIXEL)
                    .text_color(theme::TEAL)
                    .child(SharedString::from(name)),
            )
            .child(debug::toolbar(
                self.dap.as_ref(),
                self.ide.open_path.is_some(),
                cx,
            ))
    }

    /// Editor content: code editor + (when debugging) the debug panel beneath it.
    fn editor_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(div().flex_1().overflow_hidden().child(ide::content(&self.ide)))
            .children(self.dap.as_ref().map(|d| debug::panel(d, cx)))
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
                    self.editor_tab_bar(cx).into_any_element(),
                    self.editor_content(cx).into_any_element(),
                    ide::status_bar(&self.ide).into_any_element(),
                ),
                Surface::Browser => (
                    browser::tab_bar(&self.browser, &self.url_input, cx).into_any_element(),
                    self.browser_view(cx).into_any_element(),
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
                gpui::KeyBinding::new("enter", Navigate, Some("BrowserUrl")),
                // Command palette: ⌘K (and ctrl-shift-p so it never steals a
                // terminal control key like ctrl-k).
                gpui::KeyBinding::new("cmd-k", PaletteToggle, None),
                gpui::KeyBinding::new("ctrl-shift-p", PaletteToggle, None),
                gpui::KeyBinding::new("up", PaletteUp, Some("Palette")),
                gpui::KeyBinding::new("down", PaletteDown, Some("Palette")),
                gpui::KeyBinding::new("enter", PaletteExec, Some("Palette")),
                gpui::KeyBinding::new("escape", PaletteClose, Some("Palette")),
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

/// Translate a keystroke into the raw bytes to write to the PTY.
///
/// Order matters: control combos first, then named special keys, then the
/// printable `key_char`. Named keys must beat `key_char` so Return is always
/// CR (`\r`, 0x0D) — raw-mode TUIs (Ink/Claude Code, vim, readline) only treat
/// CR as Return, and macOS frequently reports Return's `key_char` as `\n`.
fn keystroke_to_pty_bytes(key: &str, key_char: Option<&str>, m: &gpui::Modifiers) -> Vec<u8> {
    if m.control {
        let b = match key {
            "space" => vec![0],
            "[" => vec![0x1b],
            "\\" => vec![0x1c],
            "]" => vec![0x1d],
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphabetic() => {
                vec![k.as_bytes()[0].to_ascii_lowercase() - b'a' + 1]
            }
            _ => Vec::new(),
        };
        if !b.is_empty() {
            return b;
        }
    }
    let named = match key {
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
        _ => Vec::new(),
    };
    if !named.is_empty() {
        return named;
    }
    if let Some(ch) = key_char {
        let mut bytes = ch.as_bytes().to_vec();
        if m.alt {
            let mut prefixed = vec![0x1b];
            prefixed.append(&mut bytes);
            return prefixed;
        }
        return bytes;
    }
    Vec::new()
}

/// Subsequence fuzzy match: every char of `needle` appears in `haystack` in
/// order (both already lowercased).
fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|c| chars.any(|h| h == c))
}

/// Map a language id to its debug adapter `(cmd, args, adapterID)`. The adapter
/// binary must be installed (debugpy/lldb-dap/dlv); a missing one fails
/// gracefully. JS/Node (vscode-js-debug, TCP) is deferred.
fn dap_adapter_for(language: &str) -> Option<(&'static str, Vec<&'static str>, &'static str)> {
    match language {
        "python" => Some(("python3", vec!["-m", "debugpy.adapter"], "debugpy")),
        "rust" => Some(("lldb-dap", vec![], "lldb")),
        "go" => Some(("dlv", vec!["dap"], "go")),
        _ => None,
    }
}

/// Per-adapter `launch` arguments. For compiled languages `program` should be a
/// built binary; for interpreted languages it's the source file directly.
fn dap_launch_args(language: &str, program: &str, cwd: &str) -> serde_json::Value {
    match language {
        "python" => serde_json::json!({
            "request": "launch", "program": program,
            "console": "internalConsole", "cwd": cwd, "stopOnEntry": false
        }),
        "go" => serde_json::json!({
            "request": "launch", "mode": "debug", "program": program, "cwd": cwd
        }),
        // rust/lldb: program must be a compiled binary; the source path is a
        // best-effort default the user can override.
        _ => serde_json::json!({
            "request": "launch", "program": program, "cwd": cwd, "args": []
        }),
    }
}

/// Map a language id to its language server (`server_id`, binary, args), if one
/// is known. The server binary must be on `PATH`; if it's missing the start
/// simply fails and the editor works without diagnostics.
fn lsp_server_for(language: &str) -> Option<(&'static str, &'static str, Vec<&'static str>)> {
    match language {
        "rust" => Some(("lsp-rust", "rust-analyzer", vec![])),
        "python" => Some(("lsp-python", "pylsp", vec![])),
        "javascript" => Some(("lsp-js", "typescript-language-server", vec!["--stdio"])),
        _ => None,
    }
}

/// Build a `file://` URI from an absolute path string.
fn path_to_uri(path: &str) -> String {
    format!("file://{path}")
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

#[cfg(test)]
mod term_key_tests {
    use super::keystroke_to_pty_bytes;

    fn mods() -> gpui::Modifiers {
        gpui::Modifiers::default()
    }

    #[test]
    fn enter_is_carriage_return_even_with_newline_key_char() {
        // The macOS case that broke Ink/Claude Code: key_char reported as "\n".
        assert_eq!(keystroke_to_pty_bytes("enter", Some("\n"), &mods()), vec![b'\r']);
        assert_eq!(keystroke_to_pty_bytes("enter", None, &mods()), vec![b'\r']);
    }

    #[test]
    fn tab_and_backspace_beat_key_char() {
        assert_eq!(keystroke_to_pty_bytes("tab", Some("\t"), &mods()), vec![b'\t']);
        assert_eq!(keystroke_to_pty_bytes("backspace", None, &mods()), vec![0x7f]);
    }

    #[test]
    fn arrows_emit_csi() {
        assert_eq!(keystroke_to_pty_bytes("up", None, &mods()), vec![0x1b, b'[', b'A']);
        assert_eq!(keystroke_to_pty_bytes("left", None, &mods()), vec![0x1b, b'[', b'D']);
    }

    #[test]
    fn printable_uses_key_char() {
        assert_eq!(keystroke_to_pty_bytes("a", Some("a"), &mods()), vec![b'a']);
    }

    #[test]
    fn ctrl_c_is_etx() {
        let m = gpui::Modifiers {
            control: true,
            ..Default::default()
        };
        assert_eq!(keystroke_to_pty_bytes("c", Some("c"), &m), vec![3]);
    }

    #[test]
    fn alt_prefixes_escape() {
        let m = gpui::Modifiers {
            alt: true,
            ..Default::default()
        };
        assert_eq!(keystroke_to_pty_bytes("b", Some("b"), &m), vec![0x1b, b'b']);
    }
}

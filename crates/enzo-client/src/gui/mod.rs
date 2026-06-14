//! egui/eframe workspace UI, styled faithfully to `design/mockups/*.html`.
//!
//! Left icon dock → context sidebar → surface-aware tab strip → central surface
//! → pixel-font status bar. ⌘K command palette, settings overlay, and
//! mouse-driven agent approval cards. Pixel (Silkscreen) labels over
//! `JetBrains Mono` body text.

mod terminal_view;
pub mod theme;

use std::sync::mpsc::{Receiver, Sender};

use egui::{Align, CornerRadius, FontId, Layout, Margin, RichText, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};

use crate::atp::{AtpClient, DaemonMessage};
use crate::overlay::{Block, DiffLineKind, OverlayState, PromptCard};
use crate::surface::{BrowserPanel, BrowserState, DB_PAGE_SIZE, DbState, IdeState, Surface};
use crate::terminal::{DEFAULT_COLS, DEFAULT_ROWS, Terminal};
use crate::ui::UiState;

const DEFAULT_SOCK: &str = "/tmp/enzo-atp.sock";

// ── Channel message types ──────────────────────────────────────────────────────

enum UiCommand {
    NewSession {
        id: String,
        cols: u16,
        rows: u16,
    },
    CloseSession {
        id: String,
    },
    Input {
        id: String,
        data: Vec<u8>,
    },
    Resize {
        id: String,
        cols: u16,
        rows: u16,
    },
    PromptRespond {
        id: String,
        action: String,
    },
    BrowserOpen {
        id: String,
        url: String,
        w: u32,
        h: u32,
    },
    BrowserNavigate {
        id: String,
        url: String,
    },
    BrowserShot {
        id: String,
    },
    BrowserInput {
        id: String,
        method: String,
        params: serde_json::Value,
    },
    DbConnect {
        conn: String,
        path: String,
        /// Seed a fresh demo schema after connecting (first-run default db).
        seed: bool,
    },
    DbQuery {
        conn: String,
        sql: String,
    },
    DbBrowseTable {
        conn: String,
        table: String,
        page: u64,
        size: u64,
    },
}

enum Incoming {
    Connected,
    Message(DaemonMessage),
    BrowserReady,
    BrowserFrame(egui::ColorImage),
    DbConnected {
        conn: String,
        driver: String,
    },
    DbTables {
        conn: String,
        tables: Vec<crate::surface::TableInfo>,
    },
    DbResult {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        ms: u64,
        /// Total row count when this came from a table browse (drives the pager).
        total: Option<u64>,
        page: u64,
        /// Table being browsed, if this result was a browse.
        browsing: Option<String>,
    },
    DbError {
        message: String,
    },
}

// ── Entry point ────────────────────────────────────────────────────────────────

/// Launch the Enzo client window. Blocks until the window is closed.
///
/// # Errors
/// Returns an error if the windowing/GPU backend fails to initialise.
pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("enzo")
            .with_inner_size([1600.0, 940.0])
            .with_min_inner_size([960.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "enzo",
        options,
        Box::new(|cc| Ok(Box::new(EnzoApp::new(cc)))),
    )
}

// ── App ─────────────────────────────────────────────────────────────────────────

#[allow(clippy::struct_excessive_bools, reason = "independent UI toggles")]
#[doc(hidden)]
pub struct EnzoApp {
    terminals: Vec<(String, Terminal)>,
    ui: UiState,
    surface: Surface,
    ide: IdeState,
    db: DbState,
    browser: BrowserState,
    overlay: OverlayState,
    sidebar_open: bool,
    palette_open: bool,
    palette_query: String,
    settings_open: bool,
    active_theme: usize,

    // CDP browser screenshot stream
    browser_launched: bool,
    browser_pending: bool,
    browser_tex: Option<egui::TextureHandle>,
    browser_size: (u32, u32),

    incoming: Receiver<Incoming>,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<UiCommand>,
    next_session: u32,
}

const BROWSER_ID: &str = "web-0";
const BROWSER_W: u32 = 1280;
const BROWSER_H: u32 = 800;

/// Built-in theme names (mirrors `enzo-theme`); selection is visual for now.
const THEMES: &[&str] = &[
    "Enzo Dark",
    "Enzo Light",
    "Tokyo Night",
    "Matrix",
    "Game Boy DMG",
    "Amber CRT",
];

impl EnzoApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::build(cc, true)
    }

    /// Build the app shell. When `connect` is `true` (production), spawn the
    /// background ATP thread that talks to the daemon socket. When `false`
    /// (headless UI tests), skip the spawn entirely so the app never touches the
    /// socket and stays in the disconnected state forever, making snapshots
    /// deterministic regardless of any running daemon.
    fn build(cc: &eframe::CreationContext<'_>, connect: bool) -> Self {
        theme::install(&cc.egui_ctx);

        let sock = std::env::var("ENZO_ATP_SOCK").unwrap_or_else(|_| DEFAULT_SOCK.to_owned());
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<UiCommand>();
        let (in_tx, in_rx) = std::sync::mpsc::channel::<Incoming>();

        if connect {
            let ctx = cc.egui_ctx.clone();
            std::thread::Builder::new()
                .name("enzo-atp".into())
                .spawn(move || {
                    let rt = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("tokio runtime");
                    rt.block_on(run_atp(sock, ctx, in_tx, cmd_rx));
                })
                .expect("spawn atp thread");
        } else {
            // Offline (test) mode: keep `sock` unused and drop the channel ends the
            // background thread would have owned, so no socket access ever occurs.
            let _ = sock;
            drop(cmd_rx);
            drop(in_tx);
        }

        let project_root = std::env::current_dir()
            .map_or_else(|_| ".".to_string(), |p| p.to_string_lossy().into_owned());

        let mut app = Self {
            terminals: Vec::new(),
            ui: UiState::new(),
            surface: Surface::Terminal,
            ide: IdeState::new(project_root),
            db: DbState::new(),
            browser: BrowserState::demo(),
            overlay: OverlayState::new(),
            sidebar_open: true,
            palette_open: false,
            palette_query: String::new(),
            settings_open: false,
            active_theme: 0,
            browser_launched: false,
            browser_pending: false,
            browser_tex: None,
            browser_size: (BROWSER_W, BROWSER_H),
            incoming: in_rx,
            cmd_tx,
            next_session: 0,
        };
        app.spawn_terminal();
        app
    }

    // ── Sessions ────────────────────────────────────────────────────────────────

    fn spawn_terminal(&mut self) {
        let id = format!("enzo-{}", self.next_session);
        self.next_session += 1;
        self.ui
            .add_tab(id.clone(), format!("zsh {}", self.next_session));
        self.terminals
            .push((id.clone(), Terminal::new(DEFAULT_COLS, DEFAULT_ROWS)));
        let _ = self.cmd_tx.send(UiCommand::NewSession {
            id,
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
        });
        self.surface = Surface::Terminal;
    }

    fn close_active_terminal(&mut self) {
        if let Some(id) = self.ui.close_active() {
            self.terminals.retain(|(sid, _)| sid != &id);
            let _ = self.cmd_tx.send(UiCommand::CloseSession { id });
        }
    }

    fn active_terminal_idx(&self) -> Option<usize> {
        let id = self.ui.active_session_id()?;
        self.terminals.iter().position(|(sid, _)| sid == id)
    }

    fn set_active_tab(&mut self, target: usize) {
        let n = self.ui.tab_count();
        for _ in 0..n {
            if self.ui.active_index() == target {
                break;
            }
            self.ui.next_tab();
        }
    }

    // ── Incoming ────────────────────────────────────────────────────────────────

    fn drain_incoming(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.incoming.try_recv() {
            match msg {
                Incoming::Connected => {
                    self.ui.connected = true;
                    self.ensure_default_db();
                }
                Incoming::Message(DaemonMessage::Output { session_id, data }) => {
                    if let Some((_, t)) = self.terminals.iter_mut().find(|(s, _)| *s == session_id)
                    {
                        t.process(&data);
                    }
                }
                Incoming::Message(DaemonMessage::PromptShow {
                    id,
                    title,
                    body,
                    diff,
                    actions,
                    ..
                }) => self
                    .overlay
                    .set_prompt(PromptCard::new(id, title, body, diff, actions)),
                Incoming::Message(DaemonMessage::BlockPush {
                    id, title, body, ..
                }) => {
                    self.overlay.push_block(Block { id, title, body });
                }
                Incoming::Message(DaemonMessage::BlockClear { id }) => {
                    self.overlay.clear_block(&id);
                }
                Incoming::Message(DaemonMessage::Closed) => self.ui.connected = false,
                Incoming::DbConnected { conn, driver } => {
                    self.db.set_driver(&conn, driver);
                }
                Incoming::DbTables { conn, tables } => {
                    self.db.set_tables(&conn, tables);
                }
                Incoming::DbResult {
                    columns,
                    rows,
                    ms,
                    total,
                    page,
                    browsing,
                } => {
                    self.db.apply_result(columns, rows, ms);
                    self.db.browsing = browsing;
                    self.db.total_rows = total;
                    self.db.page = page;
                }
                Incoming::DbError { message } => self.db.apply_error(message),
                Incoming::BrowserReady => {
                    self.browser_launched = true;
                    self.browser_pending = false;
                }
                Incoming::BrowserFrame(img) => {
                    self.browser_size = (
                        u32::try_from(img.size[0]).unwrap_or(BROWSER_W),
                        u32::try_from(img.size[1]).unwrap_or(BROWSER_H),
                    );
                    self.browser_tex =
                        Some(ctx.load_texture("browser", img, egui::TextureOptions::LINEAR));
                    self.browser_pending = false;
                }
            }
        }
    }

    fn respond_prompt(&mut self, action: &str) {
        if let Some(card) = &self.overlay.prompt {
            let _ = self.cmd_tx.send(UiCommand::PromptRespond {
                id: card.id.clone(),
                action: action.to_owned(),
            });
        }
        if action != "edit" {
            self.overlay.clear_prompt();
        }
    }

    // ── Database actions ──────────────────────────────────────────────────────

    /// On first daemon connection, open a real on-disk demo `SQLite` database so
    /// the surface isn't empty. The file lives at `~/.enzo/demo.db` and is
    /// seeded once with a couple of small tables (idempotent).
    fn ensure_default_db(&mut self) {
        if !self.db.connections.is_empty() {
            return;
        }
        let Some(path) = default_db_path() else {
            return;
        };
        let id = self.db.add_connection("SQLite · demo.db", &path);
        let _ = self.cmd_tx.send(UiCommand::DbConnect {
            conn: id,
            path,
            seed: true,
        });
    }

    /// Run the active query tab's SQL against the active connection.
    fn run_active_query(&mut self) {
        let (Some(conn), sql) = (
            self.db.active_conn_id().map(str::to_owned),
            self.db.active_sql().trim().to_owned(),
        ) else {
            return;
        };
        if sql.is_empty() {
            return;
        }
        self.db.running = true;
        self.db.browsing = None;
        self.db.total_rows = None;
        let _ = self.cmd_tx.send(UiCommand::DbQuery { conn, sql });
    }

    /// Browse `table` in the active connection: fill the current tab with a
    /// `SELECT … LIMIT` and page through it via `db.table.browse`.
    fn browse_table(&mut self, table: &str, page: u64) {
        let Some(conn) = self.db.active_conn_id().map(str::to_owned) else {
            return;
        };
        *self.db.active_sql_mut() = format!("SELECT * FROM {table} LIMIT {DB_PAGE_SIZE};");
        self.db.running = true;
        let _ = self.cmd_tx.send(UiCommand::DbBrowseTable {
            conn,
            table: table.to_owned(),
            page,
            size: DB_PAGE_SIZE,
        });
    }

    /// Connect using the path entered in the add-connection dialog.
    fn connect_from_dialog(&mut self) {
        let path = self.db.dialog_path.trim().to_owned();
        if path.is_empty() {
            return;
        }
        let name = connection_display_name(&path);
        let id = self.db.add_connection(name, &path);
        let _ = self.cmd_tx.send(UiCommand::DbConnect {
            conn: id,
            path,
            seed: false,
        });
        self.db.dialog_open = false;
        self.db.dialog_path.clear();
    }
}

/// Path to the first-run demo database (`~/.enzo/demo.db`), creating `~/.enzo`.
fn default_db_path() -> Option<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let dir = std::path::Path::new(&home).join(".enzo");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("demo.db").to_string_lossy().into_owned())
}

/// Build a sidebar display name from a connection path.
fn connection_display_name(path: &str) -> String {
    if path == ":memory:" {
        return "SQLite · memory".to_owned();
    }
    let file = std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |f| f.to_string_lossy().into_owned());
    format!("SQLite · {file}")
}

// ── eframe::App ──────────────────────────────────────────────────────────────────

impl eframe::App for EnzoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_incoming(ctx);
        self.handle_global_keys(ctx);
        if self.overlay.prompt.is_none() && !self.palette_open && !self.settings_open {
            self.handle_terminal_input(ctx);
        }

        self.top_bar(ctx);
        self.status_bar(ctx);
        self.dock(ctx);
        if self.sidebar_open {
            self.sidebar(ctx);
        }
        self.central(ctx);
        self.draw_overlay(ctx);
        self.draw_settings(ctx);
        self.draw_db_dialog(ctx);
        self.draw_palette(ctx);

        if self.surface == Surface::Terminal {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
}

// ── Test support ────────────────────────────────────────────────────────────────
//
// Hooks used by `tests/ui_snapshot.rs` to drive the headless `egui_kittest`
// harness. The dock icons and tab chips are custom `Frame`-based buttons that do
// not expose AccessKit labels, so the integration test cannot click them via the
// accessibility tree. These thin, `#[doc(hidden)]` wrappers let the test switch
// surfaces, toggle overlays, and exercise the DB / IDE state through the same
// code paths the real UI uses. They do not change any runtime behavior.

/// Construct an [`EnzoApp`] for headless UI tests, fully offline.
///
/// Unlike the production path, this NEVER spawns the background ATP thread, so
/// no socket access ever occurs and `ui.connected` stays `false` forever. This
/// makes the egui_kittest snapshots deterministic regardless of whether an
/// `enzo-daemon` happens to be listening on the ATP socket.
#[doc(hidden)]
#[must_use]
pub fn __new_app_for_test_offline(cc: &eframe::CreationContext<'_>) -> EnzoApp {
    EnzoApp::build(cc, false)
}

/// Construct an [`EnzoApp`] for headless UI tests.
///
/// Repoints to the offline constructor so the test app never touches the socket.
#[doc(hidden)]
#[must_use]
pub fn __new_app_for_test(cc: &eframe::CreationContext<'_>) -> EnzoApp {
    __new_app_for_test_offline(cc)
}

#[doc(hidden)]
impl EnzoApp {
    /// Currently displayed surface.
    #[must_use]
    pub fn __surface(&self) -> Surface {
        self.surface
    }

    /// Switch the displayed surface (mirrors a dock-icon click).
    pub fn __set_surface(&mut self, surface: Surface) {
        self.surface = surface;
    }

    /// Open or close the ⌘K command palette.
    pub fn __set_palette_open(&mut self, open: bool) {
        self.palette_open = open;
        if open {
            self.palette_query.clear();
        }
    }

    /// Open or close the settings overlay.
    pub fn __set_settings_open(&mut self, open: bool) {
        self.settings_open = open;
    }

    /// Number of open Database query tabs.
    #[must_use]
    pub fn __db_tab_count(&self) -> usize {
        self.db.tabs.len()
    }

    /// Add a Database query tab (mirrors clicking the "+" in the DB tab strip).
    pub fn __db_add_query_tab(&mut self) {
        self.db.add_query_tab();
    }

    /// Inject a connection with the given tables (mirrors a successful connect).
    /// For deterministic offline snapshots only — routes through the same state
    /// the real `DbConnected`/`DbTables` handlers use.
    pub fn __db_add_connection(&mut self, name: &str, path: &str, tables: &[&str]) {
        let id = self.db.add_connection(name, path);
        self.db.set_driver(&id, "sqlite");
        let infos = tables
            .iter()
            .map(|t| crate::surface::TableInfo {
                name: (*t).to_owned(),
                kind: "table".to_owned(),
            })
            .collect();
        self.db.set_tables(&id, infos);
    }

    /// Inject a result set (mirrors a `DbResult` from a successful query).
    pub fn __db_apply_result(&mut self, columns: &[&str], rows: &[&[&str]], ms: u64) {
        let columns = columns.iter().map(|c| (*c).to_owned()).collect();
        let rows = rows
            .iter()
            .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
            .collect();
        self.db.apply_result(columns, rows, ms);
    }

    /// Inject a query error (mirrors a `DbError`).
    pub fn __db_set_error(&mut self, message: &str) {
        self.db.apply_error(message);
    }

    /// Open/close the add-connection dialog (mirrors the sidebar "+" button).
    pub fn __db_set_dialog_open(&mut self, open: bool) {
        self.db.dialog_open = open;
    }

    /// Number of visible rows in the IDE file explorer (expanded dirs inlined).
    #[must_use]
    pub fn __ide_entry_count(&self) -> usize {
        self.ide.entries.len()
    }

    /// `true` if the IDE explorer has at least one directory at the top level.
    #[must_use]
    pub fn __ide_first_dir_index(&self) -> Option<usize> {
        self.ide.entries.iter().position(|e| e.is_dir)
    }

    /// Activate (expand/collapse a dir, or open a file) explorer row `index`
    /// (mirrors clicking a row in the IDE explorer).
    pub fn __ide_activate(&mut self, index: usize) {
        self.ide.activate(index);
    }

    /// `true` if the IDE directory at explorer row `index` is expanded.
    #[must_use]
    pub fn __ide_is_expanded(&self, index: usize) -> bool {
        self.ide
            .entries
            .get(index)
            .is_some_and(|e| self.ide.is_expanded(&e.path))
    }
}

// ── Small style helpers ──────────────────────────────────────────────────────────

/// A Silkscreen pixel section header (e.g. "SESSIONS").
fn pixel_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(text)
            .font(theme::pixel(8.0))
            .color(theme::PURPLE),
    );
    ui.add_space(3.0);
}

/// Draw a filled, rounded badge with pixel text; returns the click response.
fn badge(ui: &mut egui::Ui, text: &str, fg: egui::Color32, bg: egui::Color32) -> egui::Response {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(theme::pixel(8.0)).color(fg));
        })
        .response
}

// ── Input ─────────────────────────────────────────────────────────────────────────

impl EnzoApp {
    fn handle_global_keys(&mut self, ctx: &egui::Context) {
        let (cmd_k, cmd_t, cmd_w, switch, esc) = ctx.input(|i| {
            let m = i.modifiers.command;
            (
                m && i.key_pressed(egui::Key::K),
                m && i.key_pressed(egui::Key::T),
                m && i.key_pressed(egui::Key::W),
                [
                    m && i.key_pressed(egui::Key::Num1),
                    m && i.key_pressed(egui::Key::Num2),
                    m && i.key_pressed(egui::Key::Num3),
                    m && i.key_pressed(egui::Key::Num4),
                ],
                i.key_pressed(egui::Key::Escape),
            )
        });

        if cmd_k {
            self.palette_open = !self.palette_open;
            self.palette_query.clear();
        }
        if esc {
            self.palette_open = false;
            self.settings_open = false;
        }
        if cmd_t {
            self.spawn_terminal();
        }
        if cmd_w && self.surface == Surface::Terminal {
            self.close_active_terminal();
        }
        for (i, on) in switch.iter().enumerate() {
            if *on {
                self.surface = [
                    Surface::Terminal,
                    Surface::Ide,
                    Surface::Database,
                    Surface::Browser,
                ][i];
            }
        }
    }

    fn handle_terminal_input(&mut self, ctx: &egui::Context) {
        if self.surface != Surface::Terminal || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let Some(id) = self.ui.active_session_id().map(str::to_owned) else {
            return;
        };
        let mut out: Vec<u8> = Vec::new();
        ctx.input(|i| {
            for ev in &i.events {
                match ev {
                    egui::Event::Text(t) => out.extend_from_slice(t.as_bytes()),
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        push_key_bytes(&mut out, *key, *modifiers);
                    }
                    _ => {}
                }
            }
        });
        if !out.is_empty() {
            let _ = self.cmd_tx.send(UiCommand::Input { id, data: out });
        }
    }
}

fn push_key_bytes(out: &mut Vec<u8>, key: egui::Key, mods: egui::Modifiers) {
    use egui::Key;
    if mods.ctrl
        && !mods.command
        && let Some(b) = ctrl_byte(key)
    {
        out.push(b);
        return;
    }
    let bytes: &[u8] = match key {
        Key::Enter => b"\r",
        Key::Tab => b"\t",
        Key::Backspace => b"\x7f",
        Key::Escape => b"\x1b",
        Key::ArrowUp => b"\x1b[A",
        Key::ArrowDown => b"\x1b[B",
        Key::ArrowRight => b"\x1b[C",
        Key::ArrowLeft => b"\x1b[D",
        Key::Home => b"\x1b[H",
        Key::End => b"\x1b[F",
        Key::PageUp => b"\x1b[5~",
        Key::PageDown => b"\x1b[6~",
        Key::Delete => b"\x1b[3~",
        _ => b"",
    };
    out.extend_from_slice(bytes);
}

fn ctrl_byte(key: egui::Key) -> Option<u8> {
    let b = key.name().as_bytes();
    if b.len() == 1 && b[0].is_ascii_alphabetic() {
        Some(b[0].to_ascii_uppercase() - b'A' + 1)
    } else {
        None
    }
}

// ── Top bar ─────────────────────────────────────────────────────────────────────

impl EnzoApp {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar")
            .exact_height(40.0)
            .frame(bar_frame(theme::BG_BAR))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("▘ enzo")
                            .color(theme::TEAL)
                            .size(15.0)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    ui.label(RichText::new("│").color(theme::BORDER));
                    ui.add_space(6.0);

                    match self.surface {
                        Surface::Terminal => self.terminal_tabs(ui),
                        Surface::Ide => plain_label(ui, "EDITOR"),
                        Surface::Database => self.db_query_tabs(ui),
                        Surface::Browser => plain_label(ui, "BROWSER"),
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (txt, col) = if self.ui.connected {
                            ("● ATP LIVE", theme::TEAL)
                        } else {
                            ("○ ATP", theme::FAINT)
                        };
                        ui.label(RichText::new(txt).font(theme::pixel(8.0)).color(col));
                        ui.add_space(12.0);
                        if badge(ui, "⌘K  SEARCH", theme::FG1, theme::BG_CARD)
                            .interact(egui::Sense::click())
                            .clicked()
                        {
                            self.palette_open = true;
                        }
                    });
                });
            });
    }

    fn terminal_tabs(&mut self, ui: &mut egui::Ui) {
        let active = self.ui.active_index();
        let tabs: Vec<String> = self.ui.tabs().iter().map(|t| t.title.clone()).collect();
        let mut switch: Option<usize> = None;
        let mut close: Option<usize> = None;
        for (i, title) in tabs.iter().enumerate() {
            let selected = i == active;
            let resp = tab_chip(ui, title, selected);
            if resp.clicked() {
                switch = Some(i);
            }
            if resp.secondary_clicked() {
                close = Some(i);
            }
        }
        if tab_plus(ui).clicked() {
            self.spawn_terminal();
        }
        if let Some(i) = switch {
            self.set_active_tab(i);
        }
        if let Some(i) = close {
            self.set_active_tab(i);
            self.close_active_terminal();
        }
    }

    fn db_query_tabs(&mut self, ui: &mut egui::Ui) {
        let active = self.db.active_tab;
        let titles: Vec<String> = self.db.tabs.iter().map(|t| t.title.clone()).collect();
        for (i, title) in titles.iter().enumerate() {
            if tab_chip(ui, title, i == active).clicked() {
                self.db.active_tab = i;
            }
        }
        if tab_plus(ui).clicked() {
            self.db.add_query_tab();
        }
    }
}

/// A tab chip — active gets a teal-stroked surface fill, like the mockups.
fn tab_chip(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let (fill, fg, stroke) = if selected {
        (
            theme::BG_SURFACE,
            theme::TEAL,
            Stroke::new(1.0, theme::TEAL),
        )
    } else {
        (theme::BG_BAR, theme::FG1, Stroke::NONE)
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(label).font(theme::pixel(8.0)).color(fg));
        })
        .response
        .interact(egui::Sense::click())
}

fn tab_plus(ui: &mut egui::Ui) -> egui::Response {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.label(RichText::new("+").color(theme::FG1).size(14.0));
        })
        .response
        .interact(egui::Sense::click())
}

/// Open `url` in the user's default browser via the OS opener.
fn open_url(url: &str) {
    let url = if url.contains("://") {
        url.to_owned()
    } else {
        format!("https://{url}")
    };
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "linux")]
    let opener = "xdg-open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    let _ = std::process::Command::new(opener).arg(&url).spawn();
}

/// A plain pixel label used as a placeholder tab strip.
fn plain_label(ui: &mut egui::Ui, label: &str) {
    ui.label(
        RichText::new(label)
            .font(theme::pixel(8.0))
            .color(theme::FG1),
    );
}

fn bar_frame(fill: egui::Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .inner_margin(Margin::symmetric(10, 4))
        .stroke(Stroke::new(1.0, theme::BORDER))
}

// ── Dock ────────────────────────────────────────────────────────────────────────

impl EnzoApp {
    fn dock(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("dock")
            .exact_width(48.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DOCK)
                    .inner_margin(Margin::symmetric(4, 10)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let items = [
                        (Surface::Terminal, theme::ICON_TERMINAL, "Terminal"),
                        (Surface::Ide, theme::ICON_CODE, "Editor"),
                        (Surface::Browser, theme::ICON_WORLD, "Browser"),
                        (Surface::Database, theme::ICON_DATABASE, "Database"),
                    ];
                    for (surf, icon, tip) in items {
                        if dock_icon(ui, icon, self.surface == surf)
                            .on_hover_text(tip)
                            .clicked()
                        {
                            self.surface = surf;
                        }
                        ui.add_space(4.0);
                    }
                    ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                        ui.add_space(2.0);
                        if dock_icon(ui, theme::ICON_SETTINGS, self.settings_open)
                            .on_hover_text("Settings")
                            .clicked()
                        {
                            self.settings_open = !self.settings_open;
                        }
                        ui.add_space(4.0);
                        if dock_icon(ui, theme::ICON_ROBOT, false)
                            .on_hover_text("AI · ⌘K")
                            .clicked()
                        {
                            self.palette_open = true;
                        }
                    });
                });
            });
    }
}

/// One dock icon button; active is teal with a faint fill.
fn dock_icon(ui: &mut egui::Ui, icon: char, active: bool) -> egui::Response {
    let fg = if active { theme::TEAL } else { theme::FAINT };
    let fill = if active {
        theme::BG_BAR
    } else {
        theme::BG_DOCK
    };
    let resp = egui::Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(6, 6))
        .show(ui, |ui| {
            ui.set_width(28.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(icon).font(theme::icon_font(18.0)).color(fg));
            });
        })
        .response
        .interact(egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect,
            CornerRadius::same(5),
            Stroke::new(1.0, theme::TEAL),
            egui::StrokeKind::Inside,
        );
    }
    resp
}

// ── Sidebar ───────────────────────────────────────────────────────────────────────

impl EnzoApp {
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(210.0)
            .width_range(168.0..=360.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_SIDE)
                    .inner_margin(Margin::same(8))
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| match self.surface {
                Surface::Terminal => self.sidebar_terminal(ui),
                Surface::Ide => self.sidebar_ide(ui),
                Surface::Database => self.sidebar_db(ui),
                Surface::Browser => self.sidebar_browser(ui),
            });
    }

    fn sidebar_terminal(&mut self, ui: &mut egui::Ui) {
        pixel_header(ui, "SESSIONS");
        let active = self.ui.active_index();
        let tabs: Vec<String> = self.ui.tabs().iter().map(|t| t.title.clone()).collect();
        let mut switch = None;
        for (i, title) in tabs.iter().enumerate() {
            if tree_row(ui, &format!("❯ {title}"), 0, i == active, theme::FG1).clicked() {
                switch = Some(i);
            }
        }
        if let Some(i) = switch {
            self.set_active_tab(i);
        }
        ui.add_space(6.0);
        if ui
            .add(
                egui::Button::new(
                    RichText::new("+ new session")
                        .font(theme::pixel(8.0))
                        .color(theme::TEAL),
                )
                .fill(theme::BG_CARD),
            )
            .clicked()
        {
            self.spawn_terminal();
        }
        ui.add_space(8.0);
        pixel_header(ui, "QUICK");
        let _ = tree_row_icon(
            ui,
            Some(theme::ICON_GIT_BRANCH),
            "main ✓",
            0,
            false,
            theme::MUTED,
        );
        let _ = tree_row_icon(
            ui,
            Some(theme::ICON_FOLDER),
            "~/github/enzo",
            0,
            false,
            theme::MUTED,
        );
    }

    fn sidebar_ide(&mut self, ui: &mut egui::Ui) {
        pixel_header(ui, "EXPLORER");
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let rows: Vec<(usize, String, bool, usize, String)> = self
                    .ide
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (i, e.name.clone(), e.is_dir, e.depth, e.path.clone()))
                    .collect();
                let mut activate = None;
                for (i, name, is_dir, depth, path) in rows {
                    let selected = i == self.ide.selected;
                    let icon = if is_dir {
                        Some(if self.ide.is_expanded(&path) {
                            theme::ICON_CHEVRON_DOWN
                        } else {
                            theme::ICON_CHEVRON_RIGHT
                        })
                    } else {
                        None
                    };
                    let label = if is_dir { name } else { format!("  {name}") };
                    if tree_row_icon(ui, icon, &label, depth, selected, theme::FG1).clicked() {
                        activate = Some(i);
                    }
                }
                if let Some(i) = activate {
                    self.ide.activate(i);
                }
            });
    }

    fn sidebar_db(&mut self, ui: &mut egui::Ui) {
        pixel_header(ui, "CONNECTIONS");
        let names: Vec<String> = self.db.connections.iter().map(|c| c.name.clone()).collect();
        let mut select = None;
        for (i, name) in names.iter().enumerate() {
            let sel = i == self.db.active_conn_idx;
            let (icon, color) = if sel {
                (theme::ICON_PLUG_CONNECTED, theme::TEAL)
            } else {
                (theme::ICON_PLUG, theme::MUTED)
            };
            if tree_row_icon(ui, Some(icon), name, 0, sel, color).clicked() {
                select = Some(i);
            }
        }
        if let Some(i) = select {
            self.db.active_conn_idx = i;
        }
        if ui
            .add(
                egui::Button::new(
                    RichText::new("+ add connection")
                        .font(theme::pixel(8.0))
                        .color(theme::TEAL),
                )
                .fill(theme::BG_CARD),
            )
            .clicked()
        {
            self.db.dialog_open = true;
            self.db.dialog_path.clear();
        }
        ui.add_space(8.0);
        pixel_header(ui, "SCHEMA");
        let tables: Vec<String> = self
            .db
            .active_tables()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        if tables.is_empty() {
            ui.label(
                RichText::new("no tables")
                    .font(theme::pixel(8.0))
                    .color(theme::FAINT),
            );
        }
        let browsing = self.db.browsing.clone();
        let mut browse = None;
        for t in tables {
            let sel = browsing.as_deref() == Some(t.as_str());
            let color = if sel { theme::TEAL } else { theme::BLUE };
            if tree_row_icon(ui, Some(theme::ICON_TABLE), &t, 1, sel, color).clicked() {
                browse = Some(t);
            }
        }
        if let Some(t) = browse {
            self.browse_table(&t, 0);
        }
    }

    fn sidebar_browser(&mut self, ui: &mut egui::Ui) {
        pixel_header(ui, "DEVTOOLS");
        for (panel, label) in [
            (BrowserPanel::Page, "▣ Page"),
            (BrowserPanel::Network, "⇅ Network"),
            (BrowserPanel::Console, "≫ Console"),
        ] {
            if tree_row(ui, label, 0, self.browser.panel == panel, theme::FG1).clicked() {
                self.browser.panel = panel;
            }
        }
    }
}

/// An indented, selectable sidebar/tree row.
fn tree_row(
    ui: &mut egui::Ui,
    label: &str,
    depth: usize,
    selected: bool,
    color: egui::Color32,
) -> egui::Response {
    tree_row_icon(ui, None, label, depth, selected, color)
}

/// An indented, selectable sidebar/tree row with an optional Tabler icon glyph
/// (rendered in the icon font) before the body label.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "tree depth is tiny; indent is clamped to 120"
)]
fn tree_row_icon(
    ui: &mut egui::Ui,
    icon: Option<char>,
    label: &str,
    depth: usize,
    selected: bool,
    color: egui::Color32,
) -> egui::Response {
    let fill = if selected {
        theme::PURPLE_BG
    } else {
        egui::Color32::TRANSPARENT
    };
    let fg = if selected { theme::PURPLE_FG } else { color };
    let indent = 6.0 + depth as f32 * 14.0;
    let indent_i = (6 + depth as i32 * 14).min(120) as i8;
    let job = tree_row_job(icon, label, fg);
    let resp = egui::Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin {
            left: indent_i,
            right: 6,
            top: 3,
            bottom: 3,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(job);
        })
        .response
        .interact(egui::Sense::click());
    if resp.hovered() && !selected {
        ui.painter()
            .rect_filled(resp.rect, CornerRadius::same(3), theme::BG_CARD);
        let galley = ui.fonts(|f| f.layout_job(tree_row_job(icon, label, color)));
        ui.painter().galley(
            resp.rect.left_center() + Vec2::new(indent, -galley.size().y / 2.0),
            galley,
            color,
        );
    }
    resp
}

/// Build a `LayoutJob` for a tree row: an optional icon glyph (icon font) plus
/// the body label (monospace), sharing the same colour.
fn tree_row_job(icon: Option<char>, label: &str, color: egui::Color32) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    if let Some(ch) = icon {
        job.append(
            &format!("{ch} "),
            0.0,
            TextFormat {
                font_id: theme::icon_font(12.0),
                color,
                valign: Align::Center,
                ..Default::default()
            },
        );
    }
    job.append(
        label,
        0.0,
        TextFormat {
            font_id: FontId::new(12.5, egui::FontFamily::Monospace),
            color,
            valign: Align::Center,
            ..Default::default()
        },
    );
    job
}

/// The DB "RUN" button label: a Tabler play glyph (icon font) before the pixel
/// "RUN ⌘↵" text, both in the dark-on-green colour from the mockup.
fn run_button_label() -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let fg = egui::Color32::from_rgb(0x17, 0x34, 0x04);
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    job.append(
        &format!("{} ", theme::ICON_PLAYER_PLAY),
        0.0,
        TextFormat {
            font_id: theme::icon_font(10.0),
            color: fg,
            valign: Align::Center,
            ..Default::default()
        },
    );
    job.append(
        "RUN ⌘↵",
        0.0,
        TextFormat {
            font_id: theme::pixel(8.0),
            color: fg,
            valign: Align::Center,
            ..Default::default()
        },
    );
    job
}

// ── Status bar ───────────────────────────────────────────────────────────────────

impl EnzoApp {
    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(bar_frame(theme::BG_BAR))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let (dot, left) = match self.surface {
                        Surface::Terminal => {
                            let s = self.active_terminal_idx().map_or_else(
                                || "no session".to_owned(),
                                |i| {
                                    let t = &self.terminals[i].1;
                                    format!("PTY zsh   {}×{}", t.cols(), t.rows())
                                },
                            );
                            (theme::TEAL, s)
                        }
                        Surface::Ide => (
                            theme::TEAL,
                            format!(
                                "{}   {}",
                                self.ide.open_path.as_deref().unwrap_or("no file"),
                                self.ide.language
                            ),
                        ),
                        Surface::Database => {
                            (theme::TEAL, format!("ADBC · {}", self.db.active_conn()))
                        }
                        Surface::Browser => (theme::AMBER, self.browser.url.clone()),
                    };
                    ui.label(RichText::new("●").color(dot).size(9.0));
                    ui.label(
                        RichText::new(left)
                            .font(theme::pixel(8.0))
                            .color(theme::FG1),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("120 FPS · ⌘K")
                                .font(theme::pixel(8.0))
                                .color(theme::FAINT),
                        );
                    });
                });
            });
    }

    // ── Central ──────────────────────────────────────────────────────────────────

    fn central(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_SURFACE))
            .show(ctx, |ui| match self.surface {
                Surface::Terminal => self.central_terminal(ui),
                Surface::Ide => self.central_ide(ui),
                Surface::Database => self.central_db(ui),
                Surface::Browser => self.central_browser(ui),
            });
    }

    fn central_terminal(&mut self, ui: &mut egui::Ui) {
        if let Some(idx) = self.active_terminal_idx() {
            let id = self.terminals[idx].0.clone();
            let fit = terminal_view::show(ui, &self.terminals[idx].1);
            let t = &self.terminals[idx].1;
            if fit.cols > 1 && fit.rows > 1 && (fit.cols != t.cols() || fit.rows != t.rows()) {
                let _ = self.cmd_tx.send(UiCommand::Resize {
                    id,
                    cols: fit.cols,
                    rows: fit.rows,
                });
            }
        } else {
            ui.centered_and_justified(|ui| {
                if ui
                    .button(RichText::new("Start a session  ( ⌘T )").size(14.0))
                    .clicked()
                {
                    self.spawn_terminal();
                }
            });
        }
    }

    fn central_ide(&mut self, ui: &mut egui::Ui) {
        if self.ide.open_path.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("⌖ select a file in the explorer")
                        .color(theme::FAINT)
                        .size(14.0),
                );
            });
            return;
        }
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                let lines = self.ide.lines.clone();
                for (i, line) in lines.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!("{:>4}", i + 1))
                                .color(theme::FAINT)
                                .monospace(),
                        );
                        ui.add_space(10.0);
                        syntax_line(ui, line, &self.ide.language);
                    });
                }
            });
    }

    #[allow(clippy::too_many_lines, reason = "one cohesive surface layout")]
    fn central_db(&mut self, ui: &mut egui::Ui) {
        // ⌘↵ runs the active query (when no modal/text field has focus elsewhere).
        let run_shortcut = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
        let mut run = false;

        // RUN bar.
        egui::Frame::new()
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let has_conn = self.db.active_conn_id().is_some();
                    if ui
                        .add_enabled(
                            has_conn && !self.db.running,
                            egui::Button::new(run_button_label()).fill(theme::GREEN),
                        )
                        .clicked()
                    {
                        run = true;
                    }
                    if self.db.running {
                        ui.label(
                            RichText::new("running…")
                                .font(theme::pixel(8.0))
                                .color(theme::AMBER),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!("{} rows", self.db.rows.len()))
                                .font(theme::pixel(8.0))
                                .color(theme::FG1),
                        );
                        if let Some(ms) = self.db.query_ms {
                            ui.label(
                                RichText::new(format!("· {ms}ms · Arrow stream"))
                                    .font(theme::pixel(8.0))
                                    .color(theme::FAINT),
                            );
                        }
                    }
                    // Pager for browsed tables (right-aligned).
                    self.db_pager(ui);
                });
            });

        // SQL editor.
        egui::Frame::new()
            .fill(theme::BG_PAGE)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(Margin::same(8))
            .outer_margin(Margin::symmetric(10, 0))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(self.db.active_sql_mut())
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .frame(false),
                );
            });
        ui.add_space(8.0);

        if run || run_shortcut {
            self.run_active_query();
        }

        // Error banner (real daemon error, rendered red).
        if let Some(err) = self.db.error.clone() {
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(0x2a, 0x14, 0x16))
                .stroke(Stroke::new(1.0, theme::RED))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::same(8))
                .outer_margin(Margin::symmetric(10, 0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("✗ {err}"))
                            .color(theme::RED_LT)
                            .monospace()
                            .size(12.0),
                    );
                });
            return;
        }

        // Result grid with pixel headers + alternating rows.
        if self.db.columns.is_empty() {
            return;
        }
        let cols = self.db.columns.clone();
        let rows = self.db.rows.clone();
        egui::Frame::new()
            .inner_margin(Margin::symmetric(10, 0))
            .show(ui, |ui| {
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .columns(Column::remainder().resizable(true), cols.len())
                    .header(22.0, |mut header| {
                        for c in &cols {
                            header.col(|ui| {
                                ui.label(
                                    RichText::new(c.to_uppercase())
                                        .font(theme::pixel(8.0))
                                        .color(theme::PURPLE),
                                );
                            });
                        }
                    })
                    .body(|mut body| {
                        for r in &rows {
                            body.row(20.0, |mut row| {
                                for (ci, v) in r.iter().enumerate() {
                                    row.col(|ui| {
                                        let color = if ci == 0 { theme::TEAL } else { theme::FG2 };
                                        ui.label(RichText::new(v).color(color).size(12.0));
                                    });
                                }
                            });
                        }
                    });
            });
    }

    /// Prev/next pager shown while browsing a table; pages via `db.table.browse`.
    fn db_pager(&mut self, ui: &mut egui::Ui) {
        let (Some(table), Some(total)) = (self.db.browsing.clone(), self.db.total_rows) else {
            return;
        };
        let pages = total.div_ceil(DB_PAGE_SIZE).max(1);
        let page = self.db.page;
        let mut goto: Option<u64> = None;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(
                    page + 1 < pages,
                    egui::Button::new(RichText::new("›").size(13.0)),
                )
                .clicked()
            {
                goto = Some(page + 1);
            }
            ui.label(
                RichText::new(format!("page {}/{}  ·  {total} rows", page + 1, pages))
                    .font(theme::pixel(8.0))
                    .color(theme::FAINT),
            );
            if ui
                .add_enabled(page > 0, egui::Button::new(RichText::new("‹").size(13.0)))
                .clicked()
            {
                goto = Some(page - 1);
            }
        });
        if let Some(p) = goto {
            self.browse_table(&table, p);
        }
    }

    #[allow(clippy::too_many_lines, reason = "one cohesive surface layout")]
    fn central_browser(&mut self, ui: &mut egui::Ui) {
        let mut go = false;
        let mut go_external = false;
        egui::Frame::new()
            .fill(theme::BG_BAR)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(Margin::symmetric(8, 5))
            .outer_margin(Margin::same(8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("◍").color(theme::TEAL));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.browser.url)
                            .desired_width(ui.available_width() - 120.0)
                            .frame(false),
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        go = true;
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("Go").size(12.0)).fill(theme::BG_CARD))
                        .clicked()
                    {
                        go = true;
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("↗").size(12.0)).fill(theme::BG_CARD))
                        .on_hover_text("Open in system browser")
                        .clicked()
                    {
                        go_external = true;
                    }
                });
            });
        if go_external {
            open_url(&self.browser.url);
        }
        if go {
            let url = normalize_url(&self.browser.url);
            self.browser.url.clone_from(&url);
            if self.browser_launched {
                let _ = self.cmd_tx.send(UiCommand::BrowserNavigate {
                    id: BROWSER_ID.into(),
                    url,
                });
            }
            self.browser.panel = BrowserPanel::Page;
        }

        match self.browser.panel {
            BrowserPanel::Page => self.browser_page(ui),
            BrowserPanel::Network => {
                let reqs = self.browser.requests.clone();
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(8, 0))
                    .show(ui, |ui| {
                        TableBuilder::new(ui)
                            .striped(true)
                            .cell_layout(Layout::left_to_right(Align::Center))
                            .column(Column::exact(64.0))
                            .column(Column::exact(60.0))
                            .column(Column::exact(72.0))
                            .column(Column::remainder())
                            .header(22.0, |mut h| {
                                for c in ["METHOD", "STATUS", "TIME", "PATH"] {
                                    h.col(|ui| {
                                        ui.label(
                                            RichText::new(c)
                                                .font(theme::pixel(8.0))
                                                .color(theme::PURPLE),
                                        );
                                    });
                                }
                            })
                            .body(|mut body| {
                                for req in &reqs {
                                    body.row(20.0, |mut row| {
                                        row.col(|ui| {
                                            ui.label(
                                                RichText::new(&req.method)
                                                    .color(theme::FG1)
                                                    .size(12.0),
                                            );
                                        });
                                        let sc = if req.status >= 400 {
                                            theme::RED
                                        } else {
                                            theme::GREEN_LT
                                        };
                                        row.col(|ui| {
                                            ui.label(
                                                RichText::new(req.status.to_string())
                                                    .color(sc)
                                                    .size(12.0),
                                            );
                                        });
                                        row.col(|ui| {
                                            ui.label(
                                                RichText::new(format!("{} ms", req.ms))
                                                    .color(theme::FAINT)
                                                    .size(12.0),
                                            );
                                        });
                                        row.col(|ui| {
                                            ui.label(
                                                RichText::new(&req.path)
                                                    .color(theme::FG2)
                                                    .size(12.0),
                                            );
                                        });
                                    });
                                }
                            });
                    });
            }
            BrowserPanel::Console => {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(4.0);
                        for line in &self.browser.console_lines {
                            let color = if line.starts_with("[ERR]") {
                                theme::RED_LT
                            } else if line.starts_with("[WARN]") {
                                theme::AMBER
                            } else {
                                theme::FG1
                            };
                            ui.label(RichText::new(line).color(color).monospace().size(12.0));
                        }
                    });
            }
        }
    }

    /// The live page surface: launch a headless browser, stream screenshots into
    /// a texture, and forward mouse/scroll to the page over CDP.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn browser_page(&mut self, ui: &mut egui::Ui) {
        // Kick off the headless browser on first view.
        if !self.browser_launched {
            if !self.browser_pending {
                self.browser_pending = true;
                let _ = self.cmd_tx.send(UiCommand::BrowserOpen {
                    id: BROWSER_ID.into(),
                    url: normalize_url(&self.browser.url),
                    w: BROWSER_W,
                    h: BROWSER_H,
                });
            }
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(
                        "◍ starting headless browser…\n(needs Chrome/Chromium installed)",
                    )
                    .color(theme::FAINT)
                    .size(13.0),
                );
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }

        // Draw the latest frame, scaled to fit while preserving aspect.
        if let Some(tex) = self.browser_tex.clone() {
            let avail = ui.available_size();
            let (bw, bh) = (self.browser_size.0 as f32, self.browser_size.1 as f32);
            let scale = (avail.x / bw).min(avail.y / bh);
            let draw = egui::vec2(bw * scale, bh * scale);
            let resp = ui.add(
                egui::Image::new(&tex)
                    .fit_to_exact_size(draw)
                    .sense(egui::Sense::click_and_drag()),
            );

            let to_page = |pos: egui::Pos2| {
                let rel = pos - resp.rect.min;
                let x = (rel.x / draw.x * bw).clamp(0.0, bw);
                let y = (rel.y / draw.y * bh).clamp(0.0, bh);
                (f64::from(x), f64::from(y))
            };

            if resp.clicked()
                && let Some(p) = resp.interact_pointer_pos()
            {
                let (x, y) = to_page(p);
                self.browser_mouse("mousePressed", x, y, 1, 0.0, 0.0);
                self.browser_mouse("mouseReleased", x, y, 1, 0.0, 0.0);
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta);
                if scroll.y.abs() > 0.5
                    && let Some(p) = resp.hover_pos()
                {
                    let (x, y) = to_page(p);
                    self.browser_mouse("mouseWheel", x, y, 0, 0.0, f64::from(-scroll.y));
                }
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("loading page…")
                        .color(theme::FAINT)
                        .size(13.0),
                );
            });
        }

        // Request the next frame (throttled).
        if !self.browser_pending {
            self.browser_pending = true;
            let _ = self.cmd_tx.send(UiCommand::BrowserShot {
                id: BROWSER_ID.into(),
            });
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(120));
    }

    /// Send a CDP `Input.dispatchMouseEvent` to the browser page.
    fn browser_mouse(&self, kind: &str, x: f64, y: f64, clicks: u32, dx: f64, dy: f64) {
        let params = serde_json::json!({
            "type": kind,
            "x": x,
            "y": y,
            "button": if clicks > 0 { "left" } else { "none" },
            "clickCount": clicks,
            "deltaX": dx,
            "deltaY": dy,
        });
        let _ = self.cmd_tx.send(UiCommand::BrowserInput {
            id: BROWSER_ID.into(),
            method: "Input.dispatchMouseEvent".into(),
            params,
        });
    }
}

/// Normalise a typed address into a URL (prepend `https://` if no scheme).
fn normalize_url(url: &str) -> String {
    let u = url.trim();
    if u.contains("://") || u.is_empty() {
        u.to_owned()
    } else {
        format!("https://{u}")
    }
}

/// Very small keyword/string syntax tinting for the IDE viewer.
fn syntax_line(ui: &mut egui::Ui, line: &str, lang: &str) {
    use egui::text::LayoutJob;
    let mut job = LayoutJob::default();
    let kw: &[&str] = match lang {
        "rust" => &[
            "fn", "let", "mut", "pub", "use", "impl", "struct", "enum", "match", "if", "else",
            "for", "while", "return", "self", "Self", "mod", "async", "await", "const", "trait",
            "where",
        ],
        "python" => &[
            "def", "class", "return", "import", "from", "if", "else", "elif", "for", "while",
            "with", "as", "lambda", "yield", "try", "except",
        ],
        _ => &[
            "function", "const", "let", "var", "return", "if", "else", "for", "while", "class",
            "import", "export",
        ],
    };
    let fmt = |c: egui::Color32| egui::text::TextFormat {
        font_id: FontId::new(13.0, egui::FontFamily::Monospace),
        color: c,
        ..Default::default()
    };
    let mut in_str = false;
    for tok in line.split_inclusive([' ', '(', ')', ';', ',']) {
        let trimmed = tok.trim_end_matches([' ', '(', ')', ';', ',']);
        let color = if tok.contains('"') || in_str {
            in_str = tok.matches('"').count() % 2 == 1;
            theme::GREEN_LT
        } else if kw.contains(&trimmed) {
            theme::KEYWORD
        } else if trimmed.starts_with("//") || trimmed.starts_with('#') {
            theme::FAINT
        } else {
            theme::FG0
        };
        job.append(tok, 0.0, fmt(color));
    }
    ui.label(job);
}

// ── Overlay (agent prompt card) ────────────────────────────────────────────────────

impl EnzoApp {
    #[allow(clippy::too_many_lines, reason = "one cohesive overlay layout")]
    fn draw_overlay(&mut self, ctx: &egui::Context) {
        let Some(card) = self.overlay.prompt.clone() else {
            return;
        };
        let mut action: Option<&'static str> = None;

        egui::Area::new("agent_dim".into())
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let screen = ctx.screen_rect();
                ui.painter().rect_filled(
                    screen,
                    CornerRadius::ZERO,
                    egui::Color32::from_black_alpha(120),
                );
            });

        egui::Window::new("agent")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 90.0))
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_SURFACE)
                    .stroke(Stroke::new(2.0, theme::PURPLE_BG))
                    .corner_radius(CornerRadius::same(8)),
            )
            .fixed_size([560.0, 0.0])
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::BG_BAR)
                    .inner_margin(Margin::symmetric(12, 9))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            badge(ui, "AI", theme::PURPLE_FG, theme::PURPLE_BG);
                            ui.add_space(2.0);
                            ui.label(RichText::new(&card.title).color(theme::FG0).strong());
                        });
                    });
                egui::Frame::new()
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        if let Some(path) = &card.diff_path {
                            ui.label(
                                RichText::new(format!("± {path}"))
                                    .color(theme::TEAL)
                                    .size(12.5),
                            );
                            ui.add_space(6.0);
                            egui::Frame::new()
                                .fill(theme::BG_PAGE)
                                .stroke(Stroke::new(1.0, theme::BORDER))
                                .corner_radius(CornerRadius::same(5))
                                .inner_margin(Margin::same(8))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    for dl in card.diff_lines.iter().take(18) {
                                        let (mark, color, bg) = match dl.kind {
                                            DiffLineKind::Add => (
                                                "+",
                                                theme::GREEN_LT,
                                                Some(egui::Color32::from_rgb(0x12, 0x24, 0x14)),
                                            ),
                                            DiffLineKind::Remove => (
                                                "-",
                                                theme::RED_LT,
                                                Some(egui::Color32::from_rgb(0x2a, 0x14, 0x16)),
                                            ),
                                            DiffLineKind::Header => ("@", theme::PURPLE_LT, None),
                                            DiffLineKind::Context => (" ", theme::FAINT, None),
                                        };
                                        let txt = RichText::new(format!("{mark} {}", dl.text))
                                            .color(color)
                                            .monospace()
                                            .size(12.0);
                                        if let Some(bg) = bg {
                                            egui::Frame::new()
                                                .fill(bg)
                                                .inner_margin(Margin::symmetric(2, 0))
                                                .show(ui, |ui| {
                                                    ui.label(txt);
                                                });
                                        } else {
                                            ui.label(txt);
                                        }
                                    }
                                });
                        } else {
                            ui.label(
                                RichText::new(&card.body)
                                    .color(theme::FG2)
                                    .monospace()
                                    .size(12.5),
                            );
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if action_button(ui, "A · ACCEPT", theme::GREEN_LT).clicked() {
                                action = Some("accept");
                            }
                            if action_button(ui, "R · REJECT", theme::RED).clicked() {
                                action = Some("reject");
                            }
                            if action_button(ui, "E · EDIT", theme::AMBER).clicked() {
                                action = Some("edit");
                            }
                        });
                    });
            });

        ctx.input(|i| {
            if i.key_pressed(egui::Key::A) {
                action = Some("accept");
            } else if i.key_pressed(egui::Key::R) {
                action = Some("reject");
            } else if i.key_pressed(egui::Key::E) {
                action = Some("edit");
            }
        });
        if let Some(a) = action {
            self.respond_prompt(a);
        }
    }

    // ── Settings overlay ──────────────────────────────────────────────────────────

    fn draw_settings(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_SURFACE)
                    .stroke(Stroke::new(2.0, theme::TEAL))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(14)),
            )
            .fixed_size([480.0, 0.0])
            .show(ctx, |ui| {
                pixel_header(ui, "THEME");
                for (i, name) in THEMES.iter().enumerate() {
                    if ui
                        .add_sized(
                            [ui.available_width(), 26.0],
                            egui::SelectableLabel::new(
                                i == self.active_theme,
                                RichText::new(*name).size(13.0),
                            ),
                        )
                        .clicked()
                    {
                        self.active_theme = i;
                    }
                }
                ui.add_space(10.0);
                pixel_header(ui, "EFFECTS");
                ui.label(
                    RichText::new("scanlines · phosphor · CRT — coming with the theme engine")
                        .color(theme::FAINT)
                        .size(12.0),
                );
                ui.add_space(10.0);
                pixel_header(ui, "ABOUT");
                ui.label(
                    RichText::new("enzo v0.2 · AI-native developer workspace")
                        .color(theme::FG1)
                        .size(12.0),
                );
            });
        if !open {
            self.settings_open = false;
        }
    }

    // ── Add-connection dialog ──────────────────────────────────────────────────────

    /// Modal to open a new database connection by file path / connection string.
    fn draw_db_dialog(&mut self, ctx: &egui::Context) {
        if !self.db.dialog_open {
            return;
        }
        let mut open = true;
        let mut connect = false;
        egui::Window::new("Add connection")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_SURFACE)
                    .stroke(Stroke::new(2.0, theme::TEAL))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(14)),
            )
            .fixed_size([440.0, 0.0])
            .show(ctx, |ui| {
                pixel_header(ui, "SQLITE DATABASE");
                ui.label(
                    RichText::new("File path or :memory:")
                        .color(theme::FAINT)
                        .size(12.0),
                );
                ui.add_space(4.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.db.dialog_path)
                        .hint_text("/path/to/database.db")
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                resp.request_focus();
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    connect = true;
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Connect").color(theme::TEAL).strong())
                                .fill(theme::BG_CARD)
                                .stroke(Stroke::new(1.0, theme::TEAL)),
                        )
                        .clicked()
                    {
                        connect = true;
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("Cancel")).fill(theme::BG_CARD))
                        .clicked()
                    {
                        self.db.dialog_open = false;
                    }
                });
            });
        if !open {
            self.db.dialog_open = false;
        }
        if connect {
            self.connect_from_dialog();
        }
    }

    // ── Command palette ────────────────────────────────────────────────────────────

    fn draw_palette(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }
        let mut chosen: Option<PaletteAction> = None;
        egui::Window::new("palette")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 120.0))
            .fixed_size([580.0, 0.0])
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_SURFACE)
                    .stroke(Stroke::new(2.0, theme::TEAL))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⌘K").color(theme::TEAL).size(14.0));
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.palette_query)
                            .hint_text("Search commands, files, surfaces…")
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Heading)
                            .frame(false),
                    );
                    resp.request_focus();
                });
                ui.add_space(8.0);
                ui.separator();
                let q = self.palette_query.to_lowercase();
                for act in PALETTE_ACTIONS {
                    if !q.is_empty() && !act.label.to_lowercase().contains(&q) {
                        continue;
                    }
                    if ui
                        .add_sized(
                            [ui.available_width(), 26.0],
                            egui::SelectableLabel::new(false, RichText::new(act.label).size(13.5)),
                        )
                        .clicked()
                    {
                        chosen = Some(act.action);
                    }
                }
            });
        if let Some(action) = chosen {
            self.run_palette_action(action);
            self.palette_open = false;
        }
    }

    fn run_palette_action(&mut self, action: PaletteAction) {
        match action {
            PaletteAction::Surface(s) => self.surface = s,
            PaletteAction::NewTerminal => self.spawn_terminal(),
            PaletteAction::ToggleSidebar => self.sidebar_open = !self.sidebar_open,
            PaletteAction::Settings => self.settings_open = true,
            PaletteAction::DemoPrompt => self.show_demo_prompt(),
        }
    }

    /// Inject a sample agent decision card so the approval UI can be seen and
    /// exercised without a live agent (the buttons still emit `prompt.respond`).
    fn show_demo_prompt(&mut self) {
        let diff = serde_json::json!({
            "path": "src/renderer.rs",
            "raw": "@@ -10,3 +10,3 @@ impl Renderer {\n     fn present(&mut self) {\n-        self.redraw_all();\n+        let dirty = self.damage.take();\n+        self.gpu.draw(dirty);\n         self.swap_buffers();\n     }",
        });
        self.overlay.set_prompt(PromptCard::new(
            "demo-1".to_owned(),
            "claude wants to edit renderer.rs".to_owned(),
            "Replace full redraw with damage-tracked draw".to_owned(),
            Some(diff),
            vec!["accept".to_owned(), "reject".to_owned(), "edit".to_owned()],
        ));
    }
}

/// A bordered action button for the agent card.
fn action_button(ui: &mut egui::Ui, label: &str, fg: egui::Color32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(fg).strong().size(12.5))
            .fill(theme::BG_CARD)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .min_size(Vec2::new(118.0, 30.0)),
    )
}

// ── Command palette catalogue ────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum PaletteAction {
    Surface(Surface),
    NewTerminal,
    ToggleSidebar,
    Settings,
    DemoPrompt,
}

struct PaletteItem {
    label: &'static str,
    action: PaletteAction,
}

const PALETTE_ACTIONS: &[PaletteItem] = &[
    PaletteItem {
        label: "Go to Terminal",
        action: PaletteAction::Surface(Surface::Terminal),
    },
    PaletteItem {
        label: "Go to Editor (IDE)",
        action: PaletteAction::Surface(Surface::Ide),
    },
    PaletteItem {
        label: "Go to Database",
        action: PaletteAction::Surface(Surface::Database),
    },
    PaletteItem {
        label: "Go to Browser",
        action: PaletteAction::Surface(Surface::Browser),
    },
    PaletteItem {
        label: "New terminal session",
        action: PaletteAction::NewTerminal,
    },
    PaletteItem {
        label: "Toggle sidebar",
        action: PaletteAction::ToggleSidebar,
    },
    PaletteItem {
        label: "Open settings",
        action: PaletteAction::Settings,
    },
    PaletteItem {
        label: "Demo: agent decision card",
        action: PaletteAction::DemoPrompt,
    },
];

// ── ATP background task ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines, reason = "linear command dispatch table")]
async fn run_atp(
    sock: String,
    ctx: egui::Context,
    tx: Sender<Incoming>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<UiCommand>,
) {
    let tx2 = tx.clone();
    let ctx2 = ctx.clone();
    let client = match AtpClient::connect(&sock, move |msg| {
        let _ = tx2.send(Incoming::Message(msg));
        ctx2.request_repaint();
    })
    .await
    {
        Ok(c) => c,
        Err(e) => {
            log::error!("ATP connect failed: {e:#}");
            return;
        }
    };
    let _ = client.register_display().await;
    let _ = tx.send(Incoming::Connected);
    ctx.request_repaint();

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            UiCommand::NewSession { id, cols, rows } => {
                if let Err(e) = client.spawn_session(&id, cols, rows).await {
                    log::error!("spawn_session {id}: {e:#}");
                }
            }
            UiCommand::CloseSession { id } => {
                let _ = client.close_session(&id).await;
            }
            UiCommand::Input { id, data } => {
                let _ = client.send_input(&id, &data).await;
            }
            UiCommand::Resize { id, cols, rows } => {
                let _ = client.resize(&id, cols, rows).await;
            }
            UiCommand::PromptRespond { id, action } => {
                let _ = client.respond_prompt(&id, &action).await;
            }
            UiCommand::BrowserOpen { id, url, w, h } => {
                match client.browser_launch(&id, &url, w, h).await {
                    Ok(()) => {
                        let _ = tx.send(Incoming::BrowserReady);
                        ctx.request_repaint();
                    }
                    Err(e) => log::error!("browser.launch: {e:#}"),
                }
            }
            UiCommand::BrowserNavigate { id, url } => {
                let _ = client.browser_navigate(&id, &url).await;
            }
            UiCommand::BrowserShot { id } => {
                if let Ok(png) = client.browser_screenshot(&id).await
                    && let Some(img) = decode_png(&png)
                {
                    let _ = tx.send(Incoming::BrowserFrame(img));
                    ctx.request_repaint();
                }
            }
            UiCommand::BrowserInput { id, method, params } => {
                let _ = client.browser_input(&id, &method, params).await;
            }
            UiCommand::DbConnect { conn, path, seed } => {
                db_connect_task(&client, &tx, &ctx, &conn, &path, seed).await;
            }
            UiCommand::DbQuery { conn, sql } => {
                let started = std::time::Instant::now();
                let incoming = match client.db_query(&conn, &sql).await {
                    Ok((columns, rows)) => Incoming::DbResult {
                        columns,
                        rows,
                        ms: elapsed_ms(started),
                        total: None,
                        page: 0,
                        browsing: None,
                    },
                    Err(e) => Incoming::DbError {
                        message: atp_error_message(&e),
                    },
                };
                let _ = tx.send(incoming);
                ctx.request_repaint();
            }
            UiCommand::DbBrowseTable {
                conn,
                table,
                page,
                size,
            } => {
                let started = std::time::Instant::now();
                let incoming = match client.db_table_browse(&conn, &table, page, size).await {
                    Ok((columns, rows, total)) => Incoming::DbResult {
                        columns,
                        rows,
                        ms: elapsed_ms(started),
                        total: Some(total),
                        page,
                        browsing: Some(table),
                    },
                    Err(e) => Incoming::DbError {
                        message: atp_error_message(&e),
                    },
                };
                let _ = tx.send(incoming);
                ctx.request_repaint();
            }
        }
    }
}

/// Connect a DB pool, optionally seed a demo schema, then report driver + tables.
async fn db_connect_task(
    client: &AtpClient,
    tx: &Sender<Incoming>,
    ctx: &egui::Context,
    conn: &str,
    path: &str,
    seed: bool,
) {
    match client.db_connect(conn, path).await {
        Ok(driver) => {
            if seed {
                seed_demo_db(client, conn).await;
            }
            let _ = tx.send(Incoming::DbConnected {
                conn: conn.to_owned(),
                driver,
            });
            if let Ok(tables) = client.db_schema_tables(conn).await {
                let tables = tables
                    .into_iter()
                    .map(|(name, kind)| crate::surface::TableInfo { name, kind })
                    .collect();
                let _ = tx.send(Incoming::DbTables {
                    conn: conn.to_owned(),
                    tables,
                });
            }
            ctx.request_repaint();
        }
        Err(e) => {
            let _ = tx.send(Incoming::DbError {
                message: atp_error_message(&e),
            });
            ctx.request_repaint();
        }
    }
}

/// Seed the first-run demo database with a couple of small, real tables.
/// Idempotent: uses `IF NOT EXISTS` + `INSERT OR IGNORE` so re-running is safe.
async fn seed_demo_db(client: &AtpClient, conn: &str) {
    const STMTS: &[&str] = &[
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT)",
        "INSERT OR IGNORE INTO users (id, name, email) VALUES \
         (1,'Alice','alice@example.com'),(2,'Bob','bob@example.com'),\
         (3,'Carol','carol@example.com'),(4,'Dave','dave@example.com')",
        "CREATE TABLE IF NOT EXISTS products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price REAL)",
        "INSERT OR IGNORE INTO products (id, name, price) VALUES \
         (1,'Keyboard',89.0),(2,'Mouse',39.5),(3,'Monitor',329.0)",
    ];
    for sql in STMTS {
        if let Err(e) = client.db_execute(conn, sql).await {
            log::warn!("seed demo db: {e:#}");
        }
    }
}

/// Milliseconds elapsed since `started`, saturating into `u64`.
fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Strip the `ATP error: ` envelope so the surface shows the bare SQL error.
fn atp_error_message(e: &anyhow::Error) -> String {
    let s = e.to_string();
    s.strip_prefix("ATP error: ")
        .unwrap_or(&s)
        .trim()
        .to_owned()
}

/// Decode PNG bytes into an egui `ColorImage` (off the UI thread).
fn decode_png(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        img.as_raw(),
    ))
}

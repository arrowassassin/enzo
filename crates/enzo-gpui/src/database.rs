//! Database surface — real, daemon-backed (HANDOFF Task 1). Connections, schema,
//! query results, errors and pagination all come over ATP; no demo data lives
//! here. Styled faithful to `design/mockups/database.html`.

use std::collections::{HashMap, HashSet};

use gpui::{
    Context, Entity, IntoElement, ParentElement, SharedString, Styled, div, prelude::*, px,
};
use gpui_component::input::{Input, InputState};

use crate::EnzoApp;
use crate::atp::{ColumnMeta, TableInfo};
use crate::text_input::TextInput;
use crate::theme;
use crate::widgets::{icon, pixel_header, text};

/// Default page size for table browsing.
pub const PAGE_SIZE: u64 = 100;

/// One query editor tab (Harlequin-style multi-buffer SQL workflow). Each tab
/// owns a real multi-line code editor (tree-sitter SQL highlighting + LSP-ready).
pub struct QueryTab {
    /// Stable client-side id.
    pub id: u32,
    /// User-facing title (`Query 1`, …).
    pub title: String,
    /// The live SQL code editor for this tab.
    pub editor: Entity<InputState>,
}

/// One executed statement in the query history.
#[derive(Clone)]
pub struct HistEntry {
    pub sql: String,
    pub ms: u64,
    pub rows: usize,
    pub ok: bool,
}

/// A live, daemon-backed connection mirrored on the client.
pub struct DbConn {
    /// ATP connection id (e.g. `"db-0"`).
    pub id: String,
    /// Sidebar display name.
    pub name: String,
    /// Driver reported by the daemon (e.g. `"sqlite"`).
    pub driver: String,
    /// Tables/views from `db.schema.tables`.
    pub tables: Vec<TableInfo>,
}

/// Database surface state. All result data is owned by the daemon and streamed
/// in over ATP — this only holds what the renderer needs.
pub struct DbState {
    pub connections: Vec<DbConn>,
    pub active: usize,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub error: Option<String>,
    pub query_ms: Option<u64>,
    pub running: bool,
    pub browsing: Option<String>,
    pub total: Option<u64>,
    pub page: u64,
    /// Primary-key column names for the browsed table (empty → not editable).
    pub pk_columns: Vec<String>,
    /// Cell `(row, col)` currently being edited, if any.
    pub editing: Option<(usize, usize)>,
    /// Catalog: expanded table names (showing their columns).
    pub expanded: HashSet<String>,
    /// Catalog: cached column schema per table (lazily fetched on expand).
    pub table_columns: HashMap<String, Vec<ColumnMeta>>,
    /// Transient status line after an export (path or error).
    pub export_msg: Option<String>,
}

impl DbState {
    /// Initial state with a single pending connection (the first-run demo db).
    pub fn new(demo_id: &str, demo_name: &str) -> Self {
        Self {
            connections: vec![DbConn {
                id: demo_id.to_owned(),
                name: demo_name.to_owned(),
                driver: String::new(),
                tables: Vec::new(),
            }],
            active: 0,
            columns: Vec::new(),
            rows: Vec::new(),
            error: None,
            query_ms: None,
            running: false,
            browsing: None,
            total: None,
            page: 0,
            pk_columns: Vec::new(),
            editing: None,
            expanded: HashSet::new(),
            table_columns: HashMap::new(),
            export_msg: None,
        }
    }

    /// Store the fetched column schema for `table`.
    pub fn set_table_columns(&mut self, table: String, columns: Vec<ColumnMeta>) {
        self.table_columns.insert(table, columns);
    }

    pub fn active_conn_id(&self) -> Option<&str> {
        self.connections.get(self.active).map(|c| c.id.as_str())
    }

    fn conn_mut(&mut self, id: &str) -> Option<&mut DbConn> {
        self.connections.iter_mut().find(|c| c.id == id)
    }

    pub fn set_driver(&mut self, id: &str, driver: String) {
        if let Some(c) = self.conn_mut(id) {
            c.driver = driver;
        }
    }

    pub fn set_tables(&mut self, id: &str, tables: Vec<TableInfo>) {
        if let Some(c) = self.conn_mut(id) {
            c.tables = tables;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_result(
        &mut self,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        ms: u64,
        total: Option<u64>,
        page: u64,
        browsing: Option<String>,
        pk_columns: Vec<String>,
    ) {
        self.columns = columns;
        self.rows = rows;
        self.query_ms = Some(ms);
        self.error = None;
        self.running = false;
        self.total = total;
        self.page = page;
        self.browsing = browsing;
        self.pk_columns = pk_columns;
        self.editing = None;
        self.export_msg = None;
    }

    /// Whether the current result set is an editable table view.
    pub fn editable(&self) -> bool {
        self.browsing.is_some() && !self.pk_columns.is_empty()
    }

    pub fn apply_error(&mut self, message: String) {
        self.error = Some(message);
        self.columns.clear();
        self.rows.clear();
        self.query_ms = None;
        self.running = false;
        self.browsing = None;
        self.total = None;
        self.editing = None;
    }

    /// Add a new (pending) connection and make it active.
    pub fn add_connection(&mut self, id: String, name: String) {
        self.connections.push(DbConn {
            id,
            name,
            driver: String::new(),
            tables: Vec::new(),
        });
        self.active = self.connections.len() - 1;
        self.columns.clear();
        self.rows.clear();
        self.error = None;
        self.browsing = None;
        self.total = None;
        self.expanded.clear();
        self.table_columns.clear();
    }
}

// ── Render (state-driven) ───────────────────────────────────────────────────

/// Connections + schema + history sidebar; clicking a table browses it.
pub fn sidebar(
    db: &DbState,
    history: &[HistEntry],
    cx: &mut Context<EnzoApp>,
) -> impl IntoElement {
    let new_conn = div()
        .id("db-new-conn")
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(5.0))
        .px(px(12.0))
        .pb(px(6.0))
        .text_size(px(8.0))
        .font_family(theme::FONT_PIXEL)
        .text_color(theme::PURPLE)
        .child("＋ NEW CONNECTION")
        .on_click(cx.listener(|this, _, window, cx| this.open_connection_dialog(window, cx)));
    let mut col = div()
        .flex()
        .flex_col()
        .child(pixel_header("CONNECTIONS"))
        .child(new_conn);
    for (i, conn) in db.connections.iter().enumerate() {
        let active = i == db.active;
        let (glyph, color) = if active {
            (theme::ICON_PLUG_CONNECTED, theme::TEAL)
        } else {
            (theme::ICON_PLUG, theme::MUTED)
        };
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .pl(px(10.0))
                .pr(px(10.0))
                .py(px(3.0))
                .child(icon(glyph, 11.0, color))
                .child(text(&conn.name, 11.0, color)),
        );
    }
    col = col.child(div().h(px(8.0))).child(pixel_header("SCHEMA"));
    if let Some(conn) = db.connections.get(db.active) {
        if conn.tables.is_empty() {
            col = col.child(
                div()
                    .pl(px(12.0))
                    .child(text("no tables", 11.0, theme::FAINT)),
            );
        }
        for t in &conn.tables {
            let name = t.name.clone();
            let expanded = db.expanded.contains(&name);
            let browsing = db.browsing.as_deref() == Some(name.as_str());
            let color = if browsing { theme::TEAL } else { theme::BLUE };
            let chevron = if expanded {
                theme::ICON_CHEVRON_DOWN
            } else {
                theme::ICON_CHEVRON_RIGHT
            };
            // Table row: chevron toggles the column list; the name browses rows.
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(3.0))
                    .pl(px(12.0))
                    .pr(px(10.0))
                    .py(px(2.0))
                    .child(
                        div()
                            .id(SharedString::from(format!("tblx-{name}")))
                            .cursor_pointer()
                            .child(icon(chevron, 10.0, theme::FAINT))
                            .on_click(cx.listener({
                                let n = name.clone();
                                move |this, _, _, cx| this.toggle_table(n.clone(), cx)
                            })),
                    )
                    .child(icon(theme::ICON_TABLE, 11.0, color))
                    .child(
                        div()
                            .id(SharedString::from(format!("tbl-{name}")))
                            .cursor_pointer()
                            .child(text(&name, 11.0, color))
                            .on_click(cx.listener({
                                let n = name.clone();
                                move |this, _, window, cx| this.browse_table(n.clone(), 0, window, cx)
                            })),
                    ),
            );
            // Expanded: one row per column with its type + PK / not-null marker.
            if expanded {
                if let Some(columns) = db.table_columns.get(&name) {
                    for c in columns {
                        let badge = if c.primary_key {
                            " PK"
                        } else if c.not_null {
                            " ●"
                        } else {
                            ""
                        };
                        let cc = if c.primary_key { theme::TEAL } else { theme::FG2 };
                        col = col.child(
                            div()
                                .flex()
                                .items_center()
                                .pl(px(34.0))
                                .pr(px(10.0))
                                .py(px(1.0))
                                .child(text(&format!("{}{}", c.name, badge), 10.5, cc))
                                .child(div().ml_auto().child(text(
                                    &c.sql_type.to_lowercase(),
                                    9.0,
                                    theme::FAINT,
                                ))),
                        );
                    }
                } else {
                    col = col.child(
                        div()
                            .pl(px(34.0))
                            .child(text("loading…", 10.0, theme::FAINT)),
                    );
                }
            }
        }
    }

    // ── History ──
    if !history.is_empty() {
        col = col.child(div().h(px(8.0))).child(pixel_header("HISTORY"));
        for (i, h) in history.iter().take(20).enumerate() {
            let mark = if h.ok { "✓" } else { "✗" };
            let mark_color = if h.ok { theme::GREEN_LT } else { theme::RED_LT };
            let one_line: String = h.sql.split_whitespace().collect::<Vec<_>>().join(" ");
            let preview: String = one_line.chars().take(28).collect();
            let sql = h.sql.clone();
            col = col.child(
                div()
                    .id(SharedString::from(format!("hist-{i}")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .pl(px(12.0))
                    .pr(px(10.0))
                    .py(px(2.0))
                    .child(text(mark, 9.0, mark_color))
                    .child(text(&preview, 10.5, theme::FG2))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_history(sql.clone(), window, cx);
                    })),
            );
        }
    }
    col
}

/// Query-tab strip: one chip per query buffer (click to switch, × to close),
/// a `＋` to open a new buffer, and the green RUN button.
pub fn tab_bar(
    tabs: &[QueryTab],
    active_tab: usize,
    db: &DbState,
    cx: &mut Context<EnzoApp>,
) -> impl IntoElement {
    let run = div()
        .id("db-run")
        .cursor_pointer()
        .ml_auto()
        .flex()
        .items_center()
        .gap(px(4.0))
        .px(px(9.0))
        .py(px(4.0))
        .rounded(px(3.0))
        .bg(theme::GREEN)
        .text_color(theme::GREEN_INK)
        .child(icon(theme::ICON_PLAYER_PLAY, 10.0, theme::GREEN_INK))
        .child(
            div()
                .text_size(px(8.0))
                .font_family(theme::FONT_PIXEL)
                .child(if db.running { "RUNNING…" } else { "RUN ⌘↵" }),
        )
        .on_click(cx.listener(|this, _, window, cx| this.run_query(window, cx)));

    let mut bar = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(12.0))
        .py(px(7.0))
        .bg(theme::BG_BAR)
        .border_b_2()
        .border_color(theme::BORDER);

    for (i, tab) in tabs.iter().enumerate() {
        let active = i == active_tab;
        let id = tab.id;
        let mut chip = div()
            .id(SharedString::from(format!("qtab-{id}")))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(5.0))
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(3.0))
            .text_size(px(8.0))
            .font_family(theme::FONT_PIXEL)
            .on_click(cx.listener(move |this, _, window, cx| this.switch_tab(i, window, cx)));
        if active {
            chip = chip.bg(theme::BG_SURFACE).text_color(theme::TEAL);
        } else {
            chip = chip.text_color(theme::FG1);
        }
        chip = chip.child(SharedString::from(tab.title.clone()));
        if tabs.len() > 1 {
            chip = chip.child(
                div()
                    .id(SharedString::from(format!("qtab-x-{id}")))
                    .cursor_pointer()
                    .text_color(theme::FAINT)
                    .child("×")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_tab(i, window, cx);
                    })),
            );
        }
        bar = bar.child(chip);
    }

    bar.child(
        div()
            .id("db-new-tab")
            .cursor_pointer()
            .px(px(6.0))
            .py(px(4.0))
            .text_size(px(10.0))
            .text_color(theme::PURPLE)
            .child("＋")
            .on_click(cx.listener(|this, _, window, cx| this.add_tab(window, cx))),
    )
    .child(run)
}

/// Multi-line SQL editor (the active query buffer) + result grid (or error).
pub fn content(
    db: &DbState,
    editor: &Entity<InputState>,
    cell_input: &Entity<TextInput>,
    cx: &mut Context<EnzoApp>,
) -> impl IntoElement {
    let sql_line = div()
        .h(px(190.0))
        .flex_none()
        .text_size(px(13.0))
        .border_b_2()
        .border_color(theme::BORDER)
        .child(Input::new(editor));

    let body = if let Some(err) = &db.error {
        div()
            .m(px(12.0))
            .px(px(10.0))
            .py(px(8.0))
            .bg(theme::rgb_hex(0x2a1416))
            .border_1()
            .border_color(theme::RED)
            .rounded(px(5.0))
            .child(text(&format!("✗ {err}"), 12.0, theme::RED_LT))
            .into_any_element()
    } else if db.columns.is_empty() {
        div().into_any_element()
    } else {
        result_grid(db, cell_input, cx).into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .size_full()
        .child(sql_line)
        .child(body)
}

/// One flex grid cell base (equal-weight columns).
fn grid_cell() -> gpui::Div {
    div()
        .flex_grow(1.0)
        .flex_shrink(1.0)
        .flex_basis(px(0.0))
        .px(px(10.0))
        .py(px(5.0))
}

/// Result grid from real `columns`/`rows`; double-click a cell to edit (when the
/// view is an editable table with a known primary key).
fn result_grid(
    db: &DbState,
    cell_input: &Entity<TextInput>,
    cx: &mut Context<EnzoApp>,
) -> impl IntoElement {
    let editable = db.editable();
    let mut header = div()
        .flex()
        .bg(theme::BG_CARD)
        .text_size(px(8.0))
        .font_family(theme::FONT_PIXEL);
    for c in &db.columns {
        header = header.child(
            grid_cell()
                .text_color(theme::PURPLE)
                .child(SharedString::from(c.to_uppercase())),
        );
    }

    let mut grid = div()
        .flex()
        .flex_col()
        .flex_1()
        .overflow_hidden()
        .child(header);
    for (ri, row) in db.rows.iter().enumerate() {
        let mut r = div()
            .flex()
            .border_b_1()
            .border_color(theme::DIVIDER)
            .text_size(px(11.0));
        if ri % 2 == 1 {
            r = r.bg(theme::BG_SIDE);
        }
        for (ci, v) in row.iter().enumerate() {
            let cell_el: gpui::AnyElement = if db.editing == Some((ri, ci)) {
                grid_cell()
                    .key_context("DbCell")
                    .bg(theme::BG_CARD)
                    .font_family(theme::FONT_MONO)
                    .text_color(theme::FG0)
                    .child(cell_input.clone())
                    .into_any_element()
            } else {
                let color = if ci == 0 { theme::TEAL } else { theme::FG2 };
                let base = grid_cell()
                    .text_color(color)
                    .child(SharedString::from(v.clone()));
                if editable {
                    base.id(SharedString::from(format!("cell-{ri}-{ci}")))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                            if ev.click_count() == 2 {
                                this.start_edit(ri, ci, window, cx);
                            }
                        }))
                        .into_any_element()
                } else {
                    base.into_any_element()
                }
            };
            r = r.child(cell_el);
        }
        grid = grid.child(r);
    }
    grid
}

/// Status bar: connection + row count + timing (+ clickable pager when browsing).
pub fn status_bar(db: &DbState, cx: &mut Context<EnzoApp>) -> impl IntoElement {
    let cell = |s: String, c: gpui::Rgba| {
        div()
            .text_size(px(8.0))
            .font_family(theme::FONT_PIXEL)
            .text_color(c)
            .child(SharedString::from(s))
    };
    let pager_btn = |id: &'static str,
                     glyph: &'static str,
                     enabled: bool,
                     delta: i64,
                     cx: &mut Context<EnzoApp>| {
        let mut b = div()
            .id(id)
            .px(px(4.0))
            .text_size(px(8.0))
            .font_family(theme::FONT_PIXEL)
            .text_color(if enabled { theme::TEAL } else { theme::FAINT })
            .child(glyph);
        if enabled {
            b = b
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.page_relative(delta, window, cx)
                }));
        }
        b
    };
    let conn = db.connections.get(db.active).map_or_else(
        || "no connection".to_owned(),
        |c| {
            format!(
                "ADBC {}",
                if c.driver.is_empty() {
                    "…"
                } else {
                    &c.driver
                }
            )
        },
    );
    let export_btn = |id: &'static str, label: &'static str, json: bool, cx: &mut Context<EnzoApp>| {
        div()
            .id(id)
            .cursor_pointer()
            .px(px(6.0))
            .py(px(2.0))
            .rounded(px(3.0))
            .bg(theme::BG_CARD)
            .text_size(px(8.0))
            .font_family(theme::FONT_PIXEL)
            .text_color(theme::TEAL)
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.export_results(json, cx)))
    };
    let mut bar = div()
        .flex()
        .items_center()
        .gap(px(14.0))
        .px(px(12.0))
        .py(px(6.0))
        .bg(theme::BG_BAR)
        .border_t_2()
        .border_color(theme::BORDER)
        .child(cell(format!("● {conn}"), theme::TEAL))
        .child(cell(format!("{} rows", db.rows.len()), theme::FG1));
    if let Some(ms) = db.query_ms {
        bar = bar.child(cell(format!("{ms}ms · Arrow stream"), theme::FG1));
    }
    if let Some(msg) = &db.export_msg {
        bar = bar.child(cell(format!("⤓ {msg}"), theme::GREEN_LT));
    }
    if !db.columns.is_empty() {
        bar = bar
            .child(export_btn("exp-csv", "⤓ CSV", false, cx))
            .child(export_btn("exp-json", "⤓ JSON", true, cx));
    }
    if let (Some(total), Some(table)) = (db.total, db.browsing.as_ref()) {
        let pages = total.div_ceil(PAGE_SIZE).max(1);
        bar = bar
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(pager_btn("pg-prev", "‹ PREV", db.page > 0, -1, cx))
                    .child(cell(
                        format!("{} · page {}/{}", table, db.page + 1, pages),
                        theme::FAINT,
                    ))
                    .child(pager_btn("pg-next", "NEXT ›", db.page + 1 < pages, 1, cx)),
            )
            .child(cell("⌘K".to_owned(), theme::FAINT));
        return bar;
    }
    bar.child(div().ml_auto().child(cell("⌘K".to_owned(), theme::FAINT)))
}

// ── Add-connection dialog (modal overlay) ───────────────────────────────────

/// Modal overlay to add a connection by file path, faithful to
/// `design/mockups/db-connection.html` (NAME + DATABASE PATH for our path-based
/// `db.connect`).
pub fn connection_dialog(
    name: &Entity<TextInput>,
    path: &Entity<TextInput>,
    driver: &str,
    cx: &mut Context<EnzoApp>,
) -> impl IntoElement {
    // Driver selector chip.
    let driver_chip = |id: &'static str, label: &'static str, value: &'static str, active: bool| {
        let mut c = div()
            .id(id)
            .cursor_pointer()
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(5.0))
            .text_size(px(9.0))
            .font_family(theme::FONT_PIXEL)
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.set_dialog_driver(value, cx)));
        if active {
            c = c.bg(theme::TEAL).text_color(theme::GREEN_INK);
        } else {
            c = c
                .bg(theme::BG_CARD)
                .border_1()
                .border_color(theme::BORDER)
                .text_color(theme::FG1);
        }
        c
    };
    let driver_row = div()
        .flex()
        .flex_col()
        .child(
            div()
                .pb(px(5.0))
                .text_size(px(8.0))
                .font_family(theme::FONT_PIXEL)
                .text_color(theme::PURPLE)
                .child("DRIVER"),
        )
        .child(
            div()
                .flex()
                .gap(px(8.0))
                .child(driver_chip("drv-sqlite", "SQLITE", "sqlite", driver == "sqlite"))
                .child(driver_chip("drv-duckdb", "DUCKDB", "duckdb", driver == "duckdb")),
        );
    // A captioned, framed input field (`.cap` + `.inp` in the mockup).
    let field = |cap: &str, input: &Entity<TextInput>| {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .pb(px(5.0))
                    .text_size(px(8.0))
                    .font_family(theme::FONT_PIXEL)
                    .text_color(theme::PURPLE)
                    .child(SharedString::from(cap.to_owned())),
            )
            .child(
                div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .bg(theme::BG_CARD)
                    .border_1()
                    .border_color(theme::BORDER)
                    .rounded(px(5.0))
                    .text_size(px(12.0))
                    .font_family(theme::FONT_MONO)
                    .text_color(theme::FG0)
                    .child(input.clone()),
            )
    };
    let button = |id: &'static str, label: &'static str, save: bool| {
        let mut b = div()
            .id(id)
            .cursor_pointer()
            .px(px(16.0))
            .py(px(9.0))
            .rounded(px(5.0))
            .text_size(px(9.0))
            .font_family(theme::FONT_PIXEL);
        b = if save {
            b.bg(theme::GREEN).text_color(theme::GREEN_INK)
        } else {
            b.bg(theme::BG_CARD)
                .border_1()
                .border_color(theme::BORDER)
                .text_color(theme::FG1)
        };
        b.child(label)
    };

    // Dim backdrop covering the window, centered card.
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
                .w(px(440.0))
                .bg(theme::BG_SURFACE)
                .border_3()
                .border_color(theme::BORDER)
                .rounded(px(12.0))
                .overflow_hidden()
                .child(
                    div()
                        .px(px(16.0))
                        .py(px(10.0))
                        .bg(theme::BG_BAR)
                        .border_b_2()
                        .border_color(theme::BORDER)
                        .text_size(px(10.0))
                        .font_family(theme::FONT_PIXEL)
                        .text_color(theme::FG1)
                        .child("NEW CONNECTION"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .px(px(20.0))
                        .py(px(16.0))
                        .child(text(
                            "Open a local database file. Use :memory: for a scratch in-memory DB.",
                            11.0,
                            theme::FAINT,
                        ))
                        .child(driver_row)
                        .child(field("NAME", name))
                        .child(field("DATABASE FILE  (.sqlite / .db / .duckdb / :memory:)", path))
                        .child(
                            div()
                                .flex()
                                .gap(px(10.0))
                                .pt(px(4.0))
                                .child(
                                    button("dlg-cancel", "CANCEL", false).on_click(
                                        cx.listener(|this, _, _, cx| this.close_dialog(cx)),
                                    ),
                                )
                                .child(button("dlg-save", "CONNECT", true).on_click(
                                    cx.listener(|this, _, _, cx| this.save_connection(cx)),
                                )),
                        ),
                ),
        )
}

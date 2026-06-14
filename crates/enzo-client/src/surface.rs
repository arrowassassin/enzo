//! Application surface state — one per top-level panel type.
//!
//! Each surface owns the UI state needed by the renderer and the event loop.
//! Heavy engine integration (LSP, DAP, real DB drivers, CEF) lives in the
//! daemon; these structs hold only what the client needs to render and
//! respond to keyboard input.

// ── Surface discriminant ──────────────────────────────────────────────────────

/// Which top-level panel is currently displayed.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Surface {
    /// PTY terminal (always available, the home surface).
    #[default]
    Terminal,
    /// Code editor / IDE view.
    Ide,
    /// Relational / SQL database client.
    Database,
    /// Web browser panel (future CEF integration).
    Browser,
}

impl Surface {
    /// Cycle to the next surface in declaration order.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Terminal => Self::Ide,
            Self::Ide => Self::Database,
            Self::Database => Self::Browser,
            Self::Browser => Self::Terminal,
        }
    }
}

// ── IDE surface ───────────────────────────────────────────────────────────────

/// One entry in the IDE file explorer.
#[derive(Clone, Debug)]
pub struct FileEntry {
    /// Absolute path.
    pub path: String,
    /// File or directory name (last component of `path`).
    pub name: String,
    /// `true` if this entry is a directory.
    pub is_dir: bool,
    /// Nesting depth (0 = root level).
    pub depth: usize,
}

/// IDE surface state: file explorer + open-file viewer.
pub struct IdeState {
    /// Root directory currently shown.
    pub root: String,
    /// Flat listing of currently *visible* tree rows (expanded dirs inlined).
    pub entries: Vec<FileEntry>,
    /// Highlighted entry index.
    pub selected: usize,
    /// Lines of the open file.
    pub lines: Vec<String>,
    /// Path of the currently open file, if any.
    pub open_path: Option<String>,
    /// Language id of the open file (for the status bar).
    pub language: String,
    /// First visible line in the code view.
    pub scroll: usize,
    /// Cursor line within the code view.
    pub cursor_line: usize,
    /// Paths of directories currently expanded.
    expanded: std::collections::HashSet<String>,
}

impl IdeState {
    /// Create an IDE state rooted at `root`. Scans only the top level.
    #[must_use]
    pub fn new(root: impl Into<String>) -> Self {
        let root = root.into();
        let entries = scan_dir(&root, 0, 0);
        Self {
            root,
            entries,
            selected: 0,
            lines: Vec::new(),
            open_path: None,
            language: "plaintext".to_owned(),
            scroll: 0,
            cursor_line: 0,
            expanded: std::collections::HashSet::new(),
        }
    }

    /// `true` if the directory at `path` is expanded.
    #[must_use]
    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    /// Activate entry `index`: expand/collapse a directory, or open a file.
    pub fn activate(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index).cloned() else {
            return;
        };
        self.selected = index;
        if entry.is_dir {
            self.toggle_dir(index, &entry);
        } else {
            self.open_file(&entry);
        }
    }

    /// Expand or collapse the directory entry at `index`.
    fn toggle_dir(&mut self, index: usize, entry: &FileEntry) {
        if self.expanded.remove(&entry.path) {
            // Collapse: drop all following rows nested under this directory.
            // `remove(i)` shifts the next row into `i`, so the index stays put.
            let i = index + 1;
            while i < self.entries.len() && self.entries[i].depth > entry.depth {
                self.entries.remove(i);
            }
        } else {
            // Expand: scan one level and splice the children in after `index`.
            self.expanded.insert(entry.path.clone());
            let children = scan_dir(&entry.path, entry.depth + 1, entry.depth + 1);
            for (k, child) in children.into_iter().enumerate() {
                self.entries.insert(index + 1 + k, child);
            }
        }
    }

    fn open_file(&mut self, entry: &FileEntry) {
        if let Ok(text) = std::fs::read_to_string(&entry.path) {
            self.lines = text.lines().map(str::to_string).collect();
            self.open_path = Some(entry.name.clone());
            self.language = language_id(&entry.name);
            self.scroll = 0;
            self.cursor_line = 0;
        }
    }

    /// Open the currently selected entry (kept for keyboard navigation).
    pub fn open_selected(&mut self) {
        self.activate(self.selected);
    }

    /// Move the file explorer selection by `delta` rows.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "entry count is always far below i64::MAX; safe cast"
    )]
    pub fn move_selection(&mut self, delta: i32) {
        let n = self.entries.len();
        if n == 0 {
            return;
        }
        let i = (self.selected as i64 + i64::from(delta)).rem_euclid(n as i64);
        self.selected = i as usize;
    }

    /// Scroll the code view by `delta` lines.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "line count is always far below i64::MAX; safe cast"
    )]
    pub fn scroll_content(&mut self, delta: i32) {
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let new = (self.scroll as i64 + i64::from(delta)).clamp(0, n as i64 - 1);
        self.scroll = new as usize;
    }
}

/// Recursively scan `path` up to `max_depth` levels deep.
fn scan_dir(path: &str, depth: usize, max_depth: usize) -> Vec<FileEntry> {
    let Ok(dir) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut children: Vec<std::fs::DirEntry> = dir.filter_map(std::result::Result::ok).collect();
    children.sort_by_key(|e| {
        let is_file = e.file_type().is_ok_and(|t| t.is_file());
        (is_file, e.file_name())
    });

    let mut out = Vec::new();
    for entry in children {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path_str = entry.path().to_string_lossy().to_string();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        out.push(FileEntry {
            path: path_str.clone(),
            name,
            is_dir,
            depth,
        });
        if is_dir && depth < max_depth {
            out.extend(scan_dir(&path_str, depth + 1, max_depth));
        }
    }
    out
}

/// Infer a short language id from a file name (drives status bar + highlight).
#[must_use]
pub fn language_id(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => "javascript",
        "json" | "jsonc" => "json",
        "toml" => "toml",
        "md" => "markdown",
        _ => "plaintext",
    }
    .to_owned()
}

// ── Database surface ──────────────────────────────────────────────────────────

/// One SQL query tab.
#[derive(Clone, Debug)]
pub struct QueryTab {
    /// Tab title.
    pub title: String,
    /// SQL editor contents.
    pub sql: String,
}

/// One table or view in a connection's schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableInfo {
    /// Object name.
    pub name: String,
    /// `"table"` or `"view"`.
    pub kind: String,
}

/// A live database connection mirrored on the client.
///
/// `id` is the ATP connection id used in `db.*` requests; the daemon owns the
/// real driver/pool. `tables` is populated from `db.schema.tables` once the
/// connection is established.
#[derive(Clone, Debug)]
pub struct DbConnection {
    /// ATP connection id (e.g. `"db-0"`).
    pub id: String,
    /// Display name shown in the sidebar (e.g. `"SQLite · demo.db"`).
    pub name: String,
    /// File path or `:memory:` the connection points at.
    pub path: String,
    /// Driver name reported by the daemon (e.g. `"sqlite"`).
    pub driver: String,
    /// Tables/views in this connection's schema.
    pub tables: Vec<TableInfo>,
}

/// Default page size for table browsing / pagination.
pub const DB_PAGE_SIZE: u64 = 100;

/// Database surface state: connections + multiple SQL query tabs + result table.
///
/// All result data is owned by the daemon and streamed back over ATP; this
/// struct holds only what the renderer needs. There is no demo/sample data —
/// an empty state renders an empty surface until a real connection is made.
#[derive(Default)]
pub struct DbState {
    /// Open query tabs.
    pub tabs: Vec<QueryTab>,
    /// Active tab index.
    pub active_tab: usize,
    /// Cursor offset (bytes) within the active query.
    pub cursor: usize,
    /// Column headers of the last result set.
    pub columns: Vec<String>,
    /// Row data of the last result set.
    pub rows: Vec<Vec<String>>,
    /// Error from the last query, if any.
    pub error: Option<String>,
    /// First visible row in the result grid.
    pub result_scroll: usize,
    /// Execution time of the last query in milliseconds.
    pub query_ms: Option<u64>,
    /// Configured connections (real, daemon-backed).
    pub connections: Vec<DbConnection>,
    /// Active connection index.
    pub active_conn_idx: usize,
    /// Table currently being browsed (paginated), if any.
    pub browsing: Option<String>,
    /// Total row count of the browsed table (drives the pager), if known.
    pub total_rows: Option<u64>,
    /// Current page index when browsing a table.
    pub page: u64,
    /// Whether the "add connection" dialog is open.
    pub dialog_open: bool,
    /// Display-name field in the add-connection dialog.
    pub dialog_name: String,
    /// Driver field in the add-connection dialog (display only; SQLite for now).
    pub dialog_driver: String,
    /// Path / connection-string field in the add-connection dialog.
    pub dialog_path: String,
    /// "Store in vault" toggle in the add-connection dialog.
    pub dialog_vault: bool,
    /// "Unlock with Touch ID" toggle in the add-connection dialog.
    pub dialog_touchid: bool,
    /// Whether a query/browse request is in flight (drives the RUN spinner).
    pub running: bool,
    /// Next tab number for naming.
    next_tab: u32,
    /// Next connection number for id generation.
    next_conn: u32,
}

impl DbState {
    /// Fresh, empty state with a single blank query tab and no connections.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tabs: vec![QueryTab {
                title: "query 1".to_owned(),
                sql: String::new(),
            }],
            page: 0,
            next_tab: 1,
            ..Self::default()
        }
    }

    /// The active connection, if any.
    #[must_use]
    pub fn active_connection(&self) -> Option<&DbConnection> {
        self.connections.get(self.active_conn_idx)
    }

    /// ATP id of the active connection, if any.
    #[must_use]
    pub fn active_conn_id(&self) -> Option<&str> {
        self.active_connection().map(|c| c.id.as_str())
    }

    /// Display name of the active connection.
    #[must_use]
    pub fn active_conn(&self) -> &str {
        self.active_connection()
            .map_or("no connection", |c| c.name.as_str())
    }

    /// Tables of the active connection (empty if none).
    #[must_use]
    pub fn active_tables(&self) -> &[TableInfo] {
        self.active_connection().map_or(&[], |c| &c.tables)
    }

    /// Mutable SQL of the active query tab (for the editor).
    pub fn active_sql_mut(&mut self) -> &mut String {
        if self.tabs.is_empty() {
            self.add_query_tab();
        }
        let i = self.active_tab.min(self.tabs.len() - 1);
        &mut self.tabs[i].sql
    }

    /// SQL of the active query tab.
    #[must_use]
    pub fn active_sql(&self) -> &str {
        self.tabs
            .get(self.active_tab)
            .map_or("", |t| t.sql.as_str())
    }

    /// Open the add-connection dialog with sensible SQLite defaults.
    pub fn open_dialog(&mut self) {
        self.dialog_open = true;
        self.dialog_name = "local · sqlite".to_owned();
        self.dialog_driver = "SQLite (ADBC)".to_owned();
        self.dialog_path.clear();
        self.dialog_vault = true;
        self.dialog_touchid = false;
    }

    /// Open a new empty query tab and make it active.
    pub fn add_query_tab(&mut self) {
        self.next_tab += 1;
        self.tabs.push(QueryTab {
            title: format!("query {}", self.next_tab),
            sql: String::new(),
        });
        self.active_tab = self.tabs.len() - 1;
    }

    /// Allocate a fresh ATP connection id (e.g. `"db-0"`).
    pub fn next_conn_id(&mut self) -> String {
        let id = format!("db-{}", self.next_conn);
        self.next_conn += 1;
        id
    }

    /// Register a new (pending) connection and make it active. Returns its id.
    pub fn add_connection(&mut self, name: impl Into<String>, path: impl Into<String>) -> String {
        let id = self.next_conn_id();
        self.connections.push(DbConnection {
            id: id.clone(),
            name: name.into(),
            path: path.into(),
            driver: String::new(),
            tables: Vec::new(),
        });
        self.active_conn_idx = self.connections.len() - 1;
        id
    }

    /// Record the driver reported by the daemon for connection `id`.
    pub fn set_driver(&mut self, id: &str, driver: impl Into<String>) {
        if let Some(c) = self.connections.iter_mut().find(|c| c.id == id) {
            c.driver = driver.into();
        }
    }

    /// Replace the table list for connection `id`.
    pub fn set_tables(&mut self, id: &str, tables: Vec<TableInfo>) {
        if let Some(c) = self.connections.iter_mut().find(|c| c.id == id) {
            c.tables = tables;
        }
    }

    /// Apply a successful query/browse result set.
    pub fn apply_result(&mut self, columns: Vec<String>, rows: Vec<Vec<String>>, ms: u64) {
        self.columns = columns;
        self.rows = rows;
        self.query_ms = Some(ms);
        self.error = None;
        self.result_scroll = 0;
        self.running = false;
    }

    /// Apply a query error (clears the previous result set's rows).
    pub fn apply_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
        self.columns.clear();
        self.rows.clear();
        self.query_ms = None;
        self.running = false;
        self.browsing = None;
        self.total_rows = None;
    }
}

// ── Browser surface ───────────────────────────────────────────────────────────

/// Which CDP sub-panel is shown in the browser surface.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum BrowserPanel {
    /// Rendered page view (future CEF texture).
    Page,
    /// Network requests captured via CDP.
    #[default]
    Network,
    /// Console log lines from the page.
    Console,
}

/// One captured network request shown in the browser devtools panel.
#[derive(Clone, Debug)]
pub struct NetworkRequest {
    /// HTTP method (e.g. `"GET"`, `"POST"`).
    pub method: String,
    /// Request path (e.g. `"/api/me"`).
    pub path: String,
    /// HTTP response status code.
    pub status: u16,
    /// Round-trip time in milliseconds.
    pub ms: u32,
}

/// Browser surface state: URL bar + devtools panel.
pub struct BrowserState {
    /// Current URL.
    pub url: String,
    /// Active devtools panel.
    pub panel: BrowserPanel,
    /// Captured network requests.
    pub requests: Vec<NetworkRequest>,
    /// Console log lines.
    pub console_lines: Vec<String>,
    /// Whether a CDP session is connected.
    pub cdp_connected: bool,
}

impl BrowserState {
    /// Initial demo state.
    #[must_use]
    pub fn demo() -> Self {
        Self {
            url: "http://localhost:8080".to_string(),
            panel: BrowserPanel::Network,
            requests: vec![
                NetworkRequest {
                    method: "GET".into(),
                    path: "/".into(),
                    status: 200,
                    ms: 5,
                },
                NetworkRequest {
                    method: "GET".into(),
                    path: "/api/me".into(),
                    status: 401,
                    ms: 3,
                },
                NetworkRequest {
                    method: "POST".into(),
                    path: "/api/login".into(),
                    status: 200,
                    ms: 88,
                },
                NetworkRequest {
                    method: "GET".into(),
                    path: "/static/app.js".into(),
                    status: 200,
                    ms: 12,
                },
                NetworkRequest {
                    method: "GET".into(),
                    path: "/favicon.ico".into(),
                    status: 404,
                    ms: 1,
                },
            ],
            console_lines: vec![
                "[ERR] 401 Unauthorized — GET /api/me".into(),
                "[LOG] React mounted".into(),
            ],
            cdp_connected: false,
        }
    }
}

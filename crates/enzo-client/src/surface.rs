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
    /// Flat, depth-sorted listing.
    pub entries: Vec<FileEntry>,
    /// Highlighted entry index.
    pub selected: usize,
    /// Lines of the open file.
    pub lines: Vec<String>,
    /// Path of the currently open file, if any.
    pub open_path: Option<String>,
    /// First visible line in the code view.
    pub scroll: usize,
    /// Cursor line within the code view.
    pub cursor_line: usize,
}

impl IdeState {
    /// Create an IDE state rooted at `root`.  Scans the directory immediately.
    #[must_use]
    pub fn new(root: impl Into<String>) -> Self {
        let root = root.into();
        let entries = scan_dir(&root, 0, 1);
        Self {
            root,
            entries,
            selected: 0,
            lines: Vec::new(),
            open_path: None,
            scroll: 0,
            cursor_line: 0,
        }
    }

    /// Open the currently selected file (no-op on directories).
    pub fn open_selected(&mut self) {
        if let Some(entry) = self.entries.get(self.selected).cloned()
            && !entry.is_dir
            && let Ok(text) = std::fs::read_to_string(&entry.path)
        {
            self.lines = text.lines().map(str::to_string).collect();
            self.open_path = Some(entry.name.clone());
            self.scroll = 0;
            self.cursor_line = 0;
        }
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

// ── Database surface ──────────────────────────────────────────────────────────

/// Database surface state: SQL editor + result table.
pub struct DbState {
    /// Text in the SQL editor.
    pub query: String,
    /// Cursor offset (bytes) within `query`.
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
    /// Display name of the active connection.
    pub active_conn: String,
}

impl DbState {
    /// Initial demo state with sample data.
    #[must_use]
    pub fn demo() -> Self {
        Self {
            query: "SELECT id, name, email FROM users LIMIT 10;".to_string(),
            cursor: 44,
            columns: vec!["id".to_string(), "name".to_string(), "email".to_string()],
            rows: vec![
                vec![
                    "1".to_string(),
                    "Alice".to_string(),
                    "alice@example.com".to_string(),
                ],
                vec![
                    "2".to_string(),
                    "Bob".to_string(),
                    "bob@example.com".to_string(),
                ],
                vec![
                    "3".to_string(),
                    "Carol".to_string(),
                    "carol@example.com".to_string(),
                ],
                vec![
                    "4".to_string(),
                    "Dave".to_string(),
                    "dave@example.com".to_string(),
                ],
                vec![
                    "5".to_string(),
                    "Eve".to_string(),
                    "eve@example.com".to_string(),
                ],
                vec![
                    "6".to_string(),
                    "Frank".to_string(),
                    "frank@example.com".to_string(),
                ],
            ],
            error: None,
            result_scroll: 0,
            query_ms: Some(12),
            active_conn: "SQLite · enzo.db".to_string(),
        }
    }

    /// Append a character at the cursor.
    pub fn insert(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let (idx, _) = self
            .query
            .char_indices()
            .rev()
            .find(|&(i, _)| i < self.cursor)
            .unwrap_or((0, ' '));
        self.query.remove(idx);
        self.cursor = idx;
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

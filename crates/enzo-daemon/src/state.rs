//! Shared daemon state — cloneable handle backed by an `Arc<Mutex<Inner>>`.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, broadcast, oneshot};

use enzo_browser::Page;
use enzo_db::pool::AnyPool;
use enzo_db::tabs::TabManager;
use enzo_editor::lsp::LspClient;
use enzo_theme::ThemeRegistry;

use crate::session::{Session, SessionId};

/// Thread-safe, cheaply-cloneable handle to all live daemon state.
#[derive(Clone)]
pub struct DaemonState(Arc<Mutex<Inner>>);

struct Inner {
    sessions: HashMap<SessionId, Session>,
    db_conns: HashMap<String, AnyPool>,
    /// Per-connection query-tab managers (multi-tab SQL editor + history).
    db_tabs: HashMap<String, TabManager>,
    lsp_clients: HashMap<String, Arc<LspClient>>,
    browser_pages: HashMap<String, Arc<Page>>,
    /// Launched headless-browser child processes, kept alive until close.
    browser_procs: HashMap<String, std::process::Child>,
    /// Pending prompt responses keyed by prompt id.
    prompt_channels: HashMap<String, oneshot::Sender<String>>,
    /// Broadcast channel for pushing notifications to all registered display clients.
    notif_tx: broadcast::Sender<String>,
    /// Global theme registry (built-ins + user themes, active selection).
    themes: ThemeRegistry,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonState {
    /// Create an empty state with no active sessions or connections.
    #[must_use]
    pub fn new() -> Self {
        let (notif_tx, _) = broadcast::channel(256);
        Self(Arc::new(Mutex::new(Inner {
            sessions: HashMap::new(),
            db_conns: HashMap::new(),
            db_tabs: HashMap::new(),
            lsp_clients: HashMap::new(),
            browser_pages: HashMap::new(),
            browser_procs: HashMap::new(),
            prompt_channels: HashMap::new(),
            notif_tx,
            themes: ThemeRegistry::new(),
        })))
    }

    // ── Sessions ──────────────────────────────────────────────────────────────

    /// Insert a session, replacing any existing session with the same id.
    pub async fn insert_session(&self, session: Session) {
        self.0
            .lock()
            .await
            .sessions
            .insert(session.id.clone(), session);
    }

    /// Whether a session with `id` already exists.
    pub async fn session_exists(&self, id: &str) -> bool {
        self.0.lock().await.sessions.contains_key(id)
    }

    /// Remove and return the session with `id`, if present.
    pub async fn remove_session(&self, id: &str) -> Option<Session> {
        self.0.lock().await.sessions.remove(id)
    }

    /// Write bytes to a session's stdin. Returns `None` if the session doesn't exist.
    pub async fn session_write_stdin(
        &self,
        id: &str,
        data: Vec<u8>,
    ) -> Option<std::io::Result<()>> {
        let inner = self.0.lock().await;
        let session = inner.sessions.get(id)?;
        Some(session.write_stdin_sync(&data))
    }

    /// Resize a session's PTY. Returns `None` if the session doesn't exist.
    pub async fn session_resize(
        &self,
        id: &str,
        cols: u16,
        rows: u16,
    ) -> Option<Result<(), String>> {
        let inner = self.0.lock().await;
        let session = inner.sessions.get(id)?;
        Some(session.resize_sync(cols, rows))
    }

    // ── Database connections ──────────────────────────────────────────────────

    /// Register a database connection pool under `id`.
    pub async fn insert_db_conn(&self, id: String, pool: AnyPool) {
        self.0.lock().await.db_conns.insert(id, pool);
    }

    /// Return a clone of the connection pool registered under `id`, if any.
    pub async fn get_db_conn(&self, id: &str) -> Option<AnyPool> {
        self.0.lock().await.db_conns.get(id).cloned()
    }

    /// Remove and return the connection pool registered under `id`, if any.
    pub async fn remove_db_conn(&self, id: &str) -> Option<AnyPool> {
        self.0.lock().await.db_conns.remove(id)
    }

    // ── LSP clients ───────────────────────────────────────────────────────────

    /// Register an LSP client under `id`.
    pub async fn insert_lsp_client(&self, id: String, client: LspClient) {
        self.0.lock().await.lsp_clients.insert(id, Arc::new(client));
    }

    /// Return a shared handle to the LSP client registered under `id`, if any.
    pub async fn get_lsp_client(&self, id: &str) -> Option<Arc<LspClient>> {
        self.0.lock().await.lsp_clients.get(id).cloned()
    }

    /// Remove and return the LSP client registered under `id`, if any.
    pub async fn remove_lsp_client(&self, id: &str) -> Option<Arc<LspClient>> {
        self.0.lock().await.lsp_clients.remove(id)
    }

    // ── Browser pages ─────────────────────────────────────────────────────────

    /// Register a browser page under `id`.
    pub async fn insert_browser_page(&self, id: String, page: Page) {
        self.0.lock().await.browser_pages.insert(id, Arc::new(page));
    }

    /// Register a launched browser process under `id` (kept alive until close).
    pub async fn insert_browser_proc(&self, id: String, child: std::process::Child) {
        self.0.lock().await.browser_procs.insert(id, child);
    }

    /// Return a shared handle to the browser page registered under `id`, if any.
    pub async fn get_browser_page(&self, id: &str) -> Option<Arc<Page>> {
        self.0.lock().await.browser_pages.get(id).cloned()
    }

    /// Remove and return the browser page registered under `id`, if any.
    pub async fn remove_browser_page(&self, id: &str) -> Option<Arc<Page>> {
        let mut inner = self.0.lock().await;
        // Kill any launched browser process for this id.
        if let Some(mut child) = inner.browser_procs.remove(id) {
            let _ = child.kill();
            let _ = child.wait();
        }
        inner.browser_pages.remove(id)
    }

    // ── Notifications (display broadcast) ─────────────────────────────────────

    /// Subscribe to all outbound notifications. Display clients call this to receive
    /// `prompt.show`, `block.push`, and similar push messages.
    pub async fn subscribe_notifications(&self) -> broadcast::Receiver<String> {
        self.0.lock().await.notif_tx.subscribe()
    }

    /// Broadcast a serialised JSON-RPC notification line to all display clients.
    ///
    /// Silently ignores the case where no display is currently registered.
    pub async fn broadcast_notification(&self, json: String) {
        let _ = self.0.lock().await.notif_tx.send(json);
    }

    // ── Prompt channels ───────────────────────────────────────────────────────

    /// Register a one-shot channel for a pending prompt identified by `id`.
    pub async fn insert_prompt(&self, id: String, tx: oneshot::Sender<String>) {
        self.0.lock().await.prompt_channels.insert(id, tx);
    }

    /// Resolve a pending prompt with `action` ("accept", "reject", or "edit").
    ///
    /// Returns `true` if the prompt existed, `false` if it had already timed out
    /// or was never registered.
    pub async fn resolve_prompt(&self, id: &str, action: String) -> bool {
        match self.0.lock().await.prompt_channels.remove(id) {
            Some(tx) => {
                let _ = tx.send(action);
                true
            }
            None => false,
        }
    }

    /// Cancel a pending prompt without sending a response (e.g. on timeout cleanup).
    pub async fn cancel_prompt(&self, id: &str) {
        self.0.lock().await.prompt_channels.remove(id);
    }

    // ── Themes ────────────────────────────────────────────────────────────────

    /// A JSON summary of all themes and the active selection (`theme.list`).
    pub async fn theme_list(&self) -> serde_json::Value {
        self.0.lock().await.themes.list_json()
    }

    /// The fully-resolved active theme as JSON (`theme.get`).
    pub async fn theme_active(&self) -> serde_json::Value {
        self.0.lock().await.themes.active().to_resolved_json()
    }

    /// Resolve a specific theme by id to JSON, if it exists.
    pub async fn theme_get(&self, id: &str) -> Option<serde_json::Value> {
        self.0
            .lock()
            .await
            .themes
            .get(id)
            .map(enzo_theme::Theme::to_resolved_json)
    }

    /// Set the active theme. Returns the resolved theme JSON on success.
    pub async fn theme_apply(&self, id: &str) -> anyhow::Result<serde_json::Value> {
        let mut inner = self.0.lock().await;
        inner.themes.set_active(id)?;
        Ok(inner.themes.active().to_resolved_json())
    }

    // ── DB query tabs ─────────────────────────────────────────────────────────

    /// Run `f` against the [`TabManager`] for connection `conn`, creating it on
    /// first use, and return the closure's result.
    pub async fn with_db_tabs<R>(&self, conn: &str, f: impl FnOnce(&mut TabManager) -> R) -> R {
        let mut inner = self.0.lock().await;
        let tabs = inner.db_tabs.entry(conn.to_owned()).or_default();
        f(tabs)
    }

    /// Drop the tab manager for a closed connection.
    pub async fn remove_db_tabs(&self, conn: &str) {
        self.0.lock().await.db_tabs.remove(conn);
    }
}

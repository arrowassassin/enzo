//! Shared daemon state — cloneable handle backed by an Arc<Mutex<Inner>>.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::session::{Session, SessionId};

/// Thread-safe, cheaply-cloneable handle to all live sessions.
#[derive(Clone)]
pub struct DaemonState(Arc<Mutex<Inner>>);

struct Inner {
    sessions: HashMap<SessionId, Session>,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonState {
    /// Create an empty state with no active sessions.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Inner {
            sessions: HashMap::new(),
        })))
    }

    /// Insert a session, replacing any existing session with the same id.
    pub async fn insert_session(&self, session: Session) {
        self.0
            .lock()
            .await
            .sessions
            .insert(session.id.clone(), session);
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
}

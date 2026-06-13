//! A terminal session: a PTY pair + the child process running inside it.

use std::sync::Mutex;

use portable_pty::{Child, MasterPty, PtySize};

/// Unique session identifier (client-assigned, e.g. a UUID string).
pub type SessionId = String;

/// An active terminal session: a PTY pair + the child shell process.
pub struct Session {
    /// Client-assigned identifier, e.g. a UUID string.
    pub id: SessionId,
    /// Write half — send bytes to the child's stdin.
    writer: Mutex<Box<dyn std::io::Write + Send>>,
    /// The PTY master (used for resize).
    master: Mutex<Box<dyn MasterPty + Send>>,
    /// The child process.
    #[allow(dead_code)]
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

impl Session {
    /// Create a new session from the PTY components returned by [`crate::pty::spawn_session`].
    #[must_use]
    pub fn new(
        id: SessionId,
        master: Box<dyn MasterPty + Send>,
        writer: Box<dyn std::io::Write + Send>,
        child: Box<dyn Child + Send + Sync>,
    ) -> Self {
        Self {
            id,
            writer: Mutex::new(writer),
            master: Mutex::new(master),
            child: Mutex::new(child),
        }
    }

    /// Resize the PTY. Called while holding the state lock — must not block.
    pub fn resize_sync(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .lock()
            .expect("master mutex poisoned")
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())
    }

    /// Write bytes to stdin. Called while holding the state lock — fast I/O.
    pub fn write_stdin_sync(&self, data: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        self.writer
            .lock()
            .expect("writer mutex poisoned")
            .write_all(data)
    }
}

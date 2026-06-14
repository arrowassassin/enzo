//! A terminal session: a PTY pair + the child shell process.

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
    /// Read half — receive bytes from the child's stdout/stderr.
    pub reader: Mutex<Box<dyn std::io::Read + Send>>,
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
        reader: Box<dyn std::io::Read + Send>,
        writer: Box<dyn std::io::Write + Send>,
        child: Box<dyn Child + Send + Sync>,
    ) -> Self {
        Self {
            id,
            writer: Mutex::new(writer),
            reader: Mutex::new(reader),
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

    /// Take the PTY reader out of the session for use in a background read task.
    ///
    /// Replaces the reader with an EOF stub so a second call returns EOF immediately.
    /// The caller should call this exactly once after spawning the session.
    pub fn take_reader(&self) -> Option<Box<dyn std::io::Read + Send>> {
        let mut guard = self.reader.lock().expect("reader mutex poisoned");
        Some(std::mem::replace(&mut *guard, Box::new(EofReader)))
    }
}

/// Stub reader that immediately returns EOF — installed after the real reader is taken.
struct EofReader;

impl std::io::Read for EofReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

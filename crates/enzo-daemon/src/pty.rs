//! PTY spawn helpers.

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::error::DaemonError;
use crate::session::Session;

/// Spawn a new PTY session running `shell` (defaults to `$SHELL` or `/bin/sh`).
pub fn spawn_session(
    id: String,
    shell: Option<&str>,
    cols: u16,
    rows: u16,
) -> Result<Session, DaemonError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| DaemonError::Pty(e.to_string()))?;

    let shell = shell
        .map(str::to_owned)
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/sh".to_owned());

    let mut cmd = CommandBuilder::new(&shell);
    cmd.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| DaemonError::Pty(e.to_string()))?;

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| DaemonError::Pty(e.to_string()))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| DaemonError::Pty(e.to_string()))?;

    Ok(Session::new(id, pair.master, reader, writer, child))
}

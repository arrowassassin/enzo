//! Daemon-level error type.

use thiserror::Error;

#[derive(Debug, Error)]
/// Daemon-level error returned by PTY, vault, and I/O operations.
pub enum DaemonError {
    /// PTY creation or I/O failure.
    #[error("PTY error: {0}")]
    Pty(String),
    /// Credential vault error.
    #[error("vault error: {0}")]
    Vault(#[from] enzo_vault::VaultError),
    /// Underlying OS I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

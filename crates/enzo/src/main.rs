//! Enzo orchestrator — boots `enzo-daemon` then `enzo-client` in one command.
//!
//! # Startup sequence
//! 1. Check whether `enzo-daemon` is already listening on the ATP socket.
//! 2. If not, locate and spawn `enzo-daemon` as a child process.
//! 3. Poll the socket until the daemon is ready (up to 5 s).
//! 4. Spawn `enzo-client` and wait for it to exit.
//! 5. Terminate the daemon child (if we started it) and exit.

use std::path::PathBuf;
use std::process::{Child, Command, ExitCode};
use std::time::{Duration, Instant};

/// ATP socket path (mirrors `enzo_daemon::DEFAULT_SOCK`).
const SOCK: &str = "/tmp/enzo-atp.sock";
/// How long to wait for the daemon to start listening.
const DAEMON_TIMEOUT: Duration = Duration::from_secs(5);
/// Poll interval while waiting for the socket.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("enzo: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    // Optionally override socket path via environment variable.
    let sock = std::env::var("ENZO_ATP_SOCK").unwrap_or_else(|_| SOCK.to_owned());

    // Only boot the daemon if it's not already running.
    let daemon_child: Option<Child> = if probe_socket(&sock) {
        None
    } else {
        let daemon = find_sibling("enzo-daemon");
        let child = Command::new(&daemon)
            .spawn()
            .map_err(|e| anyhow::anyhow!("start enzo-daemon ({}): {e}", daemon.display()))?;
        Some(child)
    };

    // Wait for the socket to become available.
    if daemon_child.is_some() {
        wait_for_socket(&sock)?;
    }

    // Run the GPU client — blocks until the window is closed.
    let client = find_sibling("enzo-client");
    let status = Command::new(&client)
        .status()
        .map_err(|e| anyhow::anyhow!("start enzo-client ({}): {e}", client.display()))?;

    // Clean up daemon if we started it.
    if let Some(mut child) = daemon_child {
        child.kill().ok();
        child.wait().ok();
    }

    if status.success() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(
            u8::try_from(status.code().unwrap_or(1)).unwrap_or(1),
        ))
    }
}

/// Try a non-blocking connect to the ATP socket; returns `true` if it succeeds.
fn probe_socket(path: &str) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// Poll `path` until a connection succeeds or `DAEMON_TIMEOUT` elapses.
fn wait_for_socket(path: &str) -> anyhow::Result<()> {
    let deadline = Instant::now() + DAEMON_TIMEOUT;
    loop {
        if probe_socket(path) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "enzo-daemon did not start within {}s (socket: {path})",
                DAEMON_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Locate `name` next to the current executable, falling back to PATH.
fn find_sibling(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(name)
}

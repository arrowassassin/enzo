//! `enzo-claude` — Claude Code adapter for the enzo terminal.
//!
//! Wraps the `claude` CLI in a PTY, proxies stdio, and surfaces tool-call
//! approval prompts as interactive ATP blocks in the GPU renderer.
//!
//! # Usage
//!
//! ```text
//! enzo-claude [claude-args…]
//! ```
//!
//! # How it works
//!
//! 1. Connects to `$ENZO_ATP_SOCK` (if present).
//! 2. Spawns `claude <args>` inside a PTY at the current terminal size.
//! 3. Forwards the user's stdin → PTY and PTY stdout → the user's terminal.
//! 4. Passes each output line through [`PromptDetector`].
//! 5. When a tool-approval prompt is detected:
//!    a. Send `prompt.show` over ATP (blocks until the user decides).
//!    b. On **accept**: inject `y\r` into Claude Code's PTY stdin.
//!    c. On **reject**: inject `n\r`.
//!    d. On **edit**: leave the PTY in its current state (user takes over).
//! 6. If no ATP socket is available, fall through transparently — the user
//!    sees a normal Claude Code session (Layer 0 compatibility).

#![allow(clippy::print_stderr)] // expected for a CLI binary

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use enzo_adapters::atp::{AtpClient, try_connect};
use enzo_adapters::detect::{DetectedPrompt, PromptDetector};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sock = std::env::var("ENZO_ATP_SOCK").unwrap_or_else(|_| "/tmp/enzo-atp.sock".to_owned());

    // `enzo-claude --demo` exercises the full adapter → daemon → display → respond
    // path with a sample diff, independent of Claude Code. Use it to verify the
    // approval card renders and the buttons work.
    if args.iter().any(|a| a == "--demo") {
        return run_demo(&sock);
    }

    // Optional ATP connection — adapts gracefully if the daemon is not running.
    let atp: Option<Arc<Mutex<AtpClient>>> = try_connect(&sock).map(|c| Arc::new(Mutex::new(c)));

    let (cols, rows) = terminal_size();

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("open PTY")?;

    let mut cmd = CommandBuilder::new("claude");
    for arg in &args {
        cmd.arg(arg);
    }
    cmd.env("ENZO_ATP_SOCK", &sock);
    cmd.env("ENZO_ADAPTER", "claude");

    let _child = pair.slave.spawn_command(cmd).context("spawn claude")?;
    drop(pair.slave);

    let pty_writer: Arc<Mutex<Box<dyn Write + Send>>> =
        Arc::new(Mutex::new(pair.master.take_writer().context("PTY writer")?));
    let mut pty_reader = pair.master.try_clone_reader().context("PTY reader")?;

    // Forward stdin → PTY master in a background thread.
    let pw = Arc::clone(&pty_writer);
    std::thread::Builder::new()
        .name("stdin-fwd".into())
        .spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let mut buf = [0u8; 256];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if pw
                            .lock()
                            .expect("pty writer lock")
                            .write_all(&buf[..n])
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        })
        .context("spawn stdin thread")?;

    // PTY master → stdout: read, detect prompts, write.
    let mut stdout = std::io::stdout();
    let mut detector = PromptDetector::new(40);
    let mut line_buf: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];

    loop {
        match pty_reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let chunk = &buf[..n];
                stdout.write_all(chunk).ok();
                stdout.flush().ok();

                for &byte in chunk {
                    if byte == b'\n' || byte == b'\r' {
                        if !line_buf.is_empty() {
                            let line = String::from_utf8_lossy(&line_buf).to_string();
                            if let Some(prompt) = detector.push(&line) {
                                handle_prompt(&prompt, atp.as_ref(), &pty_writer);
                                detector.clear();
                            }
                            line_buf.clear();
                        }
                    } else {
                        line_buf.push(byte);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Send one sample approval prompt over ATP and report the user's choice.
fn run_demo(sock: &str) -> anyhow::Result<()> {
    let Some(client) = try_connect(sock) else {
        eprintln!("[enzo-claude] no daemon at {sock} — start enzo first");
        return Ok(());
    };
    let client = Arc::new(Mutex::new(client));
    let diff = json!({
        "path": "src/renderer.rs",
        "raw": "@@ -10,3 +10,3 @@ impl Renderer {\n     fn present(&mut self) {\n-        self.redraw_all();\n+        let dirty = self.damage.take();\n+        self.gpu.draw(dirty);\n         self.swap_buffers();\n     }",
    });
    println!("[enzo-claude] sending demo decision card — respond in the enzo window…");
    let action = client.lock().expect("lock").prompt_show(
        "demo-claude",
        "diff",
        "claude wants to edit renderer.rs",
        "Replace full redraw with damage-tracked draw",
        Some(&diff),
        &["accept", "reject", "edit"],
    )?;
    println!("[enzo-claude] you chose: {action}");
    Ok(())
}

/// Surface `prompt` as an ATP approval card (if connected) or do nothing
/// so the PTY prompt remains visible for direct terminal interaction.
fn handle_prompt(
    prompt: &DetectedPrompt,
    atp: Option<&Arc<Mutex<AtpClient>>>,
    pty_writer: &Arc<Mutex<Box<dyn Write + Send>>>,
) {
    let Some(client) = atp else { return };

    let prompt_id = unique_id();
    let diff_val = prompt
        .diff
        .as_ref()
        .map(|d| json!({ "path": d.path, "raw": d.raw }));

    // Multi-option menus surface every option as a button; plain prompts keep
    // the yes/no/edit triple.
    let actions: Vec<String> = if prompt.options.is_empty() {
        vec!["accept".to_owned(), "reject".to_owned(), "edit".to_owned()]
    } else {
        prompt.options.clone()
    };
    let action_refs: Vec<&str> = actions.iter().map(String::as_str).collect();

    let action = match client.lock().expect("atp lock").prompt_show(
        &prompt_id,
        if prompt.diff.is_some() { "diff" } else { "text" },
        &prompt.title,
        &prompt.body,
        diff_val.as_ref(),
        &action_refs,
    ) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[enzo-claude] ATP prompt.show failed: {e}");
            return; // leave the PTY prompt visible for the user
        }
    };

    let keystroke: Vec<u8> = if prompt.options.is_empty() {
        match action.as_str() {
            "accept" => b"y\r".to_vec(),
            "reject" => b"n\r".to_vec(),
            "edit" => return, // leave the cursor at the prompt for manual input
            other => {
                eprintln!("[enzo-claude] unknown action '{other}', treating as reject");
                b"n\r".to_vec()
            }
        }
    } else {
        // Arrow-driven menu: step down to the chosen option (menus default to
        // the first item highlighted), then Enter.
        let idx = prompt.options.iter().position(|o| *o == action).unwrap_or(0);
        let mut k = Vec::new();
        for _ in 0..idx {
            k.extend_from_slice(b"\x1b[B"); // cursor-down
        }
        k.extend_from_slice(b"\r");
        k
    };

    if let Err(e) = pty_writer
        .lock()
        .expect("pty writer lock")
        .write_all(&keystroke)
    {
        eprintln!("[enzo-claude] PTY write error: {e}");
    }
}

/// Generate a unique prompt id from the process id and a monotonic counter.
fn unique_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    format!(
        "claude-{}-{}",
        std::process::id(),
        CTR.fetch_add(1, Ordering::Relaxed)
    )
}

/// Query terminal dimensions from environment variables, falling back to 80×24.
///
/// `$COLUMNS` / `$LINES` are set by most shells and are preferable to an
/// ioctl (which would require unsafe code).
fn terminal_size() -> (u16, u16) {
    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80_u16);
    let rows = std::env::var("LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24_u16);
    (cols, rows)
}

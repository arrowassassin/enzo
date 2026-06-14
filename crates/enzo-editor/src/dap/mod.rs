//! Debug Adapter Protocol client.
//!
//! Spawns a DAP adapter process and communicates over its stdin/stdout using
//! the Content-Length framed protocol.  Supports `initialize`, `launch`,
//! `setBreakpoints`, `continue`, `next`, `stepIn`, `stepOut`, `stackTrace`,
//! and `disconnect`.

mod transport;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, oneshot};

pub use transport::DapEvent;

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// A live connection to a debug adapter process.
pub struct DapClient {
    writer: Arc<Mutex<tokio::process::ChildStdin>>,
    pending: PendingMap,
    next_seq: Arc<AtomicU64>,
    _child: Child,
}

impl DapClient {
    /// Spawn `cmd` with `args` and connect to its stdio.
    ///
    /// `on_event` is called for every event the adapter emits (stopped,
    /// breakpoint, output, terminated, etc.) from a background task.
    pub fn spawn(
        cmd: &str,
        args: &[&str],
        on_event: impl FnMut(DapEvent) + Send + 'static,
    ) -> anyhow::Result<Self> {
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {cmd}"))?;

        let stdin = child.stdin.take().context("stdin")?;
        let stdout = child.stdout.take().context("stdout")?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_r = Arc::clone(&pending);
        let writer = Arc::new(Mutex::new(stdin));

        tokio::spawn(async move {
            if let Err(e) = transport::read_loop(stdout, pending_r, on_event).await {
                log::warn!("DAP read loop: {e:#}");
            }
        });

        Ok(Self {
            writer,
            pending,
            next_seq: Arc::new(AtomicU64::new(1)),
            _child: child,
        })
    }

    /// Send a DAP request and wait for the response body.
    pub async fn request(&self, command: &str, args: Value) -> anyhow::Result<Value> {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel::<Value>();
        self.pending.lock().await.insert(seq, tx);

        let msg = json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": args,
        });
        transport::write_message(&mut *self.writer.lock().await, &msg).await?;

        let resp = rx.await.context("response channel closed")?;
        if !resp["success"].as_bool().unwrap_or(false) {
            let msg = resp["message"].as_str().unwrap_or("unknown error");
            anyhow::bail!("DAP error: {msg}");
        }
        Ok(resp["body"].clone())
    }

    // ── High-level helpers ───────────────────────────────────────────────────

    /// Send `initialize` with the given adapter ID.
    pub async fn initialize(&self, adapter_id: &str) -> anyhow::Result<Value> {
        self.request(
            "initialize",
            json!({
                "clientID": "enzo",
                "clientName": "enzo",
                "adapterID": adapter_id,
                "pathFormat": "path",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "supportsRunInTerminalRequest": false,
                "supportsProgressReporting": false,
            }),
        )
        .await
    }

    /// Launch a program under the debugger.
    pub async fn launch(&self, program: &str, args: &[&str]) -> anyhow::Result<Value> {
        self.request(
            "launch",
            json!({
                "noDebug": false,
                "program": program,
                "args": args,
            }),
        )
        .await
    }

    /// Set breakpoints for one source file (replaces all previous breakpoints).
    pub async fn set_breakpoints(&self, path: &str, lines: &[u32]) -> anyhow::Result<Value> {
        let bps: Vec<Value> = lines.iter().map(|&l| json!({ "line": l })).collect();
        self.request(
            "setBreakpoints",
            json!({
                "source": { "path": path },
                "breakpoints": bps,
            }),
        )
        .await
    }

    /// Continue execution of a thread (`threadId = 0` means all threads).
    pub async fn continue_exec(&self, thread_id: u64) -> anyhow::Result<Value> {
        self.request("continue", json!({ "threadId": thread_id }))
            .await
    }

    /// Step over (next line) in a thread.
    pub async fn next(&self, thread_id: u64) -> anyhow::Result<Value> {
        self.request("next", json!({ "threadId": thread_id })).await
    }

    /// Step into a call.
    pub async fn step_in(&self, thread_id: u64) -> anyhow::Result<Value> {
        self.request("stepIn", json!({ "threadId": thread_id }))
            .await
    }

    /// Step out of the current frame.
    pub async fn step_out(&self, thread_id: u64) -> anyhow::Result<Value> {
        self.request("stepOut", json!({ "threadId": thread_id }))
            .await
    }

    /// Retrieve the stack trace for a thread.
    pub async fn stack_trace(&self, thread_id: u64) -> anyhow::Result<Value> {
        self.request(
            "stackTrace",
            json!({ "threadId": thread_id, "startFrame": 0, "levels": 20 }),
        )
        .await
    }

    /// List local variables in a scope.
    pub async fn variables(&self, variables_reference: u64) -> anyhow::Result<Value> {
        self.request(
            "variables",
            json!({ "variablesReference": variables_reference }),
        )
        .await
    }

    /// Disconnect from the adapter (optionally terminating the debuggee).
    pub async fn disconnect(&self, terminate: bool) -> anyhow::Result<Value> {
        self.request(
            "disconnect",
            json!({ "restart": false, "terminateDebuggee": terminate }),
        )
        .await
    }
}

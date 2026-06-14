//! Language Server Protocol client.
//!
//! Spawns an LSP server process and communicates over its stdin/stdout using
//! the Content-Length framed JSON-RPC 2.0 protocol defined by the LSP spec.
//!
//! # Example
//! ```no_run
//! use enzo_editor::lsp::LspClient;
//! let client = LspClient::spawn("rust-analyzer", &[], |_| {}).unwrap();
//! // client.initialize(...).await within an async context
//! ```

mod transport;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, oneshot};

pub use transport::LspNotification;

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// A live connection to a language server process.
pub struct LspClient {
    writer: Arc<Mutex<tokio::process::ChildStdin>>,
    pending: PendingMap,
    next_id: Arc<AtomicU64>,
    /// The server process — held so it is not dropped and killed prematurely.
    _child: Child,
}

impl LspClient {
    /// Spawn `cmd` with `args` and connect to its stdio.
    ///
    /// `on_notification` is called for every server-initiated notification
    /// (diagnostics, progress, etc.) from a background task.
    pub fn spawn(
        cmd: &str,
        args: &[&str],
        on_notification: impl FnMut(LspNotification) + Send + 'static,
    ) -> anyhow::Result<Self> {
        use tokio::process::ChildStdout;

        let mut child = Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {cmd}"))?;

        let stdin: tokio::process::ChildStdin = child.stdin.take().context("stdin")?;
        let stdout: ChildStdout = child.stdout.take().context("stdout")?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_r = Arc::clone(&pending);
        let writer = Arc::new(Mutex::new(stdin));

        tokio::spawn(async move {
            if let Err(e) = transport::read_loop(stdout, pending_r, on_notification).await {
                log::warn!("LSP read loop: {e:#}");
            }
        });

        Ok(Self {
            writer,
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
            _child: child,
        })
    }

    /// Send a request and wait for the result value.
    pub async fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel::<Value>();
        self.pending.lock().await.insert(id, tx);

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        transport::write_message(&mut *self.writer.lock().await, &msg).await?;

        let resp = rx.await.context("response channel closed")?;
        if let Some(err) = resp.get("error") {
            anyhow::bail!("LSP error: {err}");
        }
        Ok(resp["result"].clone())
    }

    /// Send a notification (no response expected).
    pub async fn notify(&self, method: &str, params: Value) -> anyhow::Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        transport::write_message(&mut *self.writer.lock().await, &msg).await
    }

    // ── High-level helpers ───────────────────────────────────────────────────

    /// Send `initialize` and then `initialized`.
    pub async fn initialize(&self, root_uri: &str) -> anyhow::Result<Value> {
        let result = self
            .request(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "rootUri": root_uri,
                    "capabilities": {
                        "textDocument": {
                            "completion": { "completionItem": { "snippetSupport": false } },
                            "hover": { "contentFormat": ["plaintext"] },
                            "definition": {},
                            "references": {},
                            "diagnostic": { "dynamicRegistration": false },
                            "publishDiagnostics": { "relatedInformation": true }
                        },
                        "workspace": {
                            "applyEdit": false,
                            "didChangeWatchedFiles": { "dynamicRegistration": false }
                        }
                    },
                    "clientInfo": { "name": "enzo", "version": "0.1.0" }
                }),
            )
            .await?;
        self.notify("initialized", json!({})).await?;
        Ok(result)
    }

    /// Notify the server that a file was opened.
    pub async fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i32,
        text: &str,
    ) -> anyhow::Result<()> {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": version,
                    "text": text
                }
            }),
        )
        .await
    }

    /// Notify the server that a file changed (full sync).
    pub async fn did_change(&self, uri: &str, version: i32, text: &str) -> anyhow::Result<()> {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }]
            }),
        )
        .await
    }

    /// Request completions at a position.
    pub async fn completion(&self, uri: &str, line: u32, character: u32) -> anyhow::Result<Value> {
        self.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
        .await
    }

    /// Request hover information at a position.
    pub async fn hover(&self, uri: &str, line: u32, character: u32) -> anyhow::Result<Value> {
        self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
        .await
    }

    /// Request go-to-definition.
    pub async fn definition(&self, uri: &str, line: u32, character: u32) -> anyhow::Result<Value> {
        self.request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
        .await
    }

    /// Send `shutdown` then `exit`.
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.request("shutdown", json!(null)).await?;
        self.notify("exit", json!(null)).await
    }
}

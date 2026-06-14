//! ATP connectivity for the GPUI client.
//!
//! Mirrors the daemon-facing logic of the legacy egui client: a dedicated
//! background thread owns a tokio runtime + the JSON-RPC connection, and
//! exchanges plain values with the GPUI thread over `std::sync::mpsc` channels
//! (commands out, [`Incoming`] events in). The GPUI side drains `incoming`
//! each tick and updates entity state — the render/input path never blocks.
//!
//! Some protocol messages (resize, agent prompt/block) are defined but not yet
//! wired to a surface; those fields/variants are intentionally retained.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use anyhow::Context as _;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, mpsc, oneshot};

/// Default daemon socket path (override with `ENZO_ATP_SOCK`).
pub const DEFAULT_SOCK: &str = "/tmp/enzo-atp.sock";

/// One table/view in a connection's schema.
#[derive(Clone, Debug)]
pub struct TableInfo {
    pub name: String,
    pub kind: String,
}

/// One column in a table's schema (for the expandable catalog).
#[derive(Clone, Debug)]
pub struct ColumnMeta {
    pub name: String,
    pub sql_type: String,
    pub primary_key: bool,
    pub not_null: bool,
}

/// One LSP diagnostic (a squiggle), in LSP 0-based line/character coordinates.
#[derive(Clone, Debug)]
pub struct DiagItem {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub message: String,
    /// LSP severity: 1=Error, 2=Warning, 3=Info, 4=Hint.
    pub severity: u8,
}

/// A debug call-stack frame.
#[derive(Clone, Debug)]
pub struct StackFrame {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub line: u32,
}

/// A scope (Locals/Globals/…) at a stack frame.
#[derive(Clone, Debug)]
pub struct DapScope {
    pub name: String,
    pub reference: u64,
}

/// A debugger variable (name/value/type + child reference for expansion).
#[derive(Clone, Debug)]
pub struct DapVar {
    pub name: String,
    pub value: String,
    pub ty: String,
    pub reference: u64,
}

/// Which step a debug step command performs.
#[derive(Clone, Copy, Debug)]
pub enum DapStepKind {
    Over,
    In,
    Out,
}

/// A tree-sitter highlight span (byte range + capture name).
#[derive(Clone, Debug)]
pub struct HlSpan {
    pub start: usize,
    pub end: usize,
    pub name: String,
}

/// One `git status` entry.
#[derive(Clone, Debug)]
pub struct GitEntry {
    pub path: String,
    pub state: String,
    pub staged: bool,
}

/// UI → daemon requests (fire-and-forget from the GPUI thread).
#[derive(Debug)]
pub enum Command {
    DbConnect {
        conn: String,
        path: String,
        /// Driver to use (`"sqlite"` | `"duckdb"`; empty → inferred from path).
        driver: String,
        seed: bool,
    },
    DbQuery {
        conn: String,
        sql: String,
    },
    DbBrowse {
        conn: String,
        table: String,
        page: u64,
        size: u64,
    },
    /// Fetch the column schema of `table` (for the expandable catalog).
    DbColumns {
        conn: String,
        table: String,
    },
    DbUpdate {
        conn: String,
        table: String,
        /// `(column, value)` pairs to set.
        cells: Vec<(String, String)>,
        /// `(column, value)` pairs identifying the row (primary key).
        pk: Vec<(String, String)>,
    },
    NewSession {
        id: String,
        cols: u16,
        rows: u16,
    },
    Input {
        id: String,
        data: Vec<u8>,
    },
    Resize {
        id: String,
        cols: u16,
        rows: u16,
    },
    PromptRespond {
        id: String,
        action: String,
    },
    /// Request tree-sitter highlight spans for `source` (`path` echoes back so
    /// the client can drop stale responses).
    Highlight {
        path: String,
        language: String,
        source: String,
    },
    BrowserLaunch {
        id: String,
        width: u32,
        height: u32,
    },
    BrowserNavigate {
        id: String,
        url: String,
    },
    /// Request a PNG screenshot of the page → [`Incoming::BrowserShot`].
    BrowserShot {
        id: String,
    },
    /// Generic CDP passthrough (mouse/keyboard input, screencast control, …).
    /// `method`/`params` are forwarded verbatim to the page's CDP session.
    BrowserInput {
        id: String,
        method: String,
        params: serde_json::Value,
    },
    GitStatus {
        root: String,
    },
    GitStage {
        root: String,
        file: String,
        unstage: bool,
    },
    GitCommit {
        root: String,
        message: String,
    },
    /// Start a language server (`id` per language) and run the LSP `initialize`
    /// handshake rooted at `root_uri`. Processed before any later `LspDidOpen`.
    LspStart {
        id: String,
        cmd: String,
        args: Vec<String>,
        root_uri: String,
    },
    /// Notify the server that `uri` was opened (`textDocument/didOpen`).
    LspDidOpen {
        id: String,
        uri: String,
        language_id: String,
        version: i64,
        text: String,
    },
    /// Notify the server of a full-text edit (`textDocument/didChange`).
    LspDidChange {
        id: String,
        uri: String,
        version: i64,
        text: String,
    },
    /// Start a debug adapter and run initialize → launch (deferred response).
    DapStart {
        id: String,
        cmd: String,
        args: Vec<String>,
        adapter_id: String,
        launch: serde_json::Value,
    },
    DapSetBreakpoints {
        id: String,
        path: String,
        lines: Vec<u32>,
    },
    DapConfigDone {
        id: String,
    },
    DapStackTrace {
        id: String,
        thread_id: u64,
    },
    DapScopes {
        id: String,
        frame_id: u64,
    },
    DapVariables {
        id: String,
        reference: u64,
    },
    DapContinue {
        id: String,
        thread_id: u64,
    },
    DapStep {
        id: String,
        thread_id: u64,
        kind: DapStepKind,
    },
    DapStop {
        id: String,
    },
}

/// daemon → UI events, drained by the GPUI thread.
#[derive(Debug)]
pub enum Incoming {
    Connected,
    Closed,
    DbConnected {
        conn: String,
        driver: String,
    },
    DbTables {
        conn: String,
        tables: Vec<TableInfo>,
    },
    DbResult {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        ms: u64,
        total: Option<u64>,
        page: u64,
        browsing: Option<String>,
        /// Primary-key column names (empty for ad-hoc queries → not editable).
        pk_columns: Vec<String>,
    },
    DbColumns {
        conn: String,
        table: String,
        columns: Vec<ColumnMeta>,
    },
    DbError {
        message: String,
    },
    Output {
        session_id: String,
        data: Vec<u8>,
    },
    BrowserShot {
        png: Vec<u8>,
    },
    /// A live screencast frame (JPEG) plus the CDP `session_id` to ack.
    BrowserFrame {
        jpeg: Vec<u8>,
        session_id: i64,
    },
    /// A browser launch/navigate/screenshot failed (e.g. Chrome not installed).
    BrowserError {
        message: String,
    },
    GitStatus {
        branch: String,
        entries: Vec<GitEntry>,
    },
    Highlight {
        path: String,
        spans: Vec<HlSpan>,
    },
    PromptShow {
        id: String,
        title: String,
        body: String,
        actions: Vec<String>,
    },
    BlockPush {
        id: String,
        title: String,
        body: String,
    },
    BlockClear {
        id: String,
    },
    /// Diagnostics for `uri` from `textDocument/publishDiagnostics`.
    LspDiagnostics {
        uri: String,
        items: Vec<DiagItem>,
    },
    /// The adapter is ready for breakpoint configuration (`initialized` event).
    DapInitialized,
    /// Execution stopped (breakpoint/step/entry) on `thread_id`.
    DapStopped {
        thread_id: u64,
        reason: String,
    },
    DapContinued,
    DapOutput {
        category: String,
        text: String,
    },
    DapTerminated,
    DapStackTraceResult {
        frames: Vec<StackFrame>,
    },
    DapScopesResult {
        scopes: Vec<DapScope>,
    },
    DapVariablesResult {
        reference: u64,
        vars: Vec<DapVar>,
    },
}

/// Handle to the background ATP thread.
pub struct Atp {
    pub commands: mpsc::UnboundedSender<Command>,
    pub incoming: Receiver<Incoming>,
}

/// Spawn the background ATP thread and return a handle. Never blocks.
pub fn connect() -> Atp {
    let sock = std::env::var("ENZO_ATP_SOCK").unwrap_or_else(|_| DEFAULT_SOCK.to_owned());
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();
    let (in_tx, in_rx) = std::sync::mpsc::channel::<Incoming>();

    std::thread::Builder::new()
        .name("enzo-atp".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(run(sock, in_tx, cmd_rx));
        })
        .expect("spawn atp thread");

    Atp {
        commands: cmd_tx,
        incoming: in_rx,
    }
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// The async client: writer + pending response map.
#[derive(Clone)]
struct Client {
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    pending: PendingMap,
    next_id: Arc<Mutex<u64>>,
}

impl Client {
    async fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = {
            let mut n = self.next_id.lock().await;
            let id = *n;
            *n += 1;
            id
        };
        let (tx, rx) = oneshot::channel::<Value>();
        self.pending.lock().await.insert(id, tx);
        let mut line = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        }))
        .context("serialize")?;
        line.push('\n');
        self.writer
            .lock()
            .await
            .write_all(line.as_bytes())
            .await
            .context("write")?;
        let resp = rx.await.context("response channel closed")?;
        if let Some(err) = resp.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| err.to_string(), str::to_owned);
            anyhow::bail!("{msg}");
        }
        Ok(resp["result"].clone())
    }
}

/// Background entry point: connect (retrying so a late-starting daemon is
/// picked up), then run the command loop. Buffered commands (e.g. the initial
/// `DbConnect`) are processed once the connection is established.
async fn run(sock: String, tx: Sender<Incoming>, mut cmd_rx: mpsc::UnboundedReceiver<Command>) {
    let stream = loop {
        match UnixStream::connect(&sock).await {
            Ok(s) => break s,
            Err(e) => {
                log::debug!("ATP connect {sock}: {e}; retrying");
                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
            }
        }
    };
    let (reader, writer) = stream.into_split();
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let client = Client {
        writer: Arc::new(Mutex::new(writer)),
        pending: Arc::clone(&pending),
        next_id: Arc::new(Mutex::new(1)),
    };

    let notify_tx = tx.clone();
    tokio::spawn(async move {
        if let Err(e) = read_loop(reader, pending, notify_tx).await {
            log::warn!("ATP read loop ended: {e:#}");
        }
    });

    let _ = client.request("display.register", json!({})).await;
    let _ = tx.send(Incoming::Connected);

    while let Some(cmd) = cmd_rx.recv().await {
        handle_command(&client, &tx, cmd).await;
    }
}

#[allow(clippy::too_many_lines)]
async fn handle_command(client: &Client, tx: &Sender<Incoming>, cmd: Command) {
    match cmd {
        Command::DbConnect {
            conn,
            path,
            driver,
            seed,
        } => {
            match client
                .request(
                    "db.connect",
                    json!({ "id": conn, "path": path, "driver": driver }),
                )
                .await
            {
                Ok(r) => {
                    let driver = r["driver"].as_str().unwrap_or("sqlite").to_owned();
                    if seed {
                        seed_demo(client, &conn).await;
                    }
                    let _ = tx.send(Incoming::DbConnected {
                        conn: conn.clone(),
                        driver,
                    });
                    if let Ok(t) = client
                        .request("db.schema.tables", json!({ "conn": conn }))
                        .await
                    {
                        let tables = t["tables"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .map(|x| TableInfo {
                                        name: x["name"].as_str().unwrap_or_default().to_owned(),
                                        kind: x["kind"].as_str().unwrap_or("table").to_owned(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let _ = tx.send(Incoming::DbTables { conn, tables });
                    }
                }
                Err(e) => {
                    let _ = tx.send(Incoming::DbError {
                        message: e.to_string(),
                    });
                }
            }
        }
        Command::DbQuery { conn, sql } => {
            let started = std::time::Instant::now();
            match client
                .request("db.query", json!({ "conn": conn, "sql": sql }))
                .await
            {
                Ok(r) => {
                    let (columns, rows) = parse_cols_rows(&r);
                    let _ = tx.send(Incoming::DbResult {
                        columns,
                        rows,
                        ms: elapsed_ms(started),
                        total: None,
                        page: 0,
                        browsing: None,
                        pk_columns: Vec::new(),
                    });
                }
                Err(e) => {
                    let _ = tx.send(Incoming::DbError {
                        message: e.to_string(),
                    });
                }
            }
        }
        Command::DbBrowse {
            conn,
            table,
            page,
            size,
        } => {
            let started = std::time::Instant::now();
            match client
                .request(
                    "db.table.browse",
                    json!({ "conn": conn, "table": table, "page": page, "size": size }),
                )
                .await
            {
                Ok(r) => {
                    let total = r["total"].as_u64().unwrap_or(0);
                    let (columns, rows) = parse_cols_rows(&r);
                    let pk_columns = fetch_pk_columns(client, &conn, &table).await;
                    let _ = tx.send(Incoming::DbResult {
                        columns,
                        rows,
                        ms: elapsed_ms(started),
                        total: Some(total),
                        page,
                        browsing: Some(table),
                        pk_columns,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Incoming::DbError {
                        message: e.to_string(),
                    });
                }
            }
        }
        Command::DbColumns { conn, table } => {
            if let Ok(r) = client
                .request("db.schema.columns", json!({ "conn": conn, "table": table }))
                .await
            {
                let columns = r["columns"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|c| ColumnMeta {
                                name: c["name"].as_str().unwrap_or_default().to_owned(),
                                sql_type: c["sql_type"].as_str().unwrap_or_default().to_owned(),
                                primary_key: c["primary_key"].as_bool().unwrap_or(false),
                                not_null: c["not_null"].as_bool().unwrap_or(false),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let _ = tx.send(Incoming::DbColumns {
                    conn,
                    table,
                    columns,
                });
            }
        }
        Command::DbUpdate {
            conn,
            table,
            cells,
            pk,
        } => {
            let to_pairs = |v: Vec<(String, String)>| -> Vec<Value> {
                v.into_iter()
                    .map(|(column, value)| json!({ "column": column, "value": value }))
                    .collect()
            };
            if let Err(e) = client
                .request(
                    "db.table.update",
                    json!({
                        "conn": conn,
                        "table": table,
                        "cells": to_pairs(cells),
                        "pk": to_pairs(pk),
                    }),
                )
                .await
            {
                let _ = tx.send(Incoming::DbError {
                    message: e.to_string(),
                });
            }
        }
        Command::NewSession { id, cols, rows } => {
            let _ = client
                .request(
                    "session.spawn",
                    json!({ "id": id, "cols": cols, "rows": rows }),
                )
                .await;
        }
        Command::Input { id, data } => {
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
            let _ = client
                .request("session.input", json!({ "id": id, "data": b64 }))
                .await;
        }
        Command::Resize { id, cols, rows } => {
            let _ = client
                .request(
                    "session.resize",
                    json!({ "id": id, "cols": cols, "rows": rows }),
                )
                .await;
        }
        Command::PromptRespond { id, action } => {
            let _ = client
                .request("prompt.respond", json!({ "id": id, "action": action }))
                .await;
        }
        Command::Highlight {
            path,
            language,
            source,
        } => {
            if let Ok(r) = client
                .request(
                    "editor.highlight",
                    json!({ "language": language, "source": source }),
                )
                .await
            {
                let spans = r["spans"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| {
                                Some(HlSpan {
                                    start: usize::try_from(s["start"].as_u64()?).ok()?,
                                    end: usize::try_from(s["end"].as_u64()?).ok()?,
                                    name: s["name"].as_str().unwrap_or_default().to_owned(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let _ = tx.send(Incoming::Highlight { path, spans });
            }
        }
        Command::BrowserLaunch { id, width, height } => {
            if let Err(e) = client
                .request(
                    "browser.launch",
                    json!({ "id": id, "width": width, "height": height }),
                )
                .await
            {
                let _ = tx.send(Incoming::BrowserError {
                    message: format!("launch failed: {e}"),
                });
            }
        }
        Command::BrowserNavigate { id, url } => {
            if let Err(e) = client
                .request("browser.navigate", json!({ "id": id, "url": url }))
                .await
            {
                let _ = tx.send(Incoming::BrowserError {
                    message: format!("navigate failed: {e}"),
                });
            }
        }
        Command::BrowserInput { id, method, params } => {
            // Fire-and-forget CDP call; errors (e.g. closed page) are non-fatal.
            let _ = client
                .request(
                    "browser.input",
                    json!({ "id": id, "method": method, "params": params }),
                )
                .await;
        }
        Command::BrowserShot { id } => match client
            .request("browser.screenshot", json!({ "id": id }))
            .await
        {
            Ok(r) => {
                if let Some(b64) = r["png"].as_str()
                    && let Ok(png) =
                        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
                {
                    let _ = tx.send(Incoming::BrowserShot { png });
                }
            }
            Err(e) => {
                let _ = tx.send(Incoming::BrowserError {
                    message: format!("screenshot failed: {e}"),
                });
            }
        },
        Command::GitStatus { root } => {
            send_git_status(client, tx, &root).await;
        }
        Command::GitStage {
            root,
            file,
            unstage,
        } => {
            let method = if unstage { "git.unstage" } else { "git.stage" };
            let _ = client
                .request(method, json!({ "path": root, "file": file }))
                .await;
            send_git_status(client, tx, &root).await;
        }
        Command::GitCommit { root, message } => {
            if client
                .request("git.commit", json!({ "path": root, "message": message }))
                .await
                .is_err()
            {
                // surface nothing special; a re-status reflects the result
            }
            send_git_status(client, tx, &root).await;
        }
        Command::LspStart {
            id,
            cmd,
            args,
            root_uri,
        } => {
            // Spawn the server; if the binary is missing this fails and we just
            // skip LSP for this language (the editor still works, no squiggles).
            if client
                .request("lsp.start", json!({ "id": id, "cmd": cmd, "args": args }))
                .await
                .is_err()
            {
                return;
            }
            let init = json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {
                    "textDocument": {
                        "synchronization": { "didSave": false, "dynamicRegistration": false },
                        "publishDiagnostics": { "relatedInformation": false }
                    }
                }
            });
            let _ = client
                .request(
                    "lsp.request",
                    json!({ "id": id, "method": "initialize", "params": init }),
                )
                .await;
            let _ = client
                .request(
                    "lsp.notify",
                    json!({ "id": id, "method": "initialized", "params": {} }),
                )
                .await;
        }
        Command::LspDidOpen {
            id,
            uri,
            language_id,
            version,
            text,
        } => {
            let params = json!({
                "textDocument": {
                    "uri": uri, "languageId": language_id, "version": version, "text": text
                }
            });
            let _ = client
                .request(
                    "lsp.notify",
                    json!({ "id": id, "method": "textDocument/didOpen", "params": params }),
                )
                .await;
        }
        Command::LspDidChange {
            id,
            uri,
            version,
            text,
        } => {
            let params = json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [ { "text": text } ]
            });
            let _ = client
                .request(
                    "lsp.notify",
                    json!({ "id": id, "method": "textDocument/didChange", "params": params }),
                )
                .await;
        }
        // ── DAP ───────────────────────────────────────────────────────────
        // DAP requests run on spawned tasks so the deferred `launch` response
        // doesn't block setBreakpoints/configurationDone on the command loop.
        Command::DapStart {
            id,
            cmd,
            args,
            adapter_id,
            launch,
        } => {
            let c = client.clone();
            tokio::spawn(async move {
                if c.request("dap.start", json!({ "id": id, "cmd": cmd, "args": args }))
                    .await
                    .is_err()
                {
                    return;
                }
                let _ = dap_req(
                    &c,
                    &id,
                    "initialize",
                    json!({
                        "clientID": "enzo", "adapterID": adapter_id,
                        "linesStartAt1": true, "columnsStartAt1": true,
                        "pathFormat": "path", "supportsRunInTerminalRequest": false
                    }),
                )
                .await;
                // Deferred until configurationDone — only blocks this task.
                let _ = dap_req(&c, &id, "launch", launch).await;
            });
        }
        Command::DapSetBreakpoints { id, path, lines } => {
            let c = client.clone();
            tokio::spawn(async move {
                let bps: Vec<Value> = lines.iter().map(|l| json!({ "line": l })).collect();
                let _ = dap_req(
                    &c,
                    &id,
                    "setBreakpoints",
                    json!({ "source": { "path": path }, "breakpoints": bps }),
                )
                .await;
            });
        }
        Command::DapConfigDone { id } => {
            let c = client.clone();
            tokio::spawn(async move {
                let _ = dap_req(&c, &id, "configurationDone", json!({})).await;
            });
        }
        Command::DapStackTrace { id, thread_id } => {
            let c = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Ok(body) = dap_req(
                    &c,
                    &id,
                    "stackTrace",
                    json!({ "threadId": thread_id, "startFrame": 0, "levels": 20 }),
                )
                .await
                {
                    let frames = body["stackFrames"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|fr| StackFrame {
                                    id: fr["id"].as_u64().unwrap_or(0),
                                    name: fr["name"].as_str().unwrap_or_default().to_owned(),
                                    path: fr["source"]["path"].as_str().unwrap_or_default().to_owned(),
                                    line: u32::try_from(fr["line"].as_u64().unwrap_or(0)).unwrap_or(0),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let _ = tx.send(Incoming::DapStackTraceResult { frames });
                }
            });
        }
        Command::DapScopes { id, frame_id } => {
            let c = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Ok(body) =
                    dap_req(&c, &id, "scopes", json!({ "frameId": frame_id })).await
                {
                    let scopes = body["scopes"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|s| DapScope {
                                    name: s["name"].as_str().unwrap_or_default().to_owned(),
                                    reference: s["variablesReference"].as_u64().unwrap_or(0),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let _ = tx.send(Incoming::DapScopesResult { scopes });
                }
            });
        }
        Command::DapVariables { id, reference } => {
            let c = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Ok(body) = dap_req(
                    &c,
                    &id,
                    "variables",
                    json!({ "variablesReference": reference }),
                )
                .await
                {
                    let vars = body["variables"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|v| DapVar {
                                    name: v["name"].as_str().unwrap_or_default().to_owned(),
                                    value: v["value"].as_str().unwrap_or_default().to_owned(),
                                    ty: v["type"].as_str().unwrap_or_default().to_owned(),
                                    reference: v["variablesReference"].as_u64().unwrap_or(0),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let _ = tx.send(Incoming::DapVariablesResult { reference, vars });
                }
            });
        }
        Command::DapContinue { id, thread_id } => {
            let c = client.clone();
            tokio::spawn(async move {
                let _ = dap_req(&c, &id, "continue", json!({ "threadId": thread_id })).await;
            });
        }
        Command::DapStep {
            id,
            thread_id,
            kind,
        } => {
            let command = match kind {
                DapStepKind::Over => "next",
                DapStepKind::In => "stepIn",
                DapStepKind::Out => "stepOut",
            };
            let c = client.clone();
            tokio::spawn(async move {
                let _ = dap_req(&c, &id, command, json!({ "threadId": thread_id })).await;
            });
        }
        Command::DapStop { id } => {
            let _ = client.request("dap.stop", json!({ "id": id })).await;
        }
    }
}

/// Forward a single DAP request through the daemon and return the adapter body.
async fn dap_req(client: &Client, id: &str, command: &str, arguments: Value) -> anyhow::Result<Value> {
    client
        .request(
            "dap.request",
            json!({ "id": id, "command": command, "arguments": arguments }),
        )
        .await
}

/// Fetch branch + status and push a [`Incoming::GitStatus`].
async fn send_git_status(client: &Client, tx: &Sender<Incoming>, root: &str) {
    let branch = client
        .request("git.info", json!({ "path": root }))
        .await
        .ok()
        .and_then(|r| r["branch"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let entries = match client.request("git.status", json!({ "path": root })).await {
        Ok(r) => r["entries"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|e| GitEntry {
                        path: e["path"].as_str().unwrap_or_default().to_owned(),
                        state: e["state"].as_str().unwrap_or_default().to_owned(),
                        staged: e["staged"].as_bool().unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let _ = tx.send(Incoming::GitStatus { branch, entries });
}

/// Seed the first-run demo db (idempotent).
async fn seed_demo(client: &Client, conn: &str) {
    const STMTS: &[&str] = &[
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT)",
        "INSERT OR IGNORE INTO users (id, name, email) VALUES \
         (1,'Alice','alice@example.com'),(2,'Bob','bob@example.com'),\
         (3,'Carol','carol@example.com'),(4,'Dave','dave@example.com')",
        "CREATE TABLE IF NOT EXISTS products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price REAL)",
        "INSERT OR IGNORE INTO products (id, name, price) VALUES \
         (1,'Keyboard',89.0),(2,'Mouse',39.5),(3,'Monitor',329.0)",
    ];
    for sql in STMTS {
        let _ = client
            .request("db.execute", json!({ "conn": conn, "sql": sql }))
            .await;
    }
}

/// Fetch a table's primary-key column names via `db.schema.columns`.
async fn fetch_pk_columns(client: &Client, conn: &str, table: &str) -> Vec<String> {
    match client
        .request("db.schema.columns", json!({ "conn": conn, "table": table }))
        .await
    {
        Ok(r) => r["columns"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|c| c["primary_key"].as_bool().unwrap_or(false))
                    .filter_map(|c| c["name"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn parse_cols_rows(v: &Value) -> (Vec<String>, Vec<Vec<String>>) {
    let columns = v["columns"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| c.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default();
    let rows = v["rows"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    r.as_array()
                        .map(|cs| {
                            cs.iter()
                                .map(|c| match c {
                                    Value::String(s) => s.clone(),
                                    Value::Null => String::new(),
                                    other => other.to_string(),
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    (columns, rows)
}

fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Read loop: routes responses to pending senders, notifications to `tx`.
async fn read_loop(
    reader: tokio::net::unix::OwnedReadHalf,
    pending: PendingMap,
    tx: Sender<Incoming>,
) -> anyhow::Result<()> {
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await.context("read line")? {
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("ATP parse error: {e}");
                continue;
            }
        };
        if let Some(method) = v.get("method").and_then(Value::as_str) {
            handle_notification(method, &v, &tx);
        } else if let Some(id) = v.get("id").and_then(Value::as_u64)
            && let Some(sender) = pending.lock().await.remove(&id)
        {
            let _ = sender.send(v);
        }
    }
    let _ = tx.send(Incoming::Closed);
    Ok(())
}

fn handle_notification(method: &str, v: &Value, tx: &Sender<Incoming>) {
    let p = &v["params"];
    match method {
        "session.output" => {
            if let (Some(id), Some(b64)) = (p["id"].as_str(), p["data"].as_str())
                && let Ok(data) =
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            {
                let _ = tx.send(Incoming::Output {
                    session_id: id.to_owned(),
                    data,
                });
            }
        }
        "prompt.show" => {
            if let Some(id) = p["id"].as_str() {
                let actions = p["actions"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["accept".into(), "reject".into(), "edit".into()]);
                let _ = tx.send(Incoming::PromptShow {
                    id: id.to_owned(),
                    title: p["title"].as_str().unwrap_or("").to_owned(),
                    body: p["body"].as_str().unwrap_or("").to_owned(),
                    actions,
                });
            }
        }
        "block.push" => {
            if let Some(id) = p["id"].as_str() {
                let _ = tx.send(Incoming::BlockPush {
                    id: id.to_owned(),
                    title: p["title"].as_str().unwrap_or("").to_owned(),
                    body: p["body"].as_str().unwrap_or("").to_owned(),
                });
            }
        }
        "block.clear" => {
            if let Some(id) = p["id"].as_str() {
                let _ = tx.send(Incoming::BlockClear { id: id.to_owned() });
            }
        }
        "browser.event" => {
            // Live screencast frames arrive as CDP Page.screencastFrame events.
            if p["method"].as_str() == Some("Page.screencastFrame") {
                let ep = &p["params"];
                if let Some(b64) = ep["data"].as_str()
                    && let Ok(jpeg) =
                        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
                {
                    let session_id = ep["sessionId"].as_i64().unwrap_or(0);
                    let _ = tx.send(Incoming::BrowserFrame { jpeg, session_id });
                }
            }
        }
        "dap.event" => {
            let event = p["event"].as_str().unwrap_or("");
            let body = &p["body"];
            let inc = match event {
                "initialized" => Some(Incoming::DapInitialized),
                "stopped" => Some(Incoming::DapStopped {
                    thread_id: body["threadId"].as_u64().unwrap_or(0),
                    reason: body["reason"].as_str().unwrap_or_default().to_owned(),
                }),
                "continued" => Some(Incoming::DapContinued),
                "output" => Some(Incoming::DapOutput {
                    category: body["category"].as_str().unwrap_or("console").to_owned(),
                    text: body["output"].as_str().unwrap_or_default().to_owned(),
                }),
                "terminated" | "exited" => Some(Incoming::DapTerminated),
                _ => None,
            };
            if let Some(inc) = inc {
                let _ = tx.send(inc);
            }
        }
        "lsp.notification" => {
            if p["method"].as_str() == Some("textDocument/publishDiagnostics") {
                let dp = &p["params"];
                let uri = dp["uri"].as_str().unwrap_or_default().to_owned();
                let items = dp["diagnostics"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|d| {
                                let r = &d["range"];
                                let n = |path: &Value| {
                                    u32::try_from(path.as_u64().unwrap_or(0)).unwrap_or(0)
                                };
                                DiagItem {
                                    start_line: n(&r["start"]["line"]),
                                    start_col: n(&r["start"]["character"]),
                                    end_line: n(&r["end"]["line"]),
                                    end_col: n(&r["end"]["character"]),
                                    message: d["message"].as_str().unwrap_or_default().to_owned(),
                                    severity: u8::try_from(d["severity"].as_u64().unwrap_or(1))
                                        .unwrap_or(1),
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let _ = tx.send(Incoming::LspDiagnostics { uri, items });
            }
        }
        other => log::debug!("unknown notification: {other}"),
    }
}

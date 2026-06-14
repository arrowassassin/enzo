//! ATP connection handler — JSON-RPC 2.0 over newline-delimited Unix socket.
//!
//! Supported methods (v0):
//!
//! **Terminal**
//!   `session.spawn`   `{ id, cols, rows, shell? }`  → `{}`
//!   `session.input`   `{ id, data: base64 }`         → `{}`
//!   `session.resize`  `{ id, cols, rows }`           → `{}`
//!   `session.close`   `{ id }`                       → `{}`
//!   `ping`            `{}`                           → `{ pong: true }`
//!
//! **Database**
//!   `db.connect`  `{ id, path }`         → `{ driver }`
//!   `db.query`    `{ conn, sql }`        → `{ columns, rows }`
//!   `db.execute`  `{ conn, sql }`        → `{ affected }`
//!   `db.close`    `{ conn }`             → `{}`
//!
//! **LSP**
//!   `lsp.start`   `{ id, cmd, args[] }`  → `{}`
//!   `lsp.request` `{ id, method, params }`→ `<result>`
//!   `lsp.notify`  `{ id, method, params }`→ `{}`
//!   `lsp.stop`    `{ id }`               → `{}`
//!
//! **Browser**
//!   `browser.connect`  `{ id, url }`      → `{}`
//!   `browser.navigate` `{ id, url }`      → `{}`
//!   `browser.eval`     `{ id, expr }`     → `{ value }`
//!   `browser.close`    `{ id }`           → `{}`
//!
//! **Display**
//!   `display.register` `{}`              → `{}`
//!     Registers the caller as a display client; subsequent `prompt.show` and
//!     `block.push` notifications are forwarded to it.
//!
//! **Blocks** (fire-and-forget content push from AI agents)
//!   `block.push`  `{ id, type, session_id?, title, body?, diff? }` → `{}`
//!   `block.clear` `{ id }`                                         → `{}`
//!
//! **Prompts** (blocking approval flow from AI agents)
//!   `prompt.show`    `{ id, type, session_id?, title, body?, diff?, actions[] }` → `{ action }`
//!   `prompt.respond` `{ id, action }`  → `{}`
//!   `prompt.dismiss` `{ id }`          → `{}`
//!
//! Outbound notifications (daemon → display clients):
//!   `session.output`    `{ id, data: base64 }` — PTY stdout chunk
//!   `lsp.notification`  `{ id, method, params }` — LSP server push
//!   `browser.event`     `{ id, method, params }` — CDP event
//!   `prompt.show`       `{ id, type, title, body?, diff?, actions[] }` — agent approval request
//!   `block.push`        `{ id, type, title, body?, diff? }` — agent content block

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use enzo_browser::browser::Browser;
use enzo_browser::cdp::CdpEvent;
use enzo_db::pool::AnyPool;
use enzo_editor::lsp::{LspClient, LspNotification};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::pty::spawn_session;
use crate::state::DaemonState;

mod ext;

// ── JSON-RPC wire types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl Response {
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Shared async writer — used by the response loop and per-session push tasks.
type SharedWriter = Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>;

// ── Connection loop ──────────────────────────────────────────────────────────

/// Serve one ATP client connection until the peer closes the stream.
pub async fn handle_connection(stream: UnixStream, state: DaemonState) -> anyhow::Result<()> {
    let (reader, writer) = stream.into_split();
    let writer: SharedWriter = Arc::new(Mutex::new(writer));
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await.context("read line")? {
        if line.trim().is_empty() {
            continue;
        }
        debug!(line = %line, "← ATP");

        let response = match serde_json::from_str::<Request>(&line) {
            Ok(req) => dispatch(req, &state, Arc::clone(&writer)).await,
            Err(e) => Response::err(Value::Null, -32700, format!("parse error: {e}")),
        };

        send_line(&writer, &response)
            .await
            .context("write response")?;
    }

    Ok(())
}

/// Write one JSON-RPC message (response or notification) to the shared writer.
async fn send_line<T: Serialize>(writer: &SharedWriter, msg: &T) -> anyhow::Result<()> {
    let mut out = serde_json::to_string(msg).context("serialize")?;
    out.push('\n');
    debug!(line = %out.trim(), "→ ATP");
    writer
        .lock()
        .await
        .write_all(out.as_bytes())
        .await
        .context("write")
}

// ── Method dispatch ──────────────────────────────────────────────────────────

async fn dispatch(req: Request, state: &DaemonState, writer: SharedWriter) -> Response {
    let Request {
        id, method, params, ..
    } = req;
    let id = id.unwrap_or(Value::Null);
    let p = &params;

    match method.as_str() {
        "ping" => Response::ok(id, json!({ "pong": true })),

        // ── session.* ─────────────────────────────────────────────────────────
        "session.spawn" => session_spawn(id, p, state, writer).await,
        "session.input" => session_input(id, p, state).await,
        "session.resize" => session_resize(id, p, state).await,
        "session.close" => session_close(id, p, state).await,

        // ── db.* ──────────────────────────────────────────────────────────────
        "db.connect" => db_connect(id, p, state).await,
        "db.query" => db_query(id, p, state).await,
        "db.execute" => db_execute(id, p, state).await,
        "db.close" => db_close(id, p, state).await,

        // ── lsp.* ─────────────────────────────────────────────────────────────
        "lsp.start" => lsp_start(id, p, state, writer).await,
        "lsp.request" => lsp_request(id, p, state).await,
        "lsp.notify" => lsp_notify(id, p, state).await,
        "lsp.stop" => lsp_stop(id, p, state).await,

        // ── browser.* ─────────────────────────────────────────────────────────
        "browser.connect" => browser_connect(id, p, state, writer).await,
        "browser.launch" => browser_launch(id, p, state, writer).await,
        "browser.screenshot" => browser_screenshot(id, p, state).await,
        "browser.input" => browser_input(id, p, state).await,
        "browser.navigate" => browser_navigate(id, p, state).await,
        "browser.eval" => browser_eval(id, p, state).await,
        "browser.close" => browser_close(id, p, state).await,

        // ── display.* ─────────────────────────────────────────────────────────
        "display.register" => display_register(id, state, writer).await,

        // ── block.* ───────────────────────────────────────────────────────────
        "block.push" => block_push(id, p, state).await,
        "block.clear" => block_clear(id, p, state).await,

        // ── prompt.* ──────────────────────────────────────────────────────────
        "prompt.show" => prompt_show(id, p, state).await,
        "prompt.respond" => prompt_respond(id, p, state).await,
        "prompt.dismiss" => prompt_dismiss(id, p, state).await,

        // ── theme.* ───────────────────────────────────────────────────────────
        "theme.list" => ext::theme_list(id, state).await,
        "theme.get" => ext::theme_get(id, p, state).await,
        "theme.apply" => ext::theme_apply(id, p, state).await,

        // ── editor.* ──────────────────────────────────────────────────────────
        "editor.highlight" => ext::editor_highlight(id, p).await,
        "editor.format" => ext::editor_format(id, p).await,
        "editor.languages" => ext::editor_languages(id).await,

        // ── git.* ─────────────────────────────────────────────────────────────
        "git.status" => ext::git_status(id, p).await,
        "git.info" => ext::git_info(id, p).await,
        "git.diff" => ext::git_diff(id, p).await,
        "git.stage" => ext::git_stage(id, p).await,
        "git.unstage" => ext::git_unstage(id, p).await,
        "git.commit" => ext::git_commit(id, p).await,
        "git.branches" => ext::git_branches(id, p).await,
        "git.create_branch" => ext::git_create_branch(id, p).await,
        "git.checkout" => ext::git_checkout(id, p).await,
        "git.log" => ext::git_log(id, p).await,
        "git.fetch" => ext::git_fetch(id, p).await,
        "git.push" => ext::git_push(id, p).await,
        "git.worktrees" => ext::git_worktrees(id, p).await,
        "git.add_worktree" => ext::git_add_worktree(id, p).await,

        // ── db.schema.* ───────────────────────────────────────────────────────
        "db.schema.tables" => ext::db_schema_tables(id, p, state).await,
        "db.schema.columns" => ext::db_schema_columns(id, p, state).await,
        "db.schema.indexes" => ext::db_schema_indexes(id, p, state).await,

        // ── db.table.* ────────────────────────────────────────────────────────
        "db.table.browse" => ext::db_table_browse(id, p, state).await,
        "db.table.update" => ext::db_table_update(id, p, state).await,
        "db.table.delete" => ext::db_table_delete(id, p, state).await,
        "db.table.insert" => ext::db_table_insert(id, p, state).await,

        // ── db.tabs.* ─────────────────────────────────────────────────────────
        "db.tabs.list" => ext::db_tabs_list(id, p, state).await,
        "db.tabs.open" => ext::db_tabs_open(id, p, state).await,
        "db.tabs.close" => ext::db_tabs_close(id, p, state).await,
        "db.tabs.rename" => ext::db_tabs_rename(id, p, state).await,
        "db.tabs.set_sql" => ext::db_tabs_set_sql(id, p, state).await,

        other => {
            warn!(method = other, "unknown ATP method");
            Response::err(id, -32601, format!("method not found: {other}"))
        }
    }
}

// ── Session handlers ─────────────────────────────────────────────────────────

async fn session_spawn(
    id: Value,
    p: &Value,
    state: &DaemonState,
    writer: SharedWriter,
) -> Response {
    let Some(session_id) = p["id"].as_str().map(str::to_owned) else {
        return Response::err(id, -32602, "missing id");
    };
    let cols = u16::try_from(p["cols"].as_u64().unwrap_or(220)).unwrap_or(220);
    let rows = u16::try_from(p["rows"].as_u64().unwrap_or(50)).unwrap_or(50);
    let shell = p["shell"].as_str().map(str::to_owned);

    // Idempotent: re-spawning an existing id must not replace (and so kill, via
    // Session::drop) the live shell already running under it.
    if state.session_exists(&session_id).await {
        return Response::ok(id, json!({ "existing": true }));
    }

    match spawn_session(session_id.clone(), shell.as_deref(), cols, rows) {
        Ok(session) => {
            if let Some(pty_reader) = session.take_reader() {
                let sid = session_id.clone();
                let w = Arc::clone(&writer);
                tokio::task::spawn_blocking(move || {
                    push_pty_output(&sid, pty_reader, &w);
                });
            }
            state.insert_session(session).await;
            Response::ok(id, json!({}))
        }
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

async fn session_input(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(session_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(b64) = p["data"].as_str() else {
        return Response::err(id, -32602, "missing data");
    };
    let bytes = match base64_decode(b64) {
        Ok(b) => b,
        Err(e) => return Response::err(id, -32602, e),
    };
    match state.session_write_stdin(session_id, bytes).await {
        None => Response::err(id, -32001, "unknown session"),
        Some(Err(e)) => Response::err(id, -32000, e.to_string()),
        Some(Ok(())) => Response::ok(id, json!({})),
    }
}

async fn session_resize(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(session_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let cols = u16::try_from(p["cols"].as_u64().unwrap_or(220)).unwrap_or(220);
    let rows = u16::try_from(p["rows"].as_u64().unwrap_or(50)).unwrap_or(50);
    match state.session_resize(session_id, cols, rows).await {
        None => Response::err(id, -32001, "unknown session"),
        Some(Err(e)) => Response::err(id, -32000, e),
        Some(Ok(())) => Response::ok(id, json!({})),
    }
}

async fn session_close(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(session_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    match state.remove_session(session_id).await {
        None => Response::err(id, -32001, "unknown session"),
        Some(_) => Response::ok(id, json!({})),
    }
}

// ── Database handlers ─────────────────────────────────────────────────────────

async fn db_connect(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(conn_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let path = p["path"].as_str().unwrap_or(":memory:");
    // Optional explicit driver ("sqlite" | "duckdb"); otherwise inferred from
    // the path extension (.duckdb/.ddb → DuckDB), defaulting to SQLite.
    let driver = p["driver"].as_str().unwrap_or("");
    match AnyPool::open(driver, path) {
        Ok(pool) => {
            let driver = pool.driver_name();
            state.insert_db_conn(conn_id.to_owned(), pool).await;
            Response::ok(id, json!({ "driver": driver }))
        }
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

async fn db_query(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(conn_id) = p["conn"].as_str() else {
        return Response::err(id, -32602, "missing conn");
    };
    let Some(sql) = p["sql"].as_str() else {
        return Response::err(id, -32602, "missing sql");
    };
    let Some(pool) = state.get_db_conn(conn_id).await else {
        return Response::err(id, -32001, "unknown connection");
    };
    match pool.query(sql).await {
        Err(e) => Response::err(id, -32000, e.to_string()),
        Ok(batches) => match enzo_db::batches_to_json(&batches) {
            Ok(result) => Response::ok(id, result),
            Err(e) => Response::err(id, -32000, e.to_string()),
        },
    }
}

async fn db_execute(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(conn_id) = p["conn"].as_str() else {
        return Response::err(id, -32602, "missing conn");
    };
    let Some(sql) = p["sql"].as_str() else {
        return Response::err(id, -32602, "missing sql");
    };
    let Some(pool) = state.get_db_conn(conn_id).await else {
        return Response::err(id, -32001, "unknown connection");
    };
    match pool.execute(sql).await {
        Ok(n) => Response::ok(id, json!({ "affected": n })),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

async fn db_close(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(conn_id) = p["conn"].as_str() else {
        return Response::err(id, -32602, "missing conn");
    };
    if state.remove_db_conn(conn_id).await.is_some() {
        state.remove_db_tabs(conn_id).await;
        Response::ok(id, json!({}))
    } else {
        Response::err(id, -32001, "unknown connection")
    }
}

// ── LSP handlers ──────────────────────────────────────────────────────────────

async fn lsp_start(id: Value, p: &Value, state: &DaemonState, writer: SharedWriter) -> Response {
    let Some(lsp_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(cmd) = p["cmd"].as_str() else {
        return Response::err(id, -32602, "missing cmd");
    };
    let args: Vec<String> = p["args"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();

    let lsp_id_owned = lsp_id.to_owned();
    let on_notification = {
        let w = Arc::clone(&writer);
        move |notif: LspNotification| {
            let n = Notification {
                jsonrpc: "2.0",
                method: "lsp.notification",
                params: json!({
                    "id": lsp_id_owned,
                    "method": notif.method,
                    "params": notif.params,
                }),
            };
            let w = Arc::clone(&w);
            tokio::spawn(async move {
                let _ = send_line(&w, &n).await;
            });
        }
    };

    match LspClient::spawn(cmd, &args_ref, on_notification) {
        Ok(client) => {
            state.insert_lsp_client(lsp_id.to_owned(), client).await;
            Response::ok(id, json!({}))
        }
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

async fn lsp_request(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(lsp_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(method) = p["method"].as_str() else {
        return Response::err(id, -32602, "missing method");
    };
    let params = p["params"].clone();

    let Some(client) = state.get_lsp_client(lsp_id).await else {
        return Response::err(id, -32001, "unknown LSP client");
    };
    match client.request(method, params).await {
        Ok(result) => Response::ok(id, result),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

async fn lsp_notify(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(lsp_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(method) = p["method"].as_str() else {
        return Response::err(id, -32602, "missing method");
    };
    let params = p["params"].clone();

    let Some(client) = state.get_lsp_client(lsp_id).await else {
        return Response::err(id, -32001, "unknown LSP client");
    };
    match client.notify(method, params).await {
        Ok(()) => Response::ok(id, json!({})),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

async fn lsp_stop(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(lsp_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(client) = state.remove_lsp_client(lsp_id).await else {
        return Response::err(id, -32001, "unknown LSP client");
    };
    match client.shutdown().await {
        Ok(()) | Err(_) => Response::ok(id, json!({})),
    }
}

// ── Browser handlers ──────────────────────────────────────────────────────────

async fn browser_connect(
    id: Value,
    p: &Value,
    state: &DaemonState,
    writer: SharedWriter,
) -> Response {
    let Some(page_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(debug_url) = p["url"].as_str() else {
        return Response::err(id, -32602, "missing url");
    };

    let page_id_owned = page_id.to_owned();
    let on_event = {
        let w = Arc::clone(&writer);
        move |event: CdpEvent| {
            let n = Notification {
                jsonrpc: "2.0",
                method: "browser.event",
                params: json!({
                    "id": page_id_owned,
                    "method": event.method,
                    "params": event.params,
                }),
            };
            let w = Arc::clone(&w);
            tokio::spawn(async move {
                let _ = send_line(&w, &n).await;
            });
        }
    };

    match Browser::connect(debug_url)
        .attach_first_page(on_event)
        .await
    {
        Ok(page) => {
            state.insert_browser_page(page_id.to_owned(), page).await;
            Response::ok(id, json!({}))
        }
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

/// Launch a headless browser, attach to it, and (optionally) navigate.
async fn browser_launch(
    id: Value,
    p: &Value,
    state: &DaemonState,
    writer: SharedWriter,
) -> Response {
    let Some(page_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let url = p["url"].as_str().unwrap_or("about:blank").to_owned();
    let width = u32::try_from(p["width"].as_u64().unwrap_or(1280)).unwrap_or(1280);
    let height = u32::try_from(p["height"].as_u64().unwrap_or(800)).unwrap_or(800);

    let launched = match enzo_browser::launch(width, height).await {
        Ok(l) => l,
        Err(e) => return Response::err(id, -32000, e.to_string()),
    };

    let page_id_owned = page_id.to_owned();
    let on_event = {
        let w = Arc::clone(&writer);
        move |event: CdpEvent| {
            let n = Notification {
                jsonrpc: "2.0",
                method: "browser.event",
                params: json!({ "id": page_id_owned, "method": event.method, "params": event.params }),
            };
            let w = Arc::clone(&w);
            tokio::spawn(async move {
                let _ = send_line(&w, &n).await;
            });
        }
    };

    match launched.browser.attach_first_page(on_event).await {
        Ok(page) => {
            if !url.is_empty() && url != "about:blank" {
                let _ = page.navigate(&url).await;
            }
            state.insert_browser_page(page_id.to_owned(), page).await;
            state
                .insert_browser_proc(page_id.to_owned(), launched.child)
                .await;
            Response::ok(id, json!({}))
        }
        Err(e) => {
            let mut child = launched.child;
            let _ = child.kill();
            Response::err(id, -32000, e.to_string())
        }
    }
}

/// Capture a PNG screenshot of the page, returned as base64.
async fn browser_screenshot(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(page_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(page) = state.get_browser_page(page_id).await else {
        return Response::err(id, -32001, "unknown browser page");
    };
    match page.screenshot_png().await {
        Ok(bytes) => Response::ok(id, json!({ "png": base64_encode(&bytes) })),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

/// Forward a raw CDP input event (`Input.dispatchMouseEvent`, etc.) to the page.
async fn browser_input(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(page_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(method) = p["method"].as_str() else {
        return Response::err(id, -32602, "missing method");
    };
    let params = p["params"].clone();
    let Some(page) = state.get_browser_page(page_id).await else {
        return Response::err(id, -32001, "unknown browser page");
    };
    match page.session().call(method, params).await {
        Ok(_) => Response::ok(id, json!({})),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

async fn browser_navigate(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(page_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(url) = p["url"].as_str() else {
        return Response::err(id, -32602, "missing url");
    };
    let Some(page) = state.get_browser_page(page_id).await else {
        return Response::err(id, -32001, "unknown browser page");
    };
    match page.navigate(url).await {
        Ok(()) => Response::ok(id, json!({})),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

async fn browser_eval(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(page_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let Some(expr) = p["expr"].as_str() else {
        return Response::err(id, -32602, "missing expr");
    };
    let Some(page) = state.get_browser_page(page_id).await else {
        return Response::err(id, -32001, "unknown browser page");
    };
    match page.eval(expr).await {
        Ok(value) => Response::ok(id, json!({ "value": value })),
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

async fn browser_close(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(page_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    match state.remove_browser_page(page_id).await {
        None => Response::err(id, -32001, "unknown browser page"),
        Some(_) => Response::ok(id, json!({})),
    }
}

// ── Display handlers ──────────────────────────────────────────────────────────

/// Register the calling connection as a display client.
///
/// After registration the connection receives all `prompt.show` and `block.push`
/// notifications via the broadcast channel, forwarded from any adapter connection.
async fn display_register(id: Value, state: &DaemonState, writer: SharedWriter) -> Response {
    let mut rx = state.subscribe_notifications().await;
    tokio::spawn(async move {
        while let Ok(mut json) = rx.recv().await {
            json.push('\n');
            if writer
                .lock()
                .await
                .write_all(json.as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
    });
    Response::ok(id, json!({}))
}

// ── Block handlers ────────────────────────────────────────────────────────────

/// Push a content block to all display clients (fire-and-forget).
///
/// The block payload is forwarded verbatim as a `block.push` notification.
/// `type` may be `"text"`, `"diff"`, or `"code"`.
async fn block_push(id: Value, p: &Value, state: &DaemonState) -> Response {
    let notif = Notification {
        jsonrpc: "2.0",
        method: "block.push",
        params: p.clone(),
    };
    match serde_json::to_string(&notif) {
        Ok(json) => {
            state.broadcast_notification(json).await;
            Response::ok(id, json!({}))
        }
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

/// Clear a previously pushed block from all display clients.
async fn block_clear(id: Value, p: &Value, state: &DaemonState) -> Response {
    let notif = Notification {
        jsonrpc: "2.0",
        method: "block.clear",
        params: p.clone(),
    };
    match serde_json::to_string(&notif) {
        Ok(json) => {
            state.broadcast_notification(json).await;
            Response::ok(id, json!({}))
        }
        Err(e) => Response::err(id, -32000, e.to_string()),
    }
}

// ── Prompt handlers ───────────────────────────────────────────────────────────

/// Show an approval prompt to all display clients and **block** until the user
/// responds via [`prompt_respond`] or the 5-minute timeout elapses.
///
/// Called by AI CLI adapters (e.g. `enzo-claude`) to pause and surface a
/// tool-call approval in the GPU renderer.
///
/// Returns `{ "action": "accept" | "reject" | "edit" }`.
async fn prompt_show(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(prompt_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };

    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    state.insert_prompt(prompt_id.to_owned(), tx).await;

    // Broadcast the prompt to all registered display clients.
    let notif = Notification {
        jsonrpc: "2.0",
        method: "prompt.show",
        params: p.clone(),
    };
    match serde_json::to_string(&notif) {
        Ok(json) => state.broadcast_notification(json).await,
        Err(e) => {
            state.cancel_prompt(prompt_id).await;
            return Response::err(id, -32000, format!("serialize notification: {e}"));
        }
    }

    // Block until the display client calls prompt.respond (or times out).
    match tokio::time::timeout(Duration::from_mins(5), rx).await {
        Ok(Ok(action)) => Response::ok(id, json!({ "action": action })),
        Ok(Err(_)) => Response::err(id, -32000, "prompt cancelled"),
        Err(_) => {
            state.cancel_prompt(prompt_id).await;
            Response::err(id, -32000, "prompt timed out after 5 minutes")
        }
    }
}

/// Respond to a pending `prompt.show` call with the user's chosen action.
///
/// Called by the GPU display client when the user clicks ACCEPT / REJECT / EDIT.
async fn prompt_respond(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(prompt_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    let action = p["action"].as_str().unwrap_or("reject").to_owned();
    if state.resolve_prompt(prompt_id, action).await {
        Response::ok(id, json!({}))
    } else {
        Response::err(id, -32001, "unknown or already-resolved prompt id")
    }
}

/// Dismiss a pending prompt without an action (equivalent to "reject").
async fn prompt_dismiss(id: Value, p: &Value, state: &DaemonState) -> Response {
    let Some(prompt_id) = p["id"].as_str() else {
        return Response::err(id, -32602, "missing id");
    };
    if state.resolve_prompt(prompt_id, "reject".to_owned()).await {
        Response::ok(id, json!({}))
    } else {
        Response::err(id, -32001, "unknown or already-resolved prompt id")
    }
}

// ── PTY output push (blocking task) ─────────────────────────────────────────

#[derive(Serialize)]
struct Notification {
    jsonrpc: &'static str,
    method: &'static str,
    params: Value,
}

/// Read PTY stdout in a blocking thread and push `session.output` notifications.
fn push_pty_output(
    session_id: &str,
    mut reader: Box<dyn std::io::Read + Send>,
    writer: &SharedWriter,
) {
    use std::io::Read;
    let rt = tokio::runtime::Handle::current();
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let data = base64_encode(&buf[..n]);
                let notif = Notification {
                    jsonrpc: "2.0",
                    method: "session.output",
                    params: json!({ "id": session_id, "data": data }),
                };
                let w = Arc::clone(writer);
                rt.block_on(async move {
                    let _ = send_line(&w, &notif).await;
                });
            }
        }
    }
}

// ── Base64 helpers (no external dep) ────────────────────────────────────────

const B64_TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes to standard base64.
#[must_use]
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(B64_TABLE[b0 >> 2] as char);
        out.push(B64_TABLE[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() >= 2 {
            out.push(B64_TABLE[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if chunk.len() == 3 {
            out.push(B64_TABLE[b2 & 0x3f] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decode standard base64 into bytes.
pub(crate) fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use std::collections::HashMap;
    let table: HashMap<u8, u8> = B64_TABLE
        .iter()
        .enumerate()
        // Safety: alphabet has exactly 64 entries, indices 0–63 fit in u8.
        .map(|(i, &c)| (c, u8::try_from(i).expect("base64 alphabet < 64 entries")))
        .collect();

    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4 + 1);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let chunk = &bytes[i..bytes.len().min(i + 4)];
        let vals: Vec<u8> = chunk
            .iter()
            .filter(|&&b| b != b'\n' && b != b'\r')
            .map(|b| {
                table
                    .get(b)
                    .copied()
                    .ok_or_else(|| format!("invalid base64 char: {b}"))
            })
            .collect::<Result<_, _>>()?;
        match vals.len() {
            2 => out.push((vals[0] << 2) | (vals[1] >> 4)),
            3 => {
                out.push((vals[0] << 2) | (vals[1] >> 4));
                out.push((vals[1] << 4) | (vals[2] >> 2));
            }
            4 => {
                out.push((vals[0] << 2) | (vals[1] >> 4));
                out.push((vals[1] << 4) | (vals[2] >> 2));
                out.push((vals[2] << 6) | vals[3]);
            }
            _ => {}
        }
        i += 4;
    }
    Ok(out)
}

// ── Public test helper ───────────────────────────────────────────────────────

/// Invoke one ATP method and return the serialised response.
///
/// Exposed for integration tests — drives the dispatch layer without a real socket.
pub async fn call(
    state: &DaemonState,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);

    let req = Request {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!(1)),
        method: method.to_owned(),
        params,
    };
    let tmp = format!("/tmp/enzo-atp-call-{}-{n}.sock", std::process::id());
    let _ = std::fs::remove_file(&tmp);
    let listener = tokio::net::UnixListener::bind(&tmp).expect("bind call sock");
    let (server_stream, _client) = tokio::join!(
        async { listener.accept().await.map(|(s, _)| s).expect("accept") },
        tokio::net::UnixStream::connect(&tmp),
    );
    let _ = std::fs::remove_file(&tmp);
    let (_sr, sw) = server_stream.into_split();
    let writer: SharedWriter = Arc::new(Mutex::new(sw));
    let resp = dispatch(req, state, writer).await;
    serde_json::to_value(resp).expect("serialize response")
}

#[cfg(test)]
mod tests;

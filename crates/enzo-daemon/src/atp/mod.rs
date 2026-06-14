//! ATP connection handler — JSON-RPC 2.0 over newline-delimited Unix socket.
//!
//! Supported methods (v0):
//!   session.spawn   { id, cols, rows, shell? }  → {}
//!   session.input   { id, data: base64 }         → {}
//!   session.resize  { id, cols, rows }           → {}
//!   session.close   { id }                       → {}
//!   ping            {}                           → { pong: true }
//!
//! Outbound notifications (daemon → client):
//!   session.output  { id, data: base64 }         — PTY stdout chunk

use std::sync::Arc;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::pty::spawn_session;
use crate::state::DaemonState;

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

/// Shared async writer — used by both the response loop and per-session push tasks.
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
    let id = req.id.unwrap_or(Value::Null);
    match req.method.as_str() {
        "ping" => Response::ok(id, json!({ "pong": true })),

        "session.spawn" => {
            let p = &req.params;
            let Some(session_id) = p["id"].as_str().map(str::to_owned) else {
                return Response::err(id, -32602, "missing id");
            };
            let cols = u16::try_from(p["cols"].as_u64().unwrap_or(220)).unwrap_or(220);
            let rows = u16::try_from(p["rows"].as_u64().unwrap_or(50)).unwrap_or(50);
            let shell = p["shell"].as_str().map(str::to_owned);

            match spawn_session(session_id.clone(), shell.as_deref(), cols, rows) {
                Ok(session) => {
                    // Pull the PTY reader out and start a background push task.
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

        "session.input" => {
            let p = &req.params;
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

        "session.resize" => {
            let p = &req.params;
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

        "session.close" => {
            let Some(session_id) = req.params["id"].as_str() else {
                return Response::err(id, -32602, "missing id");
            };
            match state.remove_session(session_id).await {
                None => Response::err(id, -32001, "unknown session"),
                Some(_) => Response::ok(id, json!({})),
            }
        }

        other => {
            warn!(method = other, "unknown ATP method");
            Response::err(id, -32601, format!("method not found: {other}"))
        }
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
    // Each call gets a unique socket so parallel tests don't race.
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

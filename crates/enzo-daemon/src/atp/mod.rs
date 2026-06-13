//! ATP connection handler — JSON-RPC 2.0 over newline-delimited Unix socket.
//!
//! Supported methods (v0):
//!   session.spawn   { id, cols, rows, shell? }  → {}
//!   session.input   { id, data: base64 }         → {}
//!   session.resize  { id, cols, rows }           → {}
//!   session.close   { id }                       → {}
//!   ping            {}                           → { pong: true }

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
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

// ── Connection loop ──────────────────────────────────────────────────────────

/// Serve one ATP client connection until the peer closes the stream.
pub async fn handle_connection(stream: UnixStream, state: DaemonState) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await.context("read line")? {
        if line.trim().is_empty() {
            continue;
        }
        debug!(line = %line, "← ATP");

        let response = match serde_json::from_str::<Request>(&line) {
            Ok(req) => dispatch(req, &state).await,
            Err(e) => Response::err(Value::Null, -32700, format!("parse error: {e}")),
        };

        let mut out = serde_json::to_string(&response).context("serialize response")?;
        out.push('\n');
        debug!(line = %out.trim(), "→ ATP");
        writer
            .write_all(out.as_bytes())
            .await
            .context("write response")?;
    }

    Ok(())
}

// ── Method dispatch ──────────────────────────────────────────────────────────

async fn dispatch(req: Request, state: &DaemonState) -> Response {
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

// ── Public test helper ───────────────────────────────────────────────────────

/// Invoke one ATP method and return the serialised response.
///
/// Exposed for integration tests — drives the dispatch layer without a real socket.
pub async fn call(
    state: &DaemonState,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    let req = Request {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!(1)),
        method: method.to_owned(),
        params,
    };
    let resp = dispatch(req, state).await;
    serde_json::to_value(resp).expect("serialize response")
}

// ── Base64 decode (no external dep — std alphabet) ──────────────────────────

pub(crate) fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use std::collections::HashMap;
    let table: HashMap<u8, u8> = (b'A'..=b'Z')
        .chain(b'a'..=b'z')
        .chain(b'0'..=b'9')
        .chain([b'+', b'/'])
        .enumerate()
        // Safety: alphabet has exactly 64 entries, indices 0–63 fit in u8.
        .map(|(i, c)| (c, u8::try_from(i).expect("base64 alphabet < 64 entries")))
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

#[cfg(test)]
mod tests;

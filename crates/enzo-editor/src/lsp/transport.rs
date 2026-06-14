//! Content-Length framed LSP transport (read + write sides).

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{Mutex, oneshot};

/// A server-initiated notification received from the language server.
#[derive(Debug)]
pub struct LspNotification {
    /// The JSON-RPC method name (e.g. `"textDocument/publishDiagnostics"`).
    pub method: String,
    /// The `params` payload (may be `null`).
    pub params: Value,
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// Write one JSON-RPC message with a `Content-Length` header.
pub async fn write_message(stdin: &mut ChildStdin, msg: &Value) -> anyhow::Result<()> {
    let body = serde_json::to_vec(msg).context("serialize")?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin
        .write_all(header.as_bytes())
        .await
        .context("write header")?;
    stdin.write_all(&body).await.context("write body")?;
    Ok(())
}

/// Read messages from the server until EOF, routing responses and notifications.
pub async fn read_loop(
    stdout: ChildStdout,
    pending: PendingMap,
    mut on_notification: impl FnMut(LspNotification),
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stdout);
    loop {
        // Read headers until the blank line.
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).await.context("read header")?;
            if n == 0 {
                return Ok(());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                content_length = rest.trim().parse::<usize>().ok();
            }
        }

        let Some(len) = content_length else {
            log::warn!("LSP message without Content-Length");
            continue;
        };

        let mut body = vec![0u8; len];
        reader.read_exact(&mut body).await.context("read body")?;

        let v: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("LSP JSON parse error: {e}");
                continue;
            }
        };

        if v.get("method").is_some() {
            // Notification or server request — only notifications handled here.
            if v.get("id").is_none() {
                let method = v["method"].as_str().unwrap_or("").to_owned();
                let params = v["params"].clone();
                on_notification(LspNotification { method, params });
            }
        } else if let Some(id) = v.get("id").and_then(Value::as_u64)
            && let Some(tx) = pending.lock().await.remove(&id)
        {
            let _ = tx.send(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_fields() {
        let n = LspNotification {
            method: "textDocument/publishDiagnostics".into(),
            params: serde_json::json!({ "uri": "file:///a.rs", "diagnostics": [] }),
        };
        assert_eq!(n.method, "textDocument/publishDiagnostics");
        assert!(n.params["uri"].as_str().is_some());
    }
}

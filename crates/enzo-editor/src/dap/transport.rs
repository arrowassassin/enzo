//! Content-Length framed DAP transport.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{Mutex, oneshot};

/// An event received from the debug adapter.
#[derive(Debug)]
pub struct DapEvent {
    /// The event type (e.g. `"stopped"`, `"output"`, `"terminated"`).
    pub event: String,
    /// The event body (may be `null`).
    pub body: Value,
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// Write one DAP message with a `Content-Length` header.
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

/// Read messages from the adapter until EOF, dispatching events and responses.
pub async fn read_loop(
    stdout: ChildStdout,
    pending: PendingMap,
    mut on_event: impl FnMut(DapEvent),
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stdout);
    loop {
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
            log::warn!("DAP message without Content-Length");
            continue;
        };

        let mut body = vec![0u8; len];
        reader.read_exact(&mut body).await.context("read body")?;

        let v: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("DAP JSON parse error: {e}");
                continue;
            }
        };

        match v["type"].as_str() {
            Some("event") => {
                let event = v["event"].as_str().unwrap_or("").to_owned();
                let event_body = v["body"].clone();
                on_event(DapEvent {
                    event,
                    body: event_body,
                });
            }
            Some("response") => {
                if let Some(req_seq) = v["request_seq"].as_u64()
                    && let Some(tx) = pending.lock().await.remove(&req_seq)
                {
                    let _ = tx.send(v);
                }
            }
            other => {
                log::debug!("DAP unknown message type: {other:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dap_event_fields() {
        let e = DapEvent {
            event: "stopped".into(),
            body: serde_json::json!({ "reason": "breakpoint", "threadId": 1 }),
        };
        assert_eq!(e.event, "stopped");
        assert_eq!(e.body["reason"].as_str(), Some("breakpoint"));
    }
}

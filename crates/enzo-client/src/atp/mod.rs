//! ATP client — connects to enzo-daemon over a Unix socket.
//!
//! Sends JSON-RPC 2.0 requests and multiplexes responses and
//! `session.output` notifications back to callers.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, oneshot};

/// A message received from the daemon: either a response or a notification.
#[derive(Debug)]
pub enum DaemonMessage {
    /// PTY output for a session.
    Output {
        /// The session that produced the output.
        session_id: String,
        /// Raw PTY bytes.
        data: Vec<u8>,
    },
    /// The daemon closed the connection.
    Closed,
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// Live connection to the daemon.
pub struct AtpClient {
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    pending: PendingMap,
    next_id: Arc<Mutex<u64>>,
}

impl AtpClient {
    /// Connect to the daemon socket and start the read loop.
    ///
    /// `on_message` is called for every notification (PTY output, etc.)
    /// from a background tokio task.
    pub async fn connect(
        sock_path: &str,
        on_message: impl FnMut(DaemonMessage) + Send + 'static,
    ) -> anyhow::Result<Self> {
        let stream = UnixStream::connect(sock_path)
            .await
            .with_context(|| format!("connect to {sock_path}"))?;
        let (reader, writer) = stream.into_split();

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_r = Arc::clone(&pending);
        let writer = Arc::new(Mutex::new(writer));

        tokio::spawn(async move {
            if let Err(e) = read_loop(reader, pending_r, on_message).await {
                log::warn!("ATP read loop ended: {e:#}");
            }
        });

        Ok(Self {
            writer,
            pending,
            next_id: Arc::new(Mutex::new(1)),
        })
    }

    /// Send a request and wait for its response.
    pub async fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = {
            let mut n = self.next_id.lock().await;
            let id = *n;
            *n += 1;
            id
        };

        let (tx, rx) = oneshot::channel::<Value>();
        self.pending.lock().await.insert(id, tx);

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut line = serde_json::to_string(&msg).context("serialize request")?;
        line.push('\n');
        self.writer
            .lock()
            .await
            .write_all(line.as_bytes())
            .await
            .context("write request")?;

        let resp = rx.await.context("response channel closed")?;
        if let Some(err) = resp.get("error") {
            anyhow::bail!("ATP error: {err}");
        }
        Ok(resp["result"].clone())
    }

    /// Spawn a new terminal session.
    pub async fn spawn_session(&self, id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.request(
            "session.spawn",
            json!({ "id": id, "cols": cols, "rows": rows }),
        )
        .await
        .map(|_| ())
    }

    /// Send keyboard input to a session.
    pub async fn send_input(&self, id: &str, data: &[u8]) -> anyhow::Result<()> {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data);
        self.request("session.input", json!({ "id": id, "data": b64 }))
            .await
            .map(|_| ())
    }

    /// Resize a session's PTY.
    pub async fn resize(&self, id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.request(
            "session.resize",
            json!({ "id": id, "cols": cols, "rows": rows }),
        )
        .await
        .map(|_| ())
    }

    /// Close a session.
    pub async fn close_session(&self, id: &str) -> anyhow::Result<()> {
        self.request("session.close", json!({ "id": id }))
            .await
            .map(|_| ())
    }
}

// ── Read loop ────────────────────────────────────────────────────────────────

async fn read_loop(
    reader: tokio::net::unix::OwnedReadHalf,
    pending: PendingMap,
    mut on_message: impl FnMut(DaemonMessage),
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
            handle_notification(method, &v, &mut on_message);
        } else if let Some(id) = v.get("id").and_then(Value::as_u64)
            && let Some(tx) = pending.lock().await.remove(&id)
        {
            let _ = tx.send(v);
        }
    }
    on_message(DaemonMessage::Closed);
    Ok(())
}

fn handle_notification(method: &str, v: &Value, on_message: &mut impl FnMut(DaemonMessage)) {
    match method {
        "session.output" => {
            let Some(session_id) = v["params"]["id"].as_str() else {
                return;
            };
            let Some(b64) = v["params"]["data"].as_str() else {
                return;
            };
            match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64) {
                Ok(data) => on_message(DaemonMessage::Output {
                    session_id: session_id.to_owned(),
                    data,
                }),
                Err(e) => log::warn!("invalid base64 in session.output: {e}"),
            }
        }
        other => log::debug!("unknown notification: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::DaemonMessage;
    use super::handle_notification;
    use serde_json::json;

    #[test]
    fn notification_output_decoded() {
        // "hello" → "aGVsbG8="
        let v = json!({
            "jsonrpc": "2.0",
            "method": "session.output",
            "params": { "id": "s1", "data": "aGVsbG8=" }
        });
        let mut got: Option<DaemonMessage> = None;
        handle_notification("session.output", &v, &mut |m| got = Some(m));
        match got.unwrap() {
            DaemonMessage::Output { session_id, data } => {
                assert_eq!(session_id, "s1");
                assert_eq!(data, b"hello");
            }
            other @ DaemonMessage::Closed => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn notification_output_missing_id_ignored() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "session.output",
            "params": { "data": "aGVsbG8=" }
        });
        let mut count = 0u32;
        handle_notification("session.output", &v, &mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn notification_output_missing_data_ignored() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "session.output",
            "params": { "id": "s1" }
        });
        let mut count = 0u32;
        handle_notification("session.output", &v, &mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn notification_output_invalid_base64_ignored() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "session.output",
            "params": { "id": "s1", "data": "!!!" }
        });
        let mut count = 0u32;
        handle_notification("session.output", &v, &mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn notification_unknown_method_ignored() {
        let v = json!({ "jsonrpc": "2.0", "method": "foo", "params": {} });
        let mut count = 0u32;
        handle_notification("foo", &v, &mut |_| count += 1);
        assert_eq!(count, 0);
    }
}

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
    /// An AI agent requests an inline approval (rendered as an overlay card).
    PromptShow {
        /// Prompt id — echoed back in `prompt.respond`.
        id: String,
        /// Prompt kind: `"diff"` or `"text"`.
        kind: String,
        /// Short title (e.g. "claude wants to edit renderer.rs").
        title: String,
        /// Body / context text.
        body: String,
        /// Optional unified diff (`{ path, raw }`).
        diff: Option<Value>,
        /// Available actions (e.g. `["accept","reject","edit"]`).
        actions: Vec<String>,
    },
    /// An AI agent pushes a non-blocking content block.
    BlockPush {
        /// Block id.
        id: String,
        /// Block kind: `"text"`, `"diff"`, or `"code"`.
        kind: String,
        /// Title line.
        title: String,
        /// Body text.
        body: String,
    },
    /// Remove a previously-pushed block.
    BlockClear {
        /// Block id to remove.
        id: String,
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

    /// Register this connection as a display client so it receives
    /// `prompt.show` / `block.push` broadcasts from AI agent adapters.
    pub async fn register_display(&self) -> anyhow::Result<()> {
        self.request("display.register", json!({}))
            .await
            .map(|_| ())
    }

    /// Respond to a pending agent prompt with the chosen action.
    pub async fn respond_prompt(&self, id: &str, action: &str) -> anyhow::Result<()> {
        self.request("prompt.respond", json!({ "id": id, "action": action }))
            .await
            .map(|_| ())
    }

    /// Launch a headless browser under `id` and navigate to `url`.
    pub async fn browser_launch(&self, id: &str, url: &str, w: u32, h: u32) -> anyhow::Result<()> {
        self.request(
            "browser.launch",
            json!({ "id": id, "url": url, "width": w, "height": h }),
        )
        .await
        .map(|_| ())
    }

    /// Navigate an existing browser page to `url`.
    pub async fn browser_navigate(&self, id: &str, url: &str) -> anyhow::Result<()> {
        self.request("browser.navigate", json!({ "id": id, "url": url }))
            .await
            .map(|_| ())
    }

    /// Capture a screenshot; returns the decoded PNG bytes.
    pub async fn browser_screenshot(&self, id: &str) -> anyhow::Result<Vec<u8>> {
        let r = self
            .request("browser.screenshot", json!({ "id": id }))
            .await?;
        let b64 = r["png"].as_str().context("missing png")?;
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .context("decode screenshot base64")
    }

    /// Forward a raw CDP input event to the browser page.
    pub async fn browser_input(&self, id: &str, method: &str, params: Value) -> anyhow::Result<()> {
        self.request(
            "browser.input",
            json!({ "id": id, "method": method, "params": params }),
        )
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
        "prompt.show" => {
            let p = &v["params"];
            let Some(id) = p["id"].as_str() else { return };
            let actions = p["actions"].as_array().map_or_else(
                || vec!["accept".to_owned(), "reject".to_owned(), "edit".to_owned()],
                |a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                },
            );
            on_message(DaemonMessage::PromptShow {
                id: id.to_owned(),
                kind: p["type"].as_str().unwrap_or("text").to_owned(),
                title: p["title"].as_str().unwrap_or("").to_owned(),
                body: p["body"].as_str().unwrap_or("").to_owned(),
                diff: if p["diff"].is_null() {
                    None
                } else {
                    Some(p["diff"].clone())
                },
                actions,
            });
        }
        "block.push" => {
            let p = &v["params"];
            let Some(id) = p["id"].as_str() else { return };
            on_message(DaemonMessage::BlockPush {
                id: id.to_owned(),
                kind: p["type"].as_str().unwrap_or("text").to_owned(),
                title: p["title"].as_str().unwrap_or("").to_owned(),
                body: p["body"].as_str().unwrap_or("").to_owned(),
            });
        }
        "block.clear" => {
            if let Some(id) = v["params"]["id"].as_str() {
                on_message(DaemonMessage::BlockClear { id: id.to_owned() });
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
            other => panic!("unexpected: {other:?}"),
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

    #[test]
    fn notification_prompt_show_parsed() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "prompt.show",
            "params": {
                "id": "p1",
                "type": "diff",
                "title": "edit x.rs",
                "body": "context",
                "diff": { "path": "src/x.rs", "raw": "+a\n-b\n" },
                "actions": ["accept", "reject", "edit"]
            }
        });
        let mut got: Option<DaemonMessage> = None;
        handle_notification("prompt.show", &v, &mut |m| got = Some(m));
        match got.unwrap() {
            DaemonMessage::PromptShow {
                id,
                kind,
                title,
                actions,
                diff,
                ..
            } => {
                assert_eq!(id, "p1");
                assert_eq!(kind, "diff");
                assert_eq!(title, "edit x.rs");
                assert_eq!(actions, vec!["accept", "reject", "edit"]);
                assert!(diff.is_some());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn notification_prompt_show_defaults_actions() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "prompt.show",
            "params": { "id": "p2", "type": "text", "title": "run" }
        });
        let mut got: Option<DaemonMessage> = None;
        handle_notification("prompt.show", &v, &mut |m| got = Some(m));
        match got.unwrap() {
            DaemonMessage::PromptShow { actions, diff, .. } => {
                assert_eq!(actions, vec!["accept", "reject", "edit"]);
                assert!(diff.is_none());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn notification_block_push_parsed() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "block.push",
            "params": { "id": "b1", "type": "text", "title": "Note", "body": "hi" }
        });
        let mut got: Option<DaemonMessage> = None;
        handle_notification("block.push", &v, &mut |m| got = Some(m));
        match got.unwrap() {
            DaemonMessage::BlockPush {
                id, title, body, ..
            } => {
                assert_eq!(id, "b1");
                assert_eq!(title, "Note");
                assert_eq!(body, "hi");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn notification_block_clear_parsed() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "block.clear",
            "params": { "id": "b1" }
        });
        let mut got: Option<DaemonMessage> = None;
        handle_notification("block.clear", &v, &mut |m| got = Some(m));
        assert!(matches!(got, Some(DaemonMessage::BlockClear { id }) if id == "b1"));
    }

    #[test]
    fn notification_prompt_show_missing_id_ignored() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "prompt.show",
            "params": { "title": "x" }
        });
        let mut count = 0u32;
        handle_notification("prompt.show", &v, &mut |_| count += 1);
        assert_eq!(count, 0);
    }
}

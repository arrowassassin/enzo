//! CDP transport — WebSocket send/receive loop.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

/// A server-pushed CDP event.
#[derive(Debug, Clone)]
pub struct CdpEvent {
    /// CDP method name (e.g. `"Page.loadEventFired"`).
    pub method: String,
    /// Event parameters.
    pub params: Value,
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;
type WsSink = Arc<
    Mutex<futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
>;

/// Low-level CDP connection over a WebSocket.
pub struct CdpSession {
    sink: WsSink,
    pending: PendingMap,
    next_id: Arc<AtomicU64>,
}

impl CdpSession {
    /// Connect to a CDP endpoint (e.g. `ws://localhost:9222/devtools/page/<id>`).
    pub async fn connect(
        ws_url: &str,
        on_event: impl FnMut(CdpEvent) + Send + 'static,
    ) -> anyhow::Result<Self> {
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .with_context(|| format!("connect CDP {ws_url}"))?;
        let (sink, stream) = ws_stream.split();

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_r = Arc::clone(&pending);
        let sink = Arc::new(Mutex::new(sink));

        tokio::spawn(async move {
            if let Err(e) = read_loop(stream, pending_r, on_event).await {
                log::warn!("CDP read loop: {e:#}");
            }
        });

        Ok(Self {
            sink,
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    /// Send a CDP command and wait for its result.
    pub async fn call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel::<Value>();
        self.pending.lock().await.insert(id, tx);

        let msg = json!({ "id": id, "method": method, "params": params });
        let text = serde_json::to_string(&msg).context("serialize")?;
        self.sink
            .lock()
            .await
            .send(Message::Text(text))
            .await
            .context("send CDP")?;

        let resp = rx.await.context("response channel closed")?;
        if let Some(err) = resp.get("error") {
            anyhow::bail!("CDP error: {err}");
        }
        Ok(resp["result"].clone())
    }
}

async fn read_loop(
    mut stream: impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    pending: PendingMap,
    mut on_event: impl FnMut(CdpEvent),
) -> anyhow::Result<()> {
    while let Some(msg) = stream.next().await {
        let text = match msg.context("ws receive")? {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("CDP parse error: {e}");
                continue;
            }
        };

        if v.get("method").is_some() {
            let method = v["method"].as_str().unwrap_or("").to_owned();
            let params = v["params"].clone();
            on_event(CdpEvent { method, params });
        } else if let Some(id) = v.get("id").and_then(Value::as_u64)
            && let Some(tx) = pending.lock().await.remove(&id)
        {
            let _ = tx.send(v);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CdpEvent;

    #[test]
    fn cdp_event_fields() {
        let e = CdpEvent {
            method: "Page.loadEventFired".into(),
            params: serde_json::json!({ "timestamp": 1.0 }),
        };
        assert_eq!(e.method, "Page.loadEventFired");
        assert!(e.params["timestamp"].as_f64().is_some());
    }
}

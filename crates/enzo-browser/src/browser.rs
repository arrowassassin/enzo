//! Browser launcher and tab manager.
//!
//! [`Browser`] discovers open Chrome/Chromium targets via the HTTP `/json`
//! endpoint and attaches to them as [`Page`] handles.

use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;

use crate::cdp::CdpSession;
use crate::page::Page;

/// A connection to a running Chrome/Chromium browser.
pub struct Browser {
    /// Base URL of the remote debugging HTTP endpoint (e.g. `http://localhost:9222`).
    debug_url: String,
}

/// Metadata for one open tab / target returned by `/json`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// Unique target ID.
    pub id: String,
    /// Page title.
    pub title: String,
    /// Current URL.
    pub url: String,
    /// WebSocket debugger URL to attach to.
    #[serde(rename = "webSocketDebuggerUrl")]
    pub ws_debugger_url: String,
    /// Target type (`"page"`, `"worker"`, …).
    #[serde(rename = "type")]
    pub target_type: String,
}

impl Browser {
    /// Connect to a browser already running with `--remote-debugging-port=<port>`.
    ///
    /// `debug_url` should be the HTTP base URL, e.g. `"http://localhost:9222"`.
    #[must_use]
    pub fn connect(debug_url: &str) -> Self {
        Self {
            debug_url: debug_url.trim_end_matches('/').to_owned(),
        }
    }

    /// List all open targets (tabs, workers, etc.).
    pub async fn targets(&self) -> anyhow::Result<Vec<Target>> {
        let url = format!("{}/json", self.debug_url);
        let body = Self::http_get(&url).await?;
        serde_json::from_value::<Vec<Target>>(body).context("parse targets")
    }

    /// List only `"page"` type targets.
    pub async fn pages(&self) -> anyhow::Result<Vec<Target>> {
        Ok(self
            .targets()
            .await?
            .into_iter()
            .filter(|t| t.target_type == "page")
            .collect())
    }

    /// Attach to the first available page target.
    pub async fn attach_first_page(
        &self,
        on_event: impl FnMut(crate::cdp::CdpEvent) + Send + 'static,
    ) -> anyhow::Result<Page> {
        let pages = self.pages().await?;
        let target = pages
            .into_iter()
            .next()
            .context("no page targets available")?;
        self.attach(&target.ws_debugger_url, on_event).await
    }

    /// Attach to a specific WebSocket debugger URL.
    pub async fn attach(
        &self,
        ws_url: &str,
        on_event: impl FnMut(crate::cdp::CdpEvent) + Send + 'static,
    ) -> anyhow::Result<Page> {
        let session = CdpSession::connect(ws_url, on_event).await?;
        Ok(Page::new(session))
    }

    /// Fetch the browser version via `/json/version`.
    pub async fn version(&self) -> anyhow::Result<Value> {
        let url = format!("{}/json/version", self.debug_url);
        Self::http_get(&url).await
    }

    async fn http_get(url: &str) -> anyhow::Result<Value> {
        // Avoid pulling in reqwest; use tokio's TCP + manual HTTP/1.1 GET.
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpStream;

        let parsed = url::Url::parse(url).with_context(|| format!("parse url {url}"))?;
        let host = parsed.host_str().context("missing host")?;
        let port = parsed.port().unwrap_or(80);
        let path = parsed.path();
        let addr = format!("{host}:{port}");

        let mut stream = TcpStream::connect(&addr)
            .await
            .with_context(|| format!("connect {addr}"))?;

        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .context("send request")?;

        let mut reader = BufReader::new(stream);
        // Skip headers until blank line.
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.context("read header")?;
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }

        let mut body = String::new();
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).await.context("read body")?;
            if n == 0 {
                break;
            }
            body.push_str(&line);
        }

        serde_json::from_str(body.trim()).context("parse JSON body")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_connect_trims_slash() {
        let b = Browser::connect("http://localhost:9222/");
        assert_eq!(b.debug_url, "http://localhost:9222");
    }

    #[test]
    fn target_deserializes() {
        let json = serde_json::json!({
            "id": "abc123",
            "title": "My Page",
            "url": "https://example.com",
            "webSocketDebuggerUrl": "ws://localhost:9222/devtools/page/abc123",
            "type": "page"
        });
        let t: Target = serde_json::from_value(json).unwrap();
        assert_eq!(t.id, "abc123");
        assert_eq!(t.target_type, "page");
        assert!(t.ws_debugger_url.starts_with("ws://"));
    }
}

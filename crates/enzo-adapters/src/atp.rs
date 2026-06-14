//! Blocking ATP client for use in adapter binaries.
//!
//! Adapters run as ordinary processes that proxy an AI CLI's stdio through a
//! PTY. They speak ATP over a Unix socket to the enzo-daemon using synchronous
//! (blocking) I/O — no async runtime needed.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use anyhow::Context;
use serde_json::{Value, json};

/// Blocking JSON-RPC 2.0 client connected to an enzo-daemon ATP socket.
pub struct AtpClient {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl AtpClient {
    /// Connect to the ATP socket at `path`.
    pub fn connect(path: &str) -> anyhow::Result<Self> {
        let stream =
            UnixStream::connect(path).with_context(|| format!("connect to ATP socket {path}"))?;
        // `prompt.show` may block for up to 5 minutes — use no read timeout so
        // the response arrives regardless of how long the user takes.
        stream.set_read_timeout(None)?;
        let reader = BufReader::new(stream.try_clone().context("clone socket")?);
        Ok(Self {
            writer: stream,
            reader,
            next_id: 1,
        })
    }

    /// Send one JSON-RPC request and wait (blocking) for the matching response.
    pub fn call(&mut self, method: &str, params: &Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut line = serde_json::to_string(&request).context("serialize request")?;
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .context("write request")?;

        let mut resp_line = String::new();
        self.reader
            .read_line(&mut resp_line)
            .context("read response")?;

        let resp: Value = serde_json::from_str(resp_line.trim()).context("parse response")?;

        if let Some(err) = resp.get("error") {
            anyhow::bail!(
                "ATP error {}: {}",
                err["code"],
                err["message"].as_str().unwrap_or("unknown")
            );
        }
        Ok(resp["result"].clone())
    }

    /// Send a `prompt.show` request and block until the display client responds.
    ///
    /// Returns the chosen action: `"accept"`, `"reject"`, or `"edit"`.
    pub fn prompt_show(
        &mut self,
        id: &str,
        prompt_type: &str,
        title: &str,
        body: &str,
        diff: Option<&Value>,
        actions: &[&str],
    ) -> anyhow::Result<String> {
        let mut params = json!({
            "id": id,
            "type": prompt_type,
            "title": title,
            "body": body,
            "actions": actions,
        });
        if let Some(d) = diff {
            params["diff"] = d.clone();
        }
        let result = self.call("prompt.show", &params)?;
        Ok(result["action"].as_str().unwrap_or("reject").to_owned())
    }

    /// Push a content block (fire-and-forget — does not wait for user action).
    pub fn block_push(
        &mut self,
        id: &str,
        block_type: &str,
        title: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        self.call(
            "block.push",
            &json!({
                "id": id,
                "type": block_type,
                "title": title,
                "body": body,
            }),
        )?;
        Ok(())
    }
}

/// Try to connect to the ATP socket, returning `None` if the daemon is not running.
///
/// Adapters use this for graceful degradation: if the socket is unavailable they
/// fall back to plain terminal I/O (Layer 0).
#[must_use]
pub fn try_connect(path: &str) -> Option<AtpClient> {
    AtpClient::connect(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_connect_returns_none_for_nonexistent_socket() {
        assert!(try_connect("/tmp/enzo-no-such-socket-42.sock").is_none());
    }
}

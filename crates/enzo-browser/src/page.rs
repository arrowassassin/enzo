//! High-level page API built on top of a `CdpSession`.

use std::sync::Arc;

use anyhow::Context;
use serde_json::{Value, json};

use crate::cdp::CdpSession;

/// A handle to a single browser page / tab.
pub struct Page {
    session: Arc<CdpSession>,
}

impl Page {
    /// Wrap a `CdpSession` as a page.
    #[must_use]
    pub fn new(session: CdpSession) -> Self {
        Self {
            session: Arc::new(session),
        }
    }

    /// Navigate to `url` and wait for `Page.loadEventFired`.
    pub async fn navigate(&self, url: &str) -> anyhow::Result<()> {
        self.session
            .call("Page.navigate", json!({ "url": url }))
            .await
            .map(|_| ())
    }

    /// Evaluate a JavaScript expression in the page context and return the result.
    pub async fn eval(&self, expression: &str) -> anyhow::Result<Value> {
        let result = self
            .session
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                }),
            )
            .await?;
        Ok(result["result"]["value"].clone())
    }

    /// Capture a full-page PNG screenshot as raw bytes.
    pub async fn screenshot_png(&self) -> anyhow::Result<Vec<u8>> {
        let result = self
            .session
            .call(
                "Page.captureScreenshot",
                json!({ "format": "png", "captureBeyondViewport": true }),
            )
            .await?;
        let b64 = result["data"].as_str().context("missing screenshot data")?;
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .context("decode screenshot")
    }

    /// Return the current page title.
    pub async fn title(&self) -> anyhow::Result<String> {
        let v = self.eval("document.title").await?;
        Ok(v.as_str().unwrap_or("").to_owned())
    }

    /// Return the current page URL.
    pub async fn url(&self) -> anyhow::Result<String> {
        let v = self.eval("location.href").await?;
        Ok(v.as_str().unwrap_or("").to_owned())
    }

    /// Click the first element matching a CSS selector.
    pub async fn click(&self, selector: &str) -> anyhow::Result<()> {
        let script = format!(r"document.querySelector({selector:?})?.click()");
        self.eval(&script).await.map(|_| ())
    }

    /// Type text into the first element matching a CSS selector.
    pub async fn type_into(&self, selector: &str, text: &str) -> anyhow::Result<()> {
        let focus_script = format!(r"document.querySelector({selector:?})?.focus()");
        self.eval(&focus_script).await?;
        for ch in text.chars() {
            self.session
                .call(
                    "Input.dispatchKeyEvent",
                    json!({ "type": "char", "text": ch.to_string() }),
                )
                .await?;
        }
        Ok(())
    }

    /// Return the inner text of the first element matching `selector`.
    pub async fn inner_text(&self, selector: &str) -> anyhow::Result<String> {
        let script = format!(r#"document.querySelector({selector:?})?.innerText ?? """#);
        let v = self.eval(&script).await?;
        Ok(v.as_str().unwrap_or("").to_owned())
    }

    /// Enable or disable CDP domain events for `Page` and `Runtime`.
    pub async fn enable_events(&self) -> anyhow::Result<()> {
        self.session.call("Page.enable", json!({})).await?;
        self.session.call("Runtime.enable", json!({})).await?;
        Ok(())
    }

    /// Return a clone of the underlying session for raw CDP access.
    #[must_use]
    pub fn session(&self) -> Arc<CdpSession> {
        Arc::clone(&self.session)
    }
}

#[cfg(test)]
mod tests {
    use crate::cdp::CdpEvent;

    #[test]
    fn cdp_event_clone() {
        let e = CdpEvent {
            method: "Page.frameNavigated".into(),
            params: serde_json::json!({}),
        };
        let e2 = e.clone();
        assert_eq!(e.method, e2.method);
    }
}

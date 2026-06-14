//! Headless Chrome/Chromium launcher.
//!
//! Spawns a headless browser with the `DevTools` protocol enabled, waits for the
//! `/json/version` endpoint to come up, and hands back a [`Browser`] plus the
//! child process handle (kept alive by the caller; killed on close).

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;

use crate::browser::Browser;

/// The remote-debugging port used for the launched browser.
pub const DEBUG_PORT: u16 = 9222;

/// A launched headless browser process + a connected [`Browser`] handle.
pub struct Launched {
    /// The `DevTools` connection.
    pub browser: Browser,
    /// The child process (kill to shut the browser down).
    pub child: Child,
}

/// Locate a Chrome/Chromium executable.
///
/// Honours `$ENZO_CHROME`, then falls back to common platform locations.
#[must_use]
pub fn find_chrome() -> Option<String> {
    if let Ok(path) = std::env::var("ENZO_CHROME")
        && !path.is_empty()
    {
        return Some(path);
    }
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ]
    } else if cfg!(target_os = "windows") {
        &[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ]
    } else {
        &[
            "/usr/bin/google-chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
        ]
    };
    candidates
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|p| (*p).to_owned())
}

/// Launch a headless browser sized `width`×`height` and connect to it.
///
/// # Errors
/// Returns an error if no Chrome is found or the debug endpoint never comes up.
pub async fn launch(width: u32, height: u32) -> anyhow::Result<Launched> {
    let chrome = find_chrome()
        .context("no Chrome/Chromium found — set $ENZO_CHROME to the executable path")?;

    let user_dir = std::env::temp_dir().join(format!("enzo-chrome-{}", std::process::id()));
    let child = Command::new(&chrome)
        .arg("--headless=new")
        .arg(format!("--remote-debugging-port={DEBUG_PORT}"))
        .arg(format!("--user-data-dir={}", user_dir.display()))
        .arg(format!("--window-size={width},{height}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-gpu")
        .arg("--hide-scrollbars")
        .arg("about:blank")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn chrome ({chrome})"))?;

    let debug_url = format!("http://localhost:{DEBUG_PORT}");
    let browser = Browser::connect(&debug_url);

    // Wait for the debug endpoint to accept connections (up to 10s).
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if browser.version().await.is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            anyhow::bail!("chrome debug endpoint did not come up on {debug_url}");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    Ok(Launched { browser, child })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_chrome_does_not_panic() {
        // Returns Some(path) if a browser is installed, None otherwise — both fine.
        let _ = find_chrome();
    }

    #[test]
    fn debug_port_is_set() {
        assert_eq!(DEBUG_PORT, 9222);
    }
}

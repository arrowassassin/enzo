//! Headless browser automation for Enzo.
//!
//! Connects to a running Chromium instance with `--remote-debugging-port` set,
//! sends CDP commands over WebSocket, and provides high-level helpers for
//! navigation, screenshots, and DOM queries.
//!
//! # Quick start
//! ```no_run
//! use enzo_browser::Browser;
//! // Browser::connect("ws://localhost:9222/json").await?
//! ```

pub mod browser;
pub mod cdp;
pub mod page;

pub use browser::Browser;
pub use cdp::CdpEvent;
pub use page::Page;

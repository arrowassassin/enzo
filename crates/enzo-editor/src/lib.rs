//! Language services for the Enzo editor.
//!
//! - [`buffer`] — `ropey`-backed text buffer
//! - [`lang`] — language registry (extensions → LSP servers, formatters)
//! - [`highlight`] — tree-sitter syntax highlighting
//! - [`format`] — external formatter integration (rustfmt, black, prettier)
//! - [`lsp`] — Language Server Protocol client (JSON-RPC 2.0 over stdio)
//! - [`dap`] — Debug Adapter Protocol client (DAP header-framed over stdio)

pub mod buffer;
pub mod dap;
pub mod format;
pub mod highlight;
pub mod lang;
pub mod lsp;

pub use buffer::Buffer;
pub use highlight::{HighlightSpan, highlight};
pub use lang::{Formatter, Language, LspServer};

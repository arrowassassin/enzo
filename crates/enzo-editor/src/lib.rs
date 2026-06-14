//! Language service clients for the Enzo editor.
//!
//! - [`lsp`] — Language Server Protocol client (JSON-RPC 2.0 over stdio)
//! - [`dap`] — Debug Adapter Protocol client (DAP header-framed over stdio)

pub mod dap;
pub mod lsp;

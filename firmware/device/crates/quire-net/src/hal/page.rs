//! The Drop page and its PWA files, gzipped at build time (see `build.rs`).

/// `index.html`, gzipped.
pub static INDEX_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/index.html.gz"));
/// `sw.js`, gzipped.
pub static SW_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sw.js.gz"));
/// `manifest.webmanifest`, gzipped.
pub static MANIFEST_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/manifest.webmanifest.gz"));

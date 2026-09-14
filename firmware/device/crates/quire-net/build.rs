//! Compresses the Drop page (`firmware/web/drop/index.html`, one file with inline CSS and
//! JS) into `OUT_DIR/drop.html.gz`; the server sends it with `Content-Encoding: gzip`.

use std::io::Write;
use std::path::PathBuf;

/// The page must stay small: it lives in flash and is sent on every visit.
const MAX_GZIP_BYTES: usize = 60 * 1024;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../web/drop");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    for name in ["index.html", "sw.js", "manifest.webmanifest"] {
        let src = root.join(name);
        println!("cargo:rerun-if-changed={}", src.display());
        let data = std::fs::read(&src).unwrap_or_else(|e| panic!("read {}: {e}", src.display()));
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(&data).expect("gzip");
        let gz = enc.finish().expect("gzip");
        if name == "index.html" {
            assert!(gz.len() <= MAX_GZIP_BYTES, "drop page is {} B gzipped, over the {MAX_GZIP_BYTES} B budget", gz.len());
            println!("cargo:warning=drop page: {} B raw, {} B gzipped", data.len(), gz.len());
        }
        std::fs::write(out.join(format!("{name}.gz")), gz).expect("write");
    }
}

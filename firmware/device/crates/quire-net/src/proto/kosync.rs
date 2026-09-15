//! The KOReader progress-sync protocol (kosync): documents are named by the MD5 of
//! samples of the book file (KOReader's "partial" digest, so both readers agree on the
//! id for the same file), the user's key is the MD5 of the password, and progress is a
//! `PUT /syncs/progress` with a JSON body or a `GET /syncs/progress/<document>`.

use alloc::string::String;
use alloc::vec::Vec;

use quire_fs::ReadAt;

use super::json::Json;
use super::jsonlite;
use super::md5::{self, Md5};

/// The device name sent with each position.
pub const DEVICE: &str = "Quire";

/// KOReader's partial digest of a file: 1 KB samples at 1 KB · 4^i for i = −1…10.
pub fn partial_md5(file: &dyn ReadAt) -> String {
    let mut h = Md5::new();
    let mut buf = [0u8; 1024];
    let len = file.len();
    for i in -1i32..=10 {
        let offset: u64 = if i < 0 { 1024 >> 2 } else { 1024u64 << (2 * i as u32) };
        if offset >= len {
            break;
        }
        let want = ((len - offset).min(1024)) as usize;
        match file.read_at(offset, &mut buf[..want]) {
            Ok(n) if n > 0 => h.update(&buf[..n]),
            _ => break,
        }
    }
    md5::to_hex(&h.finalize())
}

/// The `x-auth-key` header value for a password.
pub fn auth_key(password: &str) -> String {
    md5::hex_of(password.as_bytes())
}

/// The base URL with any trailing slash trimmed.
pub fn base(url: &str) -> String {
    String::from(url.trim().trim_end_matches('/'))
}

/// The progress string Quire stores server-side: `quire:<section>:<chars>`.
pub fn progress_text(section: u16, chars: u32) -> String {
    alloc::format!("quire:{section}:{chars}")
}

/// A `PUT /syncs/progress` body.
pub fn put_body(document: &str, progress: &str, percentage: f32, device_id: &str) -> String {
    let mut j = Json::new();
    j.obj().kv_str("document", document).kv_str("progress", progress).key("percentage");
    j.raw(&fmt_pct(percentage));
    j.kv_str("device", DEVICE).kv_str("device_id", device_id).end();
    j.finish()
}

fn fmt_pct(p: f32) -> String {
    let p = p.clamp(0.0, 1.0);
    let n = (p * 10_000.0 + 0.5) as u32;
    alloc::format!("{}.{:04}", n / 10_000, n % 10_000)
}

/// A position as the server returns it.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerProgress {
    /// The progress string (an xpointer for KOReader, `quire:…` for Quire).
    pub progress: String,
    /// 0–1.
    pub percentage: f32,
    /// Device name.
    pub device: String,
    /// Server time of the record (seconds).
    pub timestamp: u64,
}

/// Parse a `GET /syncs/progress/<document>` response; `None` when the server has nothing.
pub fn parse_progress(json: &[u8]) -> Option<ServerProgress> {
    let v = jsonlite::parse(json)?;
    let percentage = v.get("percentage")?.as_f64()? as f32;
    Some(ServerProgress {
        progress: v.get("progress").and_then(|p| p.as_str()).unwrap_or_default(),
        percentage,
        device: v.get("device").and_then(|p| p.as_str()).unwrap_or_default(),
        timestamp: v.get("timestamp").and_then(|t| t.as_i64()).unwrap_or(0).max(0) as u64,
    })
}

/// A `quire:<section>:<chars>` progress string.
pub fn parse_progress_text(s: &str) -> Option<(u16, u32)> {
    let rest = s.strip_prefix("quire:")?;
    let (a, b) = rest.split_once(':')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// The books to push: (document id, progress, percentage), computed by the caller.
pub type Positions = Vec<(String, String, f32)>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_samples() {
        // A 20 KB file: samples at 256, 1024, 4096, 16384; the next (65536) is past the end.
        let data: Vec<u8> = (0..20_480u32).map(|i| (i % 251) as u8).collect();
        let got = partial_md5(&data.as_slice());
        let mut h = Md5::new();
        for off in [256usize, 1024, 4096, 16384] {
            h.update(&data[off..off + 1024]);
        }
        assert_eq!(got, md5::to_hex(&h.finalize()));
        // A file shorter than the first sample offset hashes nothing.
        assert_eq!(partial_md5(&[1u8, 2, 3].as_slice()), md5::hex_of(b""));
    }

    #[test]
    fn bodies() {
        assert_eq!(auth_key("secret"), "5ebe2294ecd0e0f08eab7690d2a6ee69");
        let b = put_body("abc", "quire:3:1200", 0.4567, "dev1");
        assert_eq!(b, r#"{"document":"abc","progress":"quire:3:1200","percentage":0.4567,"device":"Quire","device_id":"dev1"}"#);
        assert_eq!(fmt_pct(1.5), "1.0000");
        let p = parse_progress(
            br#"{"document":"abc","progress":"quire:3:1200","percentage":0.4567,"device":"Quire","device_id":"x","timestamp":1700000000}"#,
        )
        .unwrap();
        assert_eq!(p.percentage, 0.4567);
        assert_eq!(p.timestamp, 1_700_000_000);
        assert_eq!(parse_progress_text(&p.progress), Some((3, 1200)));
        assert!(parse_progress(br#"{}"#).is_none());
        assert_eq!(base("https://sync.example.org/"), "https://sync.example.org");
    }
}

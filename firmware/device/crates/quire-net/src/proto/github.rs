//! GitHub releases: the latest release of `arrowassassin/quire` as an [`OtaInfo`], from
//! the REST API's JSON (scanned, not parsed into a tree; a truncated document still
//! yields what came before the cut, which for GitHub is everything but the notes).

use alloc::string::String;

use quire_ui::net::OtaInfo;

use super::jsonlite::Value;

/// The repository releases are read from.
pub const REPO: &str = "arrowassassin/quire";
/// The firmware asset name in a release.
pub const ASSET: &str = "quire-x3.bin";
/// The `releases/latest` endpoint.
pub const LATEST_URL: &str = "https://api.github.com/repos/arrowassassin/quire/releases/latest";
/// Most of the release JSON kept in RAM (notes past this point are cut).
pub const MAX_JSON: usize = 12 * 1024;

/// A release, as the updater needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// What the UI shows.
    pub info: OtaInfo,
    /// The published checksum file (`quire-x3.bin.sha256` or `SHA256SUMS`), if any.
    pub sha_url: Option<String>,
}

/// Parse the (possibly cut) release JSON.
pub fn parse_release(json: &[u8]) -> Result<Release, String> {
    let v = Value(json);
    let tag = v.get("tag_name").and_then(|t| t.as_str()).ok_or_else(|| String::from("no release found"))?;
    let version = String::from(tag.trim_start_matches(['v', 'V']));
    let mut notes = v.get("body").and_then(|b| b.as_str()).unwrap_or_default();
    if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
        if notes.is_empty() {
            notes = name;
        }
    }
    let mut url = None;
    let mut size = 0u64;
    let mut sha_url = None;
    if let Some(assets) = v.get("assets") {
        for a in assets.items() {
            let name = a.get("name").and_then(|n| n.as_str()).unwrap_or_default();
            let Some(link) = a.get("browser_download_url").and_then(|u| u.as_str()) else { continue };
            if name == ASSET {
                size = a.get("size").and_then(|s| s.as_i64()).unwrap_or(0).max(0) as u64;
                url = Some(link);
            } else if (name == "quire-x3.bin.sha256" || name == "SHA256SUMS" || name == "sha256sums.txt")
                && (sha_url.is_none() || name.starts_with(ASSET))
            {
                sha_url = Some(link);
            }
        }
    }
    let url = url.ok_or_else(|| alloc::format!("release {version} has no {ASSET}"))?;
    Ok(Release { info: OtaInfo { version, notes: String::from(notes.trim()), url, size }, sha_url })
}

/// Whether `latest` is newer than the running `current` (dotted numbers compared
/// numerically; anything unparsable counts as different, so it is offered).
pub fn is_newer(latest: &str, current: &str) -> bool {
    let nums = |s: &str| -> Option<[u32; 3]> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let core = s.split(['-', '+', ' ']).next()?;
        let mut out = [0u32; 3];
        for (i, p) in core.split('.').enumerate() {
            if i >= 3 {
                break;
            }
            out[i] = p.parse().ok()?;
        }
        Some(out)
    };
    match (nums(latest), nums(current)) {
        (Some(l), Some(c)) => l > c,
        _ => latest.trim() != current.trim(),
    }
}

/// The digest for `asset` in a checksum file (`<hex>  <name>` lines or a bare hex).
pub fn sha_for(text: &str, asset: &str) -> Option<[u8; 32]> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(hex) = parts.next() else { continue };
        let name = parts.next().map(|n| n.trim_start_matches('*'));
        if hex.len() == 64 && (name.is_none() || name == Some(asset) || name.is_some_and(|n| n.ends_with(asset))) {
            let mut out = [0u8; 32];
            for i in 0..32 {
                out[i] = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
            }
            return Some(out);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &[u8] = br#"{"url":"x","tag_name":"v0.3.1","name":"Quire 0.3.1","draft":false,"assets":[
      {"name":"quire-x3.bin.sha256","size":90,"browser_download_url":"https://github.com/arrowassassin/quire/releases/download/v0.3.1/quire-x3.bin.sha256"},
      {"name":"quire-x3.bin","size":3987872,"browser_download_url":"https://github.com/arrowassassin/quire/releases/download/v0.3.1/quire-x3.bin"}],
      "body":"* Bookshop\r\n* Sync"}"#;

    #[test]
    fn parses_release() {
        let r = parse_release(JSON).unwrap();
        assert_eq!(r.info.version, "0.3.1");
        assert_eq!(r.info.size, 3_987_872);
        assert!(r.info.url.ends_with("/quire-x3.bin"));
        assert_eq!(r.info.notes, "* Bookshop\r\n* Sync");
        assert!(r.sha_url.unwrap().ends_with(".sha256"));
        // Cut before the notes: still a usable release.
        let cut = &JSON[..JSON.len() - 30];
        let r = parse_release(cut).unwrap();
        assert_eq!(r.info.notes, "Quire 0.3.1");
        assert!(parse_release(br#"{"message":"Not Found"}"#).is_err());
        assert!(parse_release(br#"{"tag_name":"v1","assets":[]}"#).unwrap_err().contains("no quire-x3.bin"));
    }

    #[test]
    fn versions() {
        assert!(is_newer("v0.3.1", "0.3.0"));
        assert!(is_newer("1.0.0", "0.9.12"));
        assert!(!is_newer("0.3.1", "0.3.1"));
        assert!(!is_newer("0.3.1", "0.4.0"));
        assert!(is_newer("nightly-2", "nightly-1"));
    }

    #[test]
    fn checksums() {
        let hex = "aa".repeat(32);
        assert_eq!(sha_for(&alloc::format!("{hex}  quire-x3.bin\nbb  other"), "quire-x3.bin"), Some([0xaa; 32]));
        assert_eq!(sha_for(&hex, "quire-x3.bin"), Some([0xaa; 32]));
        assert_eq!(sha_for("zz", "quire-x3.bin"), None);
    }
}

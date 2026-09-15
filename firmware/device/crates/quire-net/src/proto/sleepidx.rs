//! The sleep-pack index and manifests published in the repository (`sleep-packs/`):
//! what the picker lists, and which files a pack install fetches.

use alloc::string::String;
use alloc::vec::Vec;

use super::jsonlite;

/// Where the packs live.
pub const BASE_URL: &str = "https://raw.githubusercontent.com/arrowassassin/quire/main/sleep-packs/";
/// The index.
pub const INDEX_URL: &str = "https://raw.githubusercontent.com/arrowassassin/quire/main/sleep-packs/index.json";
/// Longest index or manifest accepted (bytes).
pub const MAX_JSON: usize = 16 * 1024;

/// One pack of the index: id, name, description, image count, download bytes.
pub type PackRow = (String, String, String, u16, u64);

/// A name that stays inside its folder.
fn plain(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 48
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        && !s.starts_with('.')
}

/// The packs listed in `index.json`.
pub fn parse_index(json: &[u8]) -> Vec<PackRow> {
    let Some(v) = jsonlite::parse(json) else { return Vec::new() };
    let Some(packs) = v.get("packs") else { return Vec::new() };
    packs
        .items()
        .filter_map(|p| {
            let id = p.get("id")?.as_str()?;
            if !plain(&id) {
                return None;
            }
            let name = p.get("name").and_then(|n| n.as_str()).unwrap_or_else(|| id.clone());
            let desc = p.get("description").and_then(|n| n.as_str()).unwrap_or_default();
            let images = p.get("images").and_then(|n| n.as_i64()).unwrap_or(0).clamp(0, u16::MAX as i64) as u16;
            let bytes = p.get("bytes_z").or_else(|| p.get("bytes")).and_then(|n| n.as_i64()).unwrap_or(0).max(0) as u64;
            Some((id, name, desc, images, bytes))
        })
        .collect()
}

/// The compressed image files a manifest names (`z`, else `<file>.z`), in order.
pub fn parse_pack_files(json: &[u8]) -> Vec<String> {
    let Some(v) = jsonlite::parse(json) else { return Vec::new() };
    let Some(images) = v.get("images") else { return Vec::new() };
    images
        .items()
        .filter_map(|i| {
            let z = match i.get("z").and_then(|z| z.as_str()) {
                Some(z) => z,
                None => alloc::format!("{}.z", i.get("file")?.as_str()?),
            };
            plain(&z).then_some(z)
        })
        .collect()
}

/// The URL of a file inside a pack.
pub fn pack_file_url(id: &str, file: &str) -> String {
    alloc::format!("{BASE_URL}{id}/{file}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_manifest() {
        let idx = br#"{"version":1,"packs":[{"id":"mountains","name":"Mountains","description":"Ridges","images":5,"bytes":261415,"bytes_z":67285,"path":"mountains/pack.json"},{"id":"../x","name":"bad"}]}"#;
        let rows = parse_index(idx);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], (String::from("mountains"), String::from("Mountains"), String::from("Ridges"), 5, 67285));
        let pack = br#"{"id":"mountains","images":[{"file":"01.pbm","z":"01.pbm.z"},{"file":"02.pbm"},{"file":"/etc/x"}]}"#;
        assert_eq!(parse_pack_files(pack), ["01.pbm.z", "02.pbm.z"]);
        assert_eq!(pack_file_url("mountains", "01.pbm.z"), alloc::format!("{BASE_URL}mountains/01.pbm.z"));
        assert!(parse_index(b"nope").is_empty());
    }
}

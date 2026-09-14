//! Page-index cache: for each typography profile and section, the positions where pages
//! start, so a page turn is a lookup and a resume is exact. Built on first use and, for
//! the rest of the book, during idle time.

use alloc::vec::Vec;
use quire_fs::Fs;
use quire_layout::{Geometry, Pos, Profile};
use serde::{Deserialize, Serialize};

use crate::id::fnv1a;
use crate::{Book, LibError, LibResult};

/// A cached index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct IndexFile {
    /// Section byte length when built (stale detection).
    bytes: u32,
    starts: Vec<Pos>,
}

/// Key for a profile + geometry.
pub fn profile_key(profile: &Profile, geom: &Geometry) -> u32 {
    let p = postcard::to_allocvec(profile).unwrap_or_default();
    let mut h = fnv1a(0xcbf29ce484222325, &p);
    for v in [geom.w, geom.h, geom.text.w, geom.text.h, geom.text.x as u32, geom.text.y as u32, geom.line_h as u32] {
        h = fnv1a(h, &v.to_le_bytes());
    }
    (h ^ (h >> 32)) as u32
}

fn index_dir(dir: &str, key: u32) -> alloc::string::String {
    alloc::format!("{dir}/pages/{key:08x}")
}
fn index_path(dir: &str, key: u32, section: u16) -> alloc::string::String {
    alloc::format!("{dir}/pages/{key:08x}/{section:04}.idx")
}

/// Load a section's page starts if cached and current.
pub fn load<F: Fs>(fs: &F, book: &Book, key: u32, section: u16) -> Option<Vec<Pos>> {
    let bytes = fs.read_to_vec(&index_path(&book.dir, key, section)).ok()?;
    let f: IndexFile = postcard::from_bytes(&bytes).ok()?;
    let expect = book.sections.get(section as usize)?.bytes;
    if f.bytes != expect || f.starts.is_empty() {
        return None;
    }
    Some(f.starts)
}

/// Save a section's page starts.
pub fn save<F: Fs>(fs: &F, book: &Book, key: u32, section: u16, starts: &[Pos]) -> LibResult<()> {
    let dir = index_dir(&book.dir, key);
    if !fs.exists(&dir) {
        fs.mkdir_all(&dir)?;
    }
    let bytes = book.sections.get(section as usize).map(|s| s.bytes).unwrap_or(0);
    let f = IndexFile { bytes, starts: starts.to_vec() };
    let enc = postcard::to_allocvec(&f).map_err(|_| LibError::Corrupt("index encode"))?;
    fs.write_atomic(&index_path(&book.dir, key, section), &enc)?;
    Ok(())
}

/// Get the page starts for a section, building and caching them when missing.
pub fn get_or_build<F: Fs>(fs: &F, book: &Book, key: u32, section: u16, data: &[u8], profile: Profile, geom: Geometry) -> Vec<Pos> {
    if let Some(s) = load(fs, book, key, section) {
        return s;
    }
    let starts = quire_layout::build_index(data, profile, geom);
    let starts = if starts.is_empty() { alloc::vec![Pos::START] } else { starts };
    let _ = save(fs, book, key, section, &starts);
    starts
}

/// Sections without a cached index for this profile.
pub fn missing<F: Fs>(fs: &F, book: &Book, key: u32) -> Vec<u16> {
    (0..book.section_count()).filter(|s| !fs.exists(&index_path(&book.dir, key, *s))).collect()
}

/// Build one missing section's index (an idle-time step). Returns false when complete.
pub fn build_next<F: Fs>(fs: &F, book: &Book, key: u32, profile: Profile, geom: Geometry) -> bool {
    let Some(section) = missing(fs, book, key).into_iter().next() else { return false };
    if let Ok(data) = book.section(fs, section) {
        let _ = get_or_build(fs, book, key, section, &data, profile, geom);
    } else {
        let _ = save(fs, book, key, section, &[Pos::START]);
    }
    true
}

/// Page counts per section for a complete index, or None if any is missing.
pub fn page_counts<F: Fs>(fs: &F, book: &Book, key: u32) -> Option<Vec<u16>> {
    let mut out = Vec::with_capacity(book.sections.len());
    for s in 0..book.section_count() {
        let bytes = fs.read_to_vec(&index_path(&book.dir, key, s)).ok()?;
        let f: IndexFile = postcard::from_bytes(&bytes).ok()?;
        out.push(f.starts.len().min(u16::MAX as usize) as u16);
    }
    Some(out)
}

/// Remove every cached index except `keep` (called when the profile changes twice, so a
/// book never carries more than two profiles' worth of indexes).
pub fn purge_except<F: Fs>(fs: &F, book: &Book, keep: &[u32]) {
    let pages = alloc::format!("{}/pages", book.dir);
    let Ok(entries) = fs.read_dir(&pages) else { return };
    for e in entries {
        if !e.is_dir {
            continue;
        }
        let key = u32::from_str_radix(&e.name, 16).ok();
        if key.map(|k| keep.contains(&k)).unwrap_or(false) {
            continue;
        }
        let d = quire_fs::join(&pages, &e.name);
        let _ = crate::cache::clear_dir(fs, &d, 0);
        let _ = fs.remove(&d);
    }
}

/// Index of the page containing `pos` (the last start at or before it).
pub fn page_of(starts: &[Pos], pos: Pos) -> usize {
    starts.partition_point(|s| *s <= pos).saturating_sub(1)
}

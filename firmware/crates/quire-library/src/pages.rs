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
    // A regenerable cache file: written directly (a torn write fails the stale check).
    let mut w = fs.create(&index_path(&book.dir, key, section))?;
    quire_fs::WriteFile::write_all(&mut w, &enc)?;
    quire_fs::WriteFile::flush(&mut w)?;
    Ok(())
}

/// Get the page starts for a section, building and caching them when missing. Re-reads
/// the count table to record a fresh build; callers holding the table use
/// [`get_or_build_counted`].
pub fn get_or_build<F: Fs>(fs: &F, book: &Book, key: u32, section: u16, data: &[u8], profile: Profile, geom: Geometry) -> Vec<Pos> {
    let mut c = counts(fs, book, key);
    get_or_build_counted(fs, book, key, section, data, profile, geom, &mut c)
}

/// [`get_or_build`] with the caller's copy of the count table (`u16::MAX` = unknown): a
/// fresh build updates the slot and writes the table only when it changed, so a section
/// boundary costs no extra card read.
#[allow(clippy::too_many_arguments)]
pub fn get_or_build_counted<F: Fs>(
    fs: &F,
    book: &Book,
    key: u32,
    section: u16,
    data: &[u8],
    profile: Profile,
    geom: Geometry,
    counts: &mut [u16],
) -> Vec<Pos> {
    if let Some(s) = load(fs, book, key, section) {
        record_count_in(fs, book, key, section, clamp_count(s.len()), counts);
        return s;
    }
    let starts = quire_layout::build_index(data, profile, geom);
    let starts = if starts.is_empty() { alloc::vec![Pos::START] } else { starts };
    let _ = save(fs, book, key, section, &starts);
    record_count_in(fs, book, key, section, clamp_count(starts.len()), counts);
    starts
}

/// Like [`get_or_build_counted`], but the index must have a page starting at `pin` (the
/// reader's position when the typography changed, so the new page begins on the same
/// word): a cached index without it is rebuilt with the boundary kept.
#[allow(clippy::too_many_arguments)]
pub fn get_or_build_pinned<F: Fs>(
    fs: &F,
    book: &Book,
    key: u32,
    section: u16,
    data: &[u8],
    profile: Profile,
    geom: Geometry,
    counts: &mut [u16],
    pin: Pos,
) -> Vec<Pos> {
    if let Some(s) = load(fs, book, key, section) {
        if s.contains(&pin) {
            record_count_in(fs, book, key, section, clamp_count(s.len()), counts);
            return s;
        }
    }
    let starts = quire_layout::page::build_index_pinned(data, profile, geom, pin);
    let starts = if starts.is_empty() { alloc::vec![Pos::START] } else { starts };
    let _ = save(fs, book, key, section, &starts);
    record_count_in(fs, book, key, section, clamp_count(starts.len()), counts);
    starts
}

fn clamp_count(n: usize) -> u16 {
    n.min(u16::MAX as usize - 1) as u16
}

/// Path of the per-profile page-count table.
fn counts_path(dir: &str, key: u32) -> alloc::string::String {
    alloc::format!("{dir}/pages/{key:08x}/counts.bin")
}

/// Page counts per section for a profile (u16::MAX = not built yet). One small read.
pub fn counts<F: Fs>(fs: &F, book: &Book, key: u32) -> Vec<u16> {
    let n = book.sections.len();
    let mut out = alloc::vec![u16::MAX; n];
    if let Ok(bytes) = fs.read_to_vec(&counts_path(&book.dir, key)) {
        for (i, c) in bytes.as_chunks::<2>().0.iter().enumerate().take(n) {
            out[i] = u16::from_le_bytes(*c);
        }
    }
    out
}

fn write_counts<F: Fs>(fs: &F, book: &Book, key: u32, counts: &[u16]) {
    let dir = index_dir(&book.dir, key);
    if !fs.exists(&dir) {
        let _ = fs.mkdir_all(&dir);
    }
    let mut bytes = Vec::with_capacity(counts.len() * 2);
    for c in counts {
        bytes.extend_from_slice(&c.to_le_bytes());
    }
    if let Ok(mut w) = fs.create(&counts_path(&book.dir, key)) {
        let _ = quire_fs::WriteFile::write_all(&mut w, &bytes);
        let _ = quire_fs::WriteFile::flush(&mut w);
    }
}

/// Record a section's page count in the table (re-reads the table first).
pub fn record_count<F: Fs>(fs: &F, book: &Book, key: u32, section: u16, pages: u16) {
    let mut c = counts(fs, book, key);
    record_count_in(fs, book, key, section, pages, &mut c);
}

/// Record a section's page count in the caller's table, writing it out only if it changed.
pub fn record_count_in<F: Fs>(fs: &F, book: &Book, key: u32, section: u16, pages: u16, counts: &mut [u16]) {
    if let Some(slot) = counts.get_mut(section as usize) {
        if *slot != pages {
            *slot = pages;
            write_counts(fs, book, key, counts);
        }
    }
}

/// Sections without a cached index for this profile.
pub fn missing<F: Fs>(fs: &F, book: &Book, key: u32) -> Vec<u16> {
    counts(fs, book, key).iter().enumerate().filter(|(_, c)| **c == u16::MAX).map(|(i, _)| i as u16).collect()
}

/// Build the index of the first section listed in `todo` (an idle-time step). Returns the
/// section built, or None when the list is empty. Callers keep `todo` from [`missing`].
pub fn build_next<F: Fs>(fs: &F, book: &Book, key: u32, profile: Profile, geom: Geometry, todo: &mut Vec<u16>) -> Option<u16> {
    let mut c = counts(fs, book, key);
    build_next_counted(fs, book, key, profile, geom, todo, &mut c)
}

/// [`build_next`] with the caller's copy of the count table (updated in place, written
/// only when it changed).
pub fn build_next_counted<F: Fs>(
    fs: &F,
    book: &Book,
    key: u32,
    profile: Profile,
    geom: Geometry,
    todo: &mut Vec<u16>,
    counts: &mut [u16],
) -> Option<u16> {
    let section = todo.first().copied()?;
    todo.remove(0);
    match book.section(fs, section) {
        Ok(data) => {
            get_or_build_counted(fs, book, key, section, &data, profile, geom, counts);
        }
        Err(_) => {
            let _ = save(fs, book, key, section, &[Pos::START]);
            record_count_in(fs, book, key, section, 1, counts);
        }
    }
    Some(section)
}

/// Page counts per section for a complete index, or None if any is missing.
pub fn page_counts<F: Fs>(fs: &F, book: &Book, key: u32) -> Option<Vec<u16>> {
    let c = counts(fs, book, key);
    if c.contains(&u16::MAX) {
        None
    } else {
        Some(c)
    }
}

/// Total pages from a counts table (estimating unbuilt sections from their characters).
pub fn total_pages(book: &Book, counts: &[u16], chars_per_page: u32) -> u32 {
    book.sections
        .iter()
        .enumerate()
        .map(|(i, s)| match counts.get(i) {
            Some(c) if *c != u16::MAX => *c as u32,
            _ => (s.chars / chars_per_page.max(1)).max(1),
        })
        .sum::<u32>()
        .max(1)
}

/// 1-based page number in the book for (section, page within section).
pub fn page_number(book: &Book, counts: &[u16], chars_per_page: u32, section: u16, page: usize) -> u32 {
    let before: u32 = book
        .sections
        .iter()
        .enumerate()
        .take(section as usize)
        .map(|(i, s)| match counts.get(i) {
            Some(c) if *c != u16::MAX => *c as u32,
            _ => (s.chars / chars_per_page.max(1)).max(1),
        })
        .sum();
    before + page as u32 + 1
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

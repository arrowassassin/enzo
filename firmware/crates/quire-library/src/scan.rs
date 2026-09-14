//! Scanning source folders for books and keeping the index in step with the card.

use alloc::string::String;
use alloc::vec::Vec;
use quire_doc::Format;
use quire_fs::{Fs, ReadAt};

use crate::index::{BookStats, IngestState, Loc, Status};
use crate::{BookEntry, BookId, LibResult, Library};

/// Bound on files examined per scan.
#[cfg(target_os = "none")]
const MAX_FILES: usize = 1500;
/// Bound on files examined per scan.
#[cfg(not(target_os = "none"))]
const MAX_FILES: usize = 6000;
/// Folder depth.
const MAX_DEPTH: u32 = 6;

/// What a scan found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    /// Newly added books.
    pub added: u32,
    /// Books whose file went away.
    pub missing: u32,
    /// Books that came back.
    pub returned: u32,
    /// Files skipped because they were unreadable.
    pub errors: u32,
}

/// Walk the library's sources and reconcile the index. Books that vanished are marked
/// missing (their cache and position are kept for a while so a card swap is harmless).
pub fn scan<F: Fs>(fs: &F, lib: &mut Library, now: u32) -> LibResult<ScanReport> {
    let mut report = ScanReport::default();
    let mut found: Vec<(String, u64)> = Vec::new();
    let sources = lib.sources.clone();
    for src in &sources {
        let recurse = src != "/";
        walk(fs, src, recurse, 0, &mut found);
        if found.len() >= MAX_FILES {
            break;
        }
    }
    let mut seen: Vec<BookId> = Vec::with_capacity(found.len());
    for (path, size) in &found {
        // Same path and size as a known entry: nothing to hash.
        if let Some(e) = lib.by_path(path) {
            if e.size == *size {
                seen.push(e.id);
                continue;
            }
        }
        let Ok(file) = fs.open(path) else {
            report.errors += 1;
            continue;
        };
        let Ok(id) = BookId::of_file(&file) else {
            report.errors += 1;
            continue;
        };
        let mut head = [0u8; 64];
        let n = file.read_at(0, &mut head).unwrap_or(0);
        let Some(format) = Format::detect(path, &head[..n]) else { continue };
        seen.push(id);
        if let Some(e) = lib.get_mut(id) {
            // Known content at a new path (moved or renamed).
            e.path = path.clone();
            e.size = *size;
            if e.missing {
                e.missing = false;
                report.returned += 1;
            }
            continue;
        }
        lib.upsert(BookEntry {
            id,
            path: path.clone(),
            size: *size,
            format,
            title: quire_doc::title_from_name(path),
            authors: Vec::new(),
            series: None,
            year: None,
            language: String::new(),
            subjects: Vec::new(),
            sections: 0,
            chars: 0,
            has_cover: false,
            ingest: IngestState::Pending,
            error: None,
            added: now,
            last_opened: 0,
            loc: Loc::default(),
            status: Status::Unread,
            collections: Vec::new(),
            stats: BookStats::default(),
            missing: false,
        });
        report.added += 1;
    }
    let mut changed = false;
    for b in &mut lib.books {
        let present = seen.contains(&b.id);
        if !present && !b.missing && !b.path.is_empty() {
            b.missing = true;
            report.missing += 1;
            changed = true;
        } else if present && b.missing {
            b.missing = false;
            report.returned += 1;
            changed = true;
        }
    }
    if changed {
        lib.touch();
    }
    Ok(report)
}

fn walk<F: Fs>(fs: &F, dir: &str, recurse: bool, depth: u32, out: &mut Vec<(String, u64)>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = fs.read_dir(dir) else { return };
    for e in entries {
        if e.name.starts_with('.') || e.name.starts_with('_') || e.name.eq_ignore_ascii_case("System Volume Information") {
            continue;
        }
        let path = quire_fs::join(dir, &e.name);
        if e.is_dir {
            if recurse {
                walk(fs, &path, true, depth + 1, out);
            }
        } else if e.size > 0 && is_book_name(&e.name) && !out.iter().any(|(p, _)| *p == path) {
            out.push((path, e.size));
            if out.len() >= MAX_FILES {
                return;
            }
        }
    }
}

fn is_book_name(name: &str) -> bool {
    let ext = quire_fs::extension(name);
    matches!(
        ext.as_str(),
        "epub" | "txt" | "md" | "markdown" | "fb2" | "html" | "htm" | "xhtml" | "cbz" | "pdf" | "qbk" | "jpg" | "jpeg" | "png" | "bmp"
    )
}

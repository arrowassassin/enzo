//! Scanning source folders for books and keeping the index in step with the card.

use alloc::string::String;
use alloc::vec::Vec;
use quire_doc::Format;
use quire_fs::{Fs, ReadAt};

use crate::index::{BookStats, IngestState, Loc, Status};
use crate::{BookEntry, BookId, LibResult, Library};

/// Bound on files examined per scan.
#[cfg(target_os = "none")]
const MAX_FILES: usize = 300;
/// Bound on files examined per scan.
#[cfg(not(target_os = "none"))]
const MAX_FILES: usize = 6000;
/// Folder depth.
const MAX_DEPTH: u32 = 6;
/// Top-level folders that hold the apps' own files or system data, never books.
pub const SKIP_DIRS: &[&str] = &[
    "notes",
    "flashcards",
    "stories",
    "dict",
    "sleep",
    "images",
    "pictures",
    "photos",
    "fonts",
    "quire",
    "lost.dir",
    "android",
    "dcim",
    "system volume information",
    "$recycle.bin",
];

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

/// Walk the library's sources and reconcile the index as each file is seen (no list of
/// paths is built). Books that vanished are marked missing (their cache and position
/// are kept so a card swap is harmless).
pub fn scan<F: Fs>(fs: &F, lib: &mut Library, now: u32) -> LibResult<ScanReport> {
    let mut report = ScanReport::default();
    let sources = lib.sources.clone();
    let mut seen: Vec<BookId> = Vec::with_capacity(lib.books.len());
    let mut count = 0usize;
    let mut walked: Vec<String> = Vec::new();
    for src in &sources {
        // A source inside an already walked one would be visited twice.
        if walked.iter().any(|w| w != "/" && src.starts_with(w.as_str())) {
            continue;
        }
        let mut visit = |path: String, size: u64| -> bool {
            count += 1;
            reconcile(fs, lib, &path, size, now, &mut seen, &mut report);
            count < MAX_FILES
        };
        walk(fs, src, &walked, 0, &mut visit);
        walked.push(src.clone());
        if count >= MAX_FILES {
            break;
        }
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

fn reconcile<F: Fs>(fs: &F, lib: &mut Library, path: &str, size: u64, now: u32, seen: &mut Vec<BookId>, report: &mut ScanReport) {
    // Same path and size as a known entry: nothing to hash.
    if let Some(e) = lib.by_path(path) {
        if e.size == size {
            seen.push(e.id);
            return;
        }
    }
    let Ok(file) = fs.open(path) else {
        report.errors += 1;
        return;
    };
    let Ok(id) = BookId::of_file(&file) else {
        report.errors += 1;
        return;
    };
    let mut head = [0u8; 64];
    let n = file.read_at(0, &mut head).unwrap_or(0);
    let Some(format) = Format::detect(path, &head[..n]) else { return };
    seen.push(id);
    if let Some(e) = lib.get_mut(id) {
        // Known content at a new path (moved or renamed).
        e.path = String::from(path);
        e.size = size;
        if e.missing {
            e.missing = false;
            report.returned += 1;
        }
        lib.touch();
        return;
    }
    if lib.books.len() >= crate::index::MAX_BOOKS {
        return;
    }
    lib.upsert(BookEntry {
        id,
        path: String::from(path),
        size,
        format,
        title: quire_doc::title_from_name(path),
        authors: Vec::new(),
        series: None,
        year: None,
        language: String::new(),
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
        pages_total: None,
    });
    report.added += 1;
}

/// Walk a folder, skipping hidden folders and any folder already walked as a source.
fn walk<F: Fs>(fs: &F, dir: &str, skip: &[String], depth: u32, visit: &mut dyn FnMut(String, u64) -> bool) -> bool {
    if depth > MAX_DEPTH {
        return true;
    }
    let Ok(entries) = fs.read_dir(dir) else { return true };
    for e in entries {
        if e.name.starts_with('.') || e.name.starts_with('_') {
            continue;
        }
        if e.is_dir && dir == "/" && SKIP_DIRS.iter().any(|s| e.name.eq_ignore_ascii_case(s)) {
            continue;
        }
        let path = quire_fs::join(dir, &e.name);
        if e.is_dir {
            if skip.contains(&path) {
                continue;
            }
            if !walk(fs, &path, skip, depth + 1, visit) {
                return false;
            }
        } else if e.size > 0 && is_book_name(&e.name) && !visit(path, e.size) {
            return false;
        }
    }
    true
}

fn is_book_name(name: &str) -> bool {
    let ext = quire_fs::extension(name);
    matches!(
        ext.as_str(),
        "epub" | "txt" | "md" | "markdown" | "fb2" | "html" | "htm" | "xhtml" | "cbz" | "pdf" | "qbk" | "jpg" | "jpeg" | "png" | "bmp"
    )
}

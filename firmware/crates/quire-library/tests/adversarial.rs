//! Adversarial integration tests for `quire-library`: scanning pathological directory
//! trees, ingesting degenerate text files, position arithmetic at the extremes, and
//! loading corrupt stats files. Everything runs against a real `HostFs` temp card.
//!
//! The device has ~380 KB of RAM and no crash recovery beyond a watchdog reboot, so the
//! library layer must never panic, hang, or allocate without bound on hostile on-card
//! state. Confirmed bugs are pinned in `#[ignore]` tests named in review-tester.md.

use quire_fs::host::HostFs;
use quire_library::{ingest_book, scan, Book, Library, Stats};
use std::path::PathBuf;

/// A fresh, empty temp card rooted at a unique directory.
fn card(tag: &str) -> (HostFs, PathBuf) {
    let dir = std::env::temp_dir().join(format!("quire-adv-{tag}-{}-{}", std::process::id(), nanos()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    (HostFs::new(&dir), dir)
}

fn nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

// =======================================================================================
// Scanning pathological trees.
// =======================================================================================

#[test]
fn scan_deep_tree_and_many_files() {
    let (fs, dir) = card("scan");
    // A 10-level-deep chain of directories, each with a book, plus 2000 flat files, all
    // under a recursed source ("/" is scanned shallow by design, so use a named folder).
    let root = dir.join("lib");
    std::fs::create_dir_all(&root).unwrap();
    let mut deep = root.clone();
    for level in 0..10 {
        deep = deep.join(format!("lvl{level}"));
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join(format!("book{level}.txt")), "one line of text\n").unwrap();
    }
    let flat = root.join("flat");
    std::fs::create_dir_all(&flat).unwrap();
    for i in 0..2000 {
        std::fs::write(flat.join(format!("f{i}.txt")), "x\n").unwrap();
    }
    let mut lib = Library::load(&fs);
    lib.sources = vec!["/lib".into()];
    // Must return (bounded by MAX_DEPTH / MAX_FILES), never recurse or loop forever.
    let report = scan(&fs, &mut lib, 1_000).expect("scan");
    assert!(report.added > 0, "scan found some books: {report:?}");
    // A rescan is a no-op and also terminates.
    let again = scan(&fs, &mut lib, 2_000).expect("rescan");
    assert_eq!(again.added, 0, "rescan adds nothing: {again:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// =======================================================================================
// Degenerate text ingest, then position arithmetic at the extremes.
// =======================================================================================

/// Scan `dir`, ingest the first pending book, open it. Returns the open book + its fs.
fn ingest_one(fs: &HostFs) -> Book {
    let mut lib = Library::load(fs);
    lib.sources = vec!["/".into()];
    scan(fs, &mut lib, 1).expect("scan");
    let id = *lib.pending().first().expect("a pending book");
    ingest_book(fs, &mut lib, id, &mut |_, _| {}).expect("ingest");
    Book::open(fs, id).expect("open")
}

/// Exercise the position API at 0, total, and u32::MAX for an open book.
fn poke_positions(fs: &HostFs, book: &Book) {
    let total = book.total_chars();
    for chars in [0u32, total / 2, total, total.saturating_add(1), u32::MAX] {
        let loc = book.loc_at_chars(fs, chars).expect("loc_at_chars");
        assert!(loc.section < book.section_count().max(1), "section {} in range", loc.section);
        // Reading the located section must succeed and be bounded.
        let data = book.section(fs, loc.section).expect("section");
        assert!(data.len() <= 256 * 1024, "section bounded: {}", data.len());
    }
    // Out-of-range section reads Err, never panic.
    assert!(book.section(fs, book.section_count()).is_err());
    assert!(book.section(fs, u16::MAX).is_err());
}

#[test]
fn ingest_zero_byte_txt() {
    let (fs, dir) = card("zero");
    std::fs::write(dir.join("empty.txt"), b"").unwrap();
    // A 0-byte file: scan skips zero-size entries, so nothing is pending — must not panic.
    let mut lib = Library::load(&fs);
    lib.sources = vec!["/".into()];
    scan(&fs, &mut lib, 1).expect("scan");
    // If (despite the size filter) something were pending, ingest+open must still be safe.
    for id in lib.pending() {
        ingest_book(&fs, &mut lib, id, &mut |_, _| {}).expect("ingest");
        let book = Book::open(&fs, id).expect("open");
        poke_positions(&fs, &book);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ingest_single_300kb_line_no_spaces() {
    let (fs, dir) = card("longline");
    let line = "a".repeat(300 * 1024);
    std::fs::write(dir.join("wall.txt"), line.as_bytes()).unwrap();
    let book = ingest_one(&fs);
    assert!(book.section_count() >= 1);
    poke_positions(&fs, &book);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ingest_200k_newlines() {
    let (fs, dir) = card("newlines");
    let data = "\n".repeat(200_000);
    std::fs::write(dir.join("blank.txt"), data.as_bytes()).unwrap();
    let book = ingest_one(&fs);
    poke_positions(&fs, &book);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ingest_binary_named_txt() {
    // A file with a .txt extension full of random-looking bytes: decode + ingest must not
    // panic (it degrades to CP1252 text).
    let (fs, dir) = card("bintxt");
    let mut data = Vec::new();
    let mut x = 0x1234_5678u32;
    for _ in 0..64 * 1024 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        data.push(x as u8);
    }
    std::fs::write(dir.join("noise.txt"), &data).unwrap();
    let book = ingest_one(&fs);
    poke_positions(&fs, &book);
    let _ = std::fs::remove_dir_all(&dir);
}

// =======================================================================================
// Corrupt statistics files.
// =======================================================================================

fn write_stats_file(dir: &std::path::Path, name: &str, bytes: &[u8]) {
    let stats = dir.join(".quire").join("stats");
    std::fs::create_dir_all(&stats).unwrap();
    std::fs::write(stats.join(name), bytes).unwrap();
}

#[test]
fn stats_load_corrupt_daily_bin() {
    let (fs, dir) = card("daily");
    // Garbage that is not valid postcard: load must fall back to default, not panic.
    let mut x = 0xC0FFEEu32;
    let mut junk = Vec::new();
    for _ in 0..4096 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        junk.push(x as u8);
    }
    write_stats_file(&dir, "daily.bin", &junk);
    let s = Stats::load(&fs);
    // Default goals survive a corrupt index.
    assert_eq!(s.goal_minutes, 30);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stats_load_sessions_log_no_valid_records() {
    // A sessions.log whose length is not a multiple of 32 and whose bytes never set the
    // high validity bit (r[31] & 0x80): every record is rejected, so load folds nothing
    // and returns cleanly.
    let (fs, dir) = card("nolog");
    let log = vec![0u8; 32 * 10 + 17]; // not a multiple of 32
    write_stats_file(&dir, "sessions.log", &log);
    let s = Stats::load(&fs);
    assert_eq!(s.sessions, 0, "no valid records folded");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stats_load_many_valid_bounded_records() {
    // Many valid session records with realistic (bounded) time spans: folding must be
    // fast and panic-free. Guards the aggregation path (apply) with varied data.
    let (fs, dir) = card("goodlog");
    let mut log = Vec::new();
    let base = 1_600_000_000u32; // a normal wall-clock second
    let mut x = 0xABCDEF01u32;
    let rnd = |x: &mut u32| {
        *x ^= *x << 13;
        *x ^= *x >> 17;
        *x ^= *x << 5;
        *x
    };
    for i in 0..500u32 {
        let start = base + i * 3600 + (rnd(&mut x) % 3600);
        let dur = 30 + (rnd(&mut x) % 7200); // up to 2h
        let mut r = [0u8; 32];
        r[0..8].copy_from_slice(&(i as u64).to_le_bytes());
        r[8..12].copy_from_slice(&start.to_le_bytes());
        r[12..16].copy_from_slice(&(start + dur).to_le_bytes());
        r[16..20].copy_from_slice(&dur.to_le_bytes());
        r[20..22].copy_from_slice(&((rnd(&mut x) % 50) as u16).to_le_bytes());
        r[22..26].copy_from_slice(&(rnd(&mut x) % 5000).to_le_bytes());
        r[31] = 0x80; // valid marker, no flags
        log.extend_from_slice(&r);
    }
    write_stats_file(&dir, "sessions.log", &log);
    let s = Stats::load(&fs);
    assert!(s.sessions > 0 && s.secs > 0, "records folded: {} sessions", s.sessions);
    let _ = std::fs::remove_dir_all(&dir);
}

/// BUG (confirmed): a session record whose time span reaches the top of the u32 range
/// makes `Stats::apply` overflow when spreading active seconds across clock hours.
///
/// `apply` (src/stats.rs:406) does `let next = (t / 3600 + 1) * 3600;` inside
/// `while t < s.end && left > 0`. `from_record` accepts any 32-byte record with the high
/// bit set and does NOT validate `end >= start` or bound the span, so a hostile
/// `sessions.log` (or plain corruption) can hand `apply` a record with `end` near
/// `u32::MAX`. Once `t` climbs past ~4_294_963_696, `(t/3600+1)*3600` exceeds `u32::MAX`:
///   - host/debug build: panics ("attempt to multiply with overflow");
///   - device/release build: wraps `t` back to a small value, so `while t < s.end` never
///     terminates → an infinite loop that only the watchdog can end.
///
/// The same pattern is in `day_curve` (src/stats.rs:483). Reachable straight through
/// `Stats::load`. Fixed: records are validated and the loops are bounded.
#[test]
fn stats_load_hostile_session_span_overflows() {
    let (fs, dir) = card("hostile-span");
    let mut r = [0u8; 32];
    r[8..12].copy_from_slice(&4_294_962_000u32.to_le_bytes()); // start near the top
    r[12..16].copy_from_slice(&u32::MAX.to_le_bytes()); // end = u32::MAX
    r[16..20].copy_from_slice(&200u32.to_le_bytes()); // active > 0 keeps the loop alive
    r[31] = 0x80; // valid record marker
    write_stats_file(&dir, "sessions.log", &r);
    // On a fixed build this returns; today it panics (debug) or hangs (release device).
    let _ = Stats::load(&fs);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A fully random `sessions.log`. Guarded by a worker thread with a timeout because the
/// same `apply` overflow bug can make folding either panic or loop for a very long time
/// (huge spans → up to ~1.2M hour-steps per record). Kept `#[ignore]`.
#[test]
fn stats_load_fully_random_sessions_log() {
    let (_fs, dir) = card("randlog");
    let mut x = 0x9E37_79B9u32;
    let mut log = Vec::new();
    for _ in 0..(32 * 64 + 5) {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        log.push(x as u8);
    }
    write_stats_file(&dir, "sessions.log", &log);
    let dir2 = dir.clone();
    let handle = std::thread::spawn(move || {
        let fs = HostFs::new(&dir2);
        let _ = Stats::load(&fs);
    });
    // If the fold loops forever this never joins in time.
    let start = std::time::Instant::now();
    while !handle.is_finished() {
        if start.elapsed() > std::time::Duration::from_secs(5) {
            panic!("Stats::load did not finish in 5 s on a random log (suspected infinite loop)");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    handle.join().expect("Stats::load panicked on a random log");
    let _ = std::fs::remove_dir_all(&dir);
}

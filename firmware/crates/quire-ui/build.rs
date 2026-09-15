//! Turn the two word lists in `data/` into tables the UI can search without allocating
//! or scanning text at run time:
//!
//! * `COMMON_HASHES` / `COMMON_RANKS`: FNV-1a (32-bit) of every word in
//!   `common-words.txt`, sorted by hash, with the word's frequency rank beside it
//!   (0 = "the"). A rarity lookup is one hash and a binary search.
//! * `WORDLE_ANSWERS`: the five-letter words of the common list that the Wordle list
//!   accepts, most common first, as `[u8; 5]` — the daily answer is an index into it.
//! * `WORDLE_ACCEPTED`: every accepted five-letter guess, sorted, for a binary search.
//!
//! The 164 KB of newline text stays out of the binary; the tables are ~60 KB + ~80 KB.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

fn fnv1a(word: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in word.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let common_path = root.join("data/common-words.txt");
    let wordle_path = root.join("data/wordle-words.txt");
    println!("cargo:rerun-if-changed={}", common_path.display());
    println!("cargo:rerun-if-changed={}", wordle_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    let common_text = fs::read_to_string(&common_path).expect("data/common-words.txt");
    let wordle_text = fs::read_to_string(&wordle_path).expect("data/wordle-words.txt");

    // Common words: one rank per distinct lowercase word (the first occurrence wins).
    let mut common: Vec<(u32, u16)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut ordered: Vec<String> = Vec::new();
    for line in common_text.lines() {
        let w = line.trim().to_lowercase();
        if w.is_empty() || seen.contains(&w) {
            continue;
        }
        let rank = seen.len();
        assert!(rank < u16::MAX as usize, "more common words than a u16 rank can hold");
        seen.push(w.clone());
        ordered.push(w.clone());
        common.push((fnv1a(&w), rank as u16));
    }
    common.sort_unstable();
    for pair in common.windows(2) {
        assert_ne!(
            pair[0].0, pair[1].0,
            "FNV-1a collision between common words {} and {}",
            seen[pair[0].1 as usize], seen[pair[1].1 as usize]
        );
    }

    // Wordle: the accepted guesses (sorted) and the answers (common words that are accepted).
    let mut accepted: Vec<[u8; 5]> = wordle_text
        .lines()
        .map(|l| l.trim())
        .filter(|w| w.len() == 5 && w.bytes().all(|b| b.is_ascii_lowercase()))
        .map(|w| {
            let mut a = [0u8; 5];
            a.copy_from_slice(w.as_bytes());
            a
        })
        .collect();
    accepted.sort_unstable();
    accepted.dedup();
    let answers: Vec<[u8; 5]> = ordered
        .iter()
        .filter(|w| w.len() == 5 && w.bytes().all(|b| b.is_ascii_lowercase()))
        .map(|w| {
            let mut a = [0u8; 5];
            a.copy_from_slice(w.as_bytes());
            a
        })
        .filter(|a| accepted.binary_search(a).is_ok())
        .collect();
    assert!(!answers.is_empty(), "no Wordle answers: the common list has no accepted five-letter words");

    let mut out = String::new();
    let _ = writeln!(out, "/// Number of words in the common-word table.");
    let _ = writeln!(out, "pub const COMMON_LEN: usize = {};", common.len());
    let _ = writeln!(out, "/// FNV-1a hashes of the common words, sorted.");
    let _ = write!(out, "pub static COMMON_HASHES: [u32; {}] = [", common.len());
    for (i, (h, _)) in common.iter().enumerate() {
        if i % 8 == 0 {
            out.push('\n');
        }
        let _ = write!(out, "{h:#010x}, ");
    }
    let _ = writeln!(out, "\n];");
    let _ = writeln!(out, "/// Frequency rank (0 = most common) of the word at the same index in `COMMON_HASHES`.");
    let _ = write!(out, "pub static COMMON_RANKS: [u16; {}] = [", common.len());
    for (i, (_, r)) in common.iter().enumerate() {
        if i % 16 == 0 {
            out.push('\n');
        }
        let _ = write!(out, "{r}, ");
    }
    let _ = writeln!(out, "\n];");
    let _ = writeln!(out, "/// Wordle answers: accepted five-letter common words, most common first.");
    let _ = write!(out, "pub static WORDLE_ANSWERS: [[u8; 5]; {}] = [", answers.len());
    for (i, a) in answers.iter().enumerate() {
        if i % 8 == 0 {
            out.push('\n');
        }
        let _ = write!(out, "*b\"{}\", ", std::str::from_utf8(a).unwrap());
    }
    let _ = writeln!(out, "\n];");
    let _ = writeln!(out, "/// Every accepted Wordle guess, sorted for a binary search.");
    let _ = write!(out, "pub static WORDLE_ACCEPTED: [[u8; 5]; {}] = [", accepted.len());
    for (i, a) in accepted.iter().enumerate() {
        if i % 8 == 0 {
            out.push('\n');
        }
        let _ = write!(out, "*b\"{}\", ", std::str::from_utf8(a).unwrap());
    }
    let _ = writeln!(out, "\n];");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    fs::write(Path::new(&out_dir).join("words.rs"), out).expect("write words.rs");
}

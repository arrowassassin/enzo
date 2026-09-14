//! StarDict dictionaries on the card (`/dict/<name>.ifo`, `.idx`, `.dict`, uncompressed):
//! the idx is searched by binary search over the file itself, so a 5 MB index costs a few
//! sector reads per lookup and no RAM.

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::{Fs, ReadAt, WriteFile};

/// Where dictionaries live.
pub const DICT_DIR: &str = "/dict";

/// An open dictionary.
pub struct Dict<F: Fs> {
    /// Display name.
    pub name: String,
    idx: F::File,
    dict: F::File,
    /// Sampled record positions (`.qix`), every `STRIDE`th record, exact.
    qix: F::File,
    idx_len: u64,
    dict_len: u64,
    /// Whether offsets are 64-bit.
    wide: bool,
    samples: u64,
}

/// A definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The headword as stored.
    pub headword: String,
    /// The definition text (plain, with newlines).
    pub text: String,
}

/// Every `STRIDE`th index record is sampled into the `.qix` side file.
const STRIDE: u32 = 32;
const QIX_MAGIC: &[u8; 4] = b"QIX1";
const QIX_HEADER: u64 = 16;
/// Longest headword we accept (bytes); longer records are still skipped correctly.
const MAX_WORD: usize = 480;

/// Dictionary stems available on the card.
pub fn list<F: Fs>(fs: &F) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = fs.read_dir(DICT_DIR) {
        for e in entries {
            if let Some(stem) = e.name.strip_suffix(".ifo") {
                out.push(String::from(stem));
            }
        }
    }
    out.sort();
    out
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

impl<F: Fs> Dict<F> {
    /// Open a dictionary by stem. The first open of a dictionary builds its sampled
    /// index (one sequential pass over the `.idx`); later opens are instant.
    pub fn open(fs: &F, stem: &str) -> Option<Dict<F>> {
        let ifo = fs.read_to_vec(&alloc::format!("{DICT_DIR}/{stem}.ifo")).ok()?;
        let ifo = String::from_utf8_lossy(&ifo);
        let mut name = String::from(stem);
        let mut wide = false;
        for line in ifo.lines() {
            if let Some(v) = line.strip_prefix("bookname=") {
                name = String::from(v.trim());
            }
            if let Some(v) = line.strip_prefix("idxoffsetbits=") {
                wide = v.trim() == "64";
            }
        }
        let idx = fs.open(&alloc::format!("{DICT_DIR}/{stem}.idx")).ok()?;
        let dict = fs.open(&alloc::format!("{DICT_DIR}/{stem}.dict")).ok()?;
        let idx_len = idx.len();
        let dict_len = dict.len();
        let qix_path = alloc::format!("{DICT_DIR}/{stem}.qix");
        let mut qix = fs.open(&qix_path).ok();
        if !qix.as_ref().is_some_and(|q| Self::qix_valid(q, idx_len)) {
            Self::build_qix(fs, &idx, idx_len, wide, &qix_path)?;
            qix = fs.open(&qix_path).ok();
        }
        let qix = qix?;
        let samples = (qix.len().saturating_sub(QIX_HEADER)) / 8;
        Some(Dict { name, idx, dict, qix, idx_len, dict_len, wide, samples })
    }

    fn qix_valid(q: &F::File, idx_len: u64) -> bool {
        let mut h = [0u8; 16];
        if q.read_exact_at(0, &mut h).is_err() || &h[..4] != QIX_MAGIC {
            return false;
        }
        let len = u64::from_le_bytes(h[4..12].try_into().unwrap_or([0; 8]));
        let stride = u32::from_le_bytes(h[12..16].try_into().unwrap_or([0; 4]));
        len == idx_len && stride == STRIDE
    }

    /// One sequential pass over the idx, writing every `STRIDE`th record start.
    fn build_qix(fs: &F, idx: &F::File, idx_len: u64, wide: bool, path: &str) -> Option<()> {
        let tmp = alloc::format!("{path}.tmp");
        let mut w = fs.create(&tmp).ok()?;
        let mut header = Vec::with_capacity(16);
        header.extend_from_slice(QIX_MAGIC);
        header.extend_from_slice(&idx_len.to_le_bytes());
        header.extend_from_slice(&STRIDE.to_le_bytes());
        w.write_all(&header).ok()?;
        let nums = if wide { 12 } else { 8 };
        let mut buf = alloc::vec![0u8; 4096];
        let mut pending: Vec<u8> = Vec::with_capacity(2048);
        let mut pos = 0u64;
        let mut n_rec = 0u32;
        while pos < idx_len {
            let got = idx.read_at(pos, &mut buf).ok()?;
            if got == 0 {
                break;
            }
            let mut i = 0;
            // Parse whole records inside this chunk; the tail is re-read next round.
            while let Some(nul) = buf[i..got].iter().position(|b| *b == 0) {
                let end = i + nul + 1 + nums;
                if end > got {
                    break;
                }
                if n_rec.is_multiple_of(STRIDE) {
                    pending.extend_from_slice(&(pos + i as u64).to_le_bytes());
                    if pending.len() >= 2048 {
                        w.write_all(&pending).ok()?;
                        pending.clear();
                    }
                }
                n_rec = n_rec.wrapping_add(1);
                i = end;
            }
            if i == 0 {
                // No complete record in 4 KB: a corrupt or absurd record; stop here.
                break;
            }
            pos += i as u64;
        }
        if !pending.is_empty() {
            w.write_all(&pending).ok()?;
        }
        w.flush().ok()?;
        drop(w);
        let _ = fs.remove(path);
        fs.rename(&tmp, path).ok()?;
        Some(())
    }

    fn sample(&self, i: u64) -> Option<u64> {
        let mut b = [0u8; 8];
        self.qix.read_exact_at(QIX_HEADER + i * 8, &mut b).ok()?;
        Some(u64::from_le_bytes(b))
    }

    /// Read the entry starting at `pos`: (headword, data offset, size, next pos).
    fn entry_at(&self, pos: u64) -> Option<(String, u64, u32, u64)> {
        let mut buf = [0u8; 512];
        let n = self.idx.read_at(pos, &mut buf).ok()?;
        let nums = if self.wide { 12 } else { 8 };
        let Some(nul) = buf[..n].iter().position(|b| *b == 0) else {
            // A headword longer than the buffer: skip it without allocating.
            return self.skip_long(pos, nums);
        };
        let word = String::from_utf8_lossy(&buf[..nul.min(MAX_WORD)]).into_owned();
        let mut p = nul + 1;
        if p + nums > n {
            return None;
        }
        let off = if self.wide {
            let v = u64::from_be_bytes(buf[p..p + 8].try_into().ok()?);
            p += 8;
            v
        } else {
            let v = u32::from_be_bytes(buf[p..p + 4].try_into().ok()?) as u64;
            p += 4;
            v
        };
        let size = u32::from_be_bytes(buf[p..p + 4].try_into().ok()?);
        p += 4;
        Some((word, off, size, pos + p as u64))
    }

    fn skip_long(&self, pos: u64, nums: usize) -> Option<(String, u64, u32, u64)> {
        let mut buf = [0u8; 512];
        let mut at = pos;
        for _ in 0..64 {
            let n = self.idx.read_at(at, &mut buf).ok()?;
            if n == 0 {
                return None;
            }
            if let Some(nul) = buf[..n].iter().position(|b| *b == 0) {
                let next = at + nul as u64 + 1 + nums as u64;
                return Some((String::from("\u{fffd}"), 0, 0, next));
            }
            at += n as u64;
        }
        None
    }

    /// Look up a word (exact, then case-folded), returning its definition.
    pub fn lookup(&self, word: &str) -> Option<Entry> {
        let key = word.trim();
        if key.is_empty() || self.samples == 0 {
            return None;
        }
        let target = fold(key);
        // Binary search over the sampled record starts (exact boundaries).
        let (mut lo, mut hi) = (0u64, self.samples);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let pos = self.sample(mid)?;
            let (w, _, _, _) = self.entry_at(pos)?;
            if fold(&w).as_str() <= target.as_str() {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        // `lo` is the first sample greater than the target; the word, if present, lies
        // in the stride before it.
        let start = if lo == 0 { 0 } else { self.sample(lo - 1)? };
        let mut pos = start;
        for _ in 0..STRIDE {
            if pos >= self.idx_len {
                break;
            }
            let (w, _, _, next) = self.entry_at(pos)?;
            let f = fold(&w);
            if f == target {
                return self.read_entry(pos);
            }
            if f.as_str() > target.as_str() {
                break;
            }
            pos = next;
        }
        None
    }

    fn read_entry(&self, pos: u64) -> Option<Entry> {
        let (word, off, size, _) = self.entry_at(pos)?;
        if off >= self.dict_len {
            return None;
        }
        let size = (size as u64).min(16 * 1024).min(self.dict_len - off) as usize;
        let data = self.dict.read_range(off, size).ok()?;
        let text = String::from_utf8_lossy(&data).into_owned();
        Some(Entry { headword: word, text: strip_markup(&text) })
    }

    /// Look up with stemming fallbacks: exact, lowercase, then common suffixes.
    pub fn lookup_stemmed(&self, word: &str) -> Option<Entry> {
        let w = word.trim().trim_matches(|c: char| !c.is_alphanumeric());
        if let Some(e) = self.lookup(w) {
            return Some(e);
        }
        let lower = w.to_lowercase();
        for cand in stems(&lower) {
            if let Some(e) = self.lookup(&cand) {
                return Some(e);
            }
        }
        None
    }
}

/// Candidate base forms for an inflected word.
pub fn stems(w: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |s: String| {
        if s.len() >= 2 && !out.contains(&s) {
            out.push(s);
        }
    };
    for (suffix, repl) in [
        ("ies", "y"),
        ("es", ""),
        ("s", ""),
        ("ed", ""),
        ("ed", "e"),
        ("ing", ""),
        ("ing", "e"),
        ("er", ""),
        ("est", ""),
        ("ly", ""),
        ("ness", ""),
        ("ment", ""),
    ] {
        if let Some(stem) = w.strip_suffix(suffix) {
            push(alloc::format!("{stem}{repl}"));
        }
    }
    // Doubled consonant: "wadded" → "wad".
    if let Some(stem) = w.strip_suffix("ed").or_else(|| w.strip_suffix("ing")) {
        let b = stem.as_bytes();
        if b.len() >= 2 && b[b.len() - 1] == b[b.len() - 2] {
            push(String::from(&stem[..stem.len() - 1]));
        }
    }
    out
}

/// Remove light markup (StarDict "m"/"h" types): tags and entities.
pub fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if in_tag => {}
            _ => out.push(c),
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&").replace("&quot;", "\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_fs::host::HostFs;

    fn build(dir: &std::path::Path, words: &[(&str, &str)]) {
        let mut idx = Vec::new();
        let mut dict = Vec::new();
        let mut sorted: Vec<(&str, &str)> = words.to_vec();
        sorted.sort_by_key(|(w, _)| w.to_lowercase());
        for (w, d) in sorted {
            idx.extend_from_slice(w.as_bytes());
            idx.push(0);
            idx.extend_from_slice(&(dict.len() as u32).to_be_bytes());
            idx.extend_from_slice(&(d.len() as u32).to_be_bytes());
            dict.extend_from_slice(d.as_bytes());
        }
        std::fs::create_dir_all(dir.join("dict")).unwrap();
        std::fs::write(dir.join("dict/test.ifo"), "StarDict's dict ifo file\nversion=2.4.2\nbookname=Test Dictionary\nwordcount=3\n")
            .unwrap();
        std::fs::write(dir.join("dict/test.idx"), idx).unwrap();
        std::fs::write(dir.join("dict/test.dict"), dict).unwrap();
    }

    #[test]
    fn lookup_by_binary_search_and_stemming() {
        let dir = std::env::temp_dir().join(alloc::format!("quire-dict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut words: Vec<(String, String)> =
            (0..500).map(|i| (alloc::format!("word{i:04}"), alloc::format!("definition of word {i}"))).collect();
        words.push((String::from("wad"), String::from("<b>wad</b> a small mass &amp; lump")));
        words.push((String::from("Zebra"), String::from("striped horse")));
        let refs: Vec<(&str, &str)> = words.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        build(&dir, &refs);
        let fs = HostFs::new(&dir);
        assert_eq!(list(&fs), alloc::vec![String::from("test")]);
        let d = Dict::open(&fs, "test").unwrap();
        assert_eq!(d.name, "Test Dictionary");
        assert_eq!(d.lookup("word0250").unwrap().text, "definition of word 250");
        assert_eq!(d.lookup("word0000").unwrap().text, "definition of word 0");
        assert_eq!(d.lookup("word0499").unwrap().text, "definition of word 499");
        assert_eq!(d.lookup("zebra").unwrap().headword, "Zebra");
        assert!(d.lookup("nothing").is_none());
        let e = d.lookup_stemmed("wadded").unwrap();
        assert_eq!(e.text, "wad a small mass & lump");
        for i in (0..500).step_by(7) {
            assert_eq!(d.lookup(&alloc::format!("WORD{i:04}")).unwrap().text, alloc::format!("definition of word {i}"), "word {i}");
        }
        assert!(fs.exists("/dict/test.qix"));
        // Reopen: the sampled index is reused, and a stale one is rebuilt.
        let d2 = Dict::open(&fs, "test").unwrap();
        assert_eq!(d2.lookup("word0123").unwrap().text, "definition of word 123");
        std::fs::write(dir.join("dict/test.qix"), b"QIX1garbage").unwrap();
        let d3 = Dict::open(&fs, "test").unwrap();
        assert_eq!(d3.lookup("word0499").unwrap().text, "definition of word 499");
        assert!(d3.lookup("word0500").is_none());
        assert!(d3.lookup("a").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

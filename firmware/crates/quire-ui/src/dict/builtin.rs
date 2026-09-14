//! The built-in English dictionary: WordNet 3.1 compiled into the firmware as
//! `data/en.qdict` (feature `builtin-dict`, on by default). Lookups run straight off the
//! flash-resident blob: nothing is loaded into RAM except one 4 KB inflate buffer, the
//! deflate decoder state (~10 KB) and the entry being returned.
//!
//! # The `.qdict` format (version 1; all integers little-endian)
//!
//! Three sections, each a run of independently deflated blocks of at most [`BLOCK`]
//! uncompressed bytes, so a lookup inflates one block at a time into a reused buffer:
//!
//! * **word blocks** — the headwords in sorted (lowercase, bytewise) order with their
//!   senses. Headwords are front-coded in groups of 64; the group index (uncompressed) lets
//!   a lookup binary-search the group table, inflate the block that holds the group, then
//!   scan at most 64 entries.
//! * **gloss blocks** — one record per unique gloss, numbered in first-use order.
//!   Synonyms share a gloss (WordNet synsets), so an entry either *opens* a gloss (its id is
//!   the running counter) or *back-references* an earlier one by id.
//! * **vocab blocks** — the 16 384 most useful gloss words, numbered by frequency. Gloss
//!   text is tokenised against this vocabulary before deflate, which halves the blob.
//!
//! ```text
//! header (64 bytes)
//!   0  "QDCT"          4  u16 version = 1      6  u16 flags = 0
//!   8  u32 words       12 u32 glosses          16 u32 vocab        20 u32 groups
//!   24 u32 group_index (u32[groups+1]: absolute offsets of group records; the last is the end)
//!   28 u32 wtab        (u32[wblocks+1]: absolute offsets of word blocks; the last is the end)
//!   32 u32 wblocks
//!   36 u32 gtab        ((u32 first_id, u32 offset)[gblocks+1]; the last is (glosses, end))
//!   40 u32 gblocks
//!   44 u32 vtab        (same shape as gtab, for vocab blocks)
//!   48 u32 vblocks
//!   52 u32 block = 4096   56 u32 total length   60 reserved
//! group record: u16 block, u16 offset in block, u32 gloss id counter at group start,
//!               then the group's first headword (key bytes; length from the index)
//! word block:   entries; front-coding restarts (empty prefix) at every group start
//!   entry: u8 hdr = prefix_len << 4 | suffix_len, or 0xFF, u8 prefix_len, u8 suffix_len
//!          suffix bytes (key = prefix of the previous key + suffix)
//!          u8 info: bits 0-3 sense count, bit 4 = a caps mask follows
//!          [LEB128 caps mask: bit i set = byte i of the key is upper-case in the display form]
//!          per sense: u8 pos (0 noun, 1 verb, 2 adjective, 3 adverb); bit 7 set = a u24 gloss
//!          id follows (back-reference); clear = the gloss id is the running counter, which
//!          then increments
//! gloss block:  records u8 len + token bytes.   vocab block: records u8 len + ASCII word.
//! token bytes:  0x01-0x7E literal ASCII; 0x7F = "…";
//!               0xD0-0xFF = " " + vocab[b - 0xD0];
//!               0x80-0xBF, b1 = " " + vocab[(b0 & 0x3F) << 8 | b1];
//!               0xC0-0xCF, b1 = vocab[(b0 & 0x0F) << 8 | b1] (no leading space)
//! ```
//!
//! Every block is a raw deflate stream (no zlib header). `tools/quire-dict` writes the
//! format; its tests round-trip synthetic dictionaries through this reader.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use miniz_oxide::inflate::core::{decompress, inflate_flags, DecompressorOxide};
use miniz_oxide::inflate::TINFLStatus;

use super::{stems, Entry};

/// Display name of the built-in dictionary.
pub const NAME: &str = "WordNet 3.1";
/// Uncompressed size of the largest block, and of the one inflate buffer a lookup uses.
pub const BLOCK: usize = 4096;
/// Headwords per group in the index table.
pub const GROUP: usize = 64;

#[cfg(feature = "builtin-dict")]
static BLOB: &[u8] = include_bytes!("../../data/en.qdict");

/// The compiled-in dictionary, if the `builtin-dict` feature is on.
pub fn blob() -> Option<Blob<'static>> {
    #[cfg(feature = "builtin-dict")]
    {
        Blob::parse(BLOB)
    }
    #[cfg(not(feature = "builtin-dict"))]
    {
        None
    }
}

/// Look a word up in the built-in dictionary (exact, then lowercase).
pub fn lookup(word: &str) -> Option<Entry> {
    blob()?.lookup(word)
}

/// Look up with the same stemming fallbacks as card dictionaries.
pub fn lookup_stemmed(word: &str) -> Option<Entry> {
    blob()?.lookup_stemmed(word)
}

/// Part-of-speech labels, indexed by the stored pos byte.
const POS: [&str; 4] = ["n.", "v.", "adj.", "adv."];

/// A parsed `.qdict` blob (borrowed; parsing reads only the header).
#[derive(Clone, Copy)]
pub struct Blob<'a> {
    data: &'a [u8],
    words: u32,
    groups: u32,
    group_index: usize,
    wtab: usize,
    wblocks: u32,
    gtab: usize,
    gblocks: u32,
    vtab: usize,
    vblocks: u32,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// One block at a time: a 4 KB buffer plus the decoder state, both reused across the
/// several blocks a lookup touches (word, glosses, vocabulary).
struct Inflater {
    buf: Vec<u8>,
    state: Box<DecompressorOxide>,
    /// Which (section, block) the buffer currently holds, and its length.
    cur: Option<(u8, u32)>,
    len: usize,
}

impl Inflater {
    fn new() -> Self {
        Inflater { buf: vec![0; BLOCK], state: Box::default(), cur: None, len: 0 }
    }

    /// Inflate `src` into the buffer unless it is already there; returns the block.
    fn block(&mut self, tag: (u8, u32), src: &[u8]) -> Option<&[u8]> {
        if self.cur != Some(tag) {
            self.cur = None;
            self.state.init();
            let flags = inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
            let (status, _, out) = decompress(&mut self.state, src, &mut self.buf, 0, flags);
            if status != TINFLStatus::Done {
                return None;
            }
            self.len = out;
            self.cur = Some(tag);
        }
        Some(&self.buf[..self.len])
    }
}

/// Section tags for the inflater's block cache.
const SEC_WORD: u8 = 0;
const SEC_GLOSS: u8 = 1;
const SEC_VOCAB: u8 = 2;

impl<'a> Blob<'a> {
    /// Parse the header; `None` if this is not a version-1 qdict.
    pub fn parse(data: &'a [u8]) -> Option<Blob<'a>> {
        if data.get(..4)? != b"QDCT" || u16_at(data, 4)? != 1 {
            return None;
        }
        let total = u32_at(data, 56)? as usize;
        if total != data.len() || u32_at(data, 52)? as usize != BLOCK {
            return None;
        }
        Some(Blob {
            data,
            words: u32_at(data, 8)?,
            groups: u32_at(data, 20)?,
            group_index: u32_at(data, 24)? as usize,
            wtab: u32_at(data, 28)? as usize,
            wblocks: u32_at(data, 32)?,
            gtab: u32_at(data, 36)? as usize,
            gblocks: u32_at(data, 40)?,
            vtab: u32_at(data, 44)? as usize,
            vblocks: u32_at(data, 48)?,
        })
    }

    /// Number of headwords.
    pub fn words(&self) -> u32 {
        self.words
    }

    /// Group record `i`: (block, offset, gloss base, first key).
    fn group(&self, i: u32) -> Option<(u32, usize, u32, &'a [u8])> {
        let at = u32_at(self.data, self.group_index + 4 * i as usize)? as usize;
        let end = u32_at(self.data, self.group_index + 4 * (i as usize + 1))? as usize;
        let block = u16_at(self.data, at)? as u32;
        let off = u16_at(self.data, at + 2)? as usize;
        let base = u32_at(self.data, at + 4)?;
        Some((block, off, base, self.data.get(at + 8..end)?))
    }

    fn word_block(&self, i: u32) -> Option<&'a [u8]> {
        if i >= self.wblocks {
            return None;
        }
        let a = u32_at(self.data, self.wtab + 4 * i as usize)? as usize;
        let b = u32_at(self.data, self.wtab + 4 * (i as usize + 1))? as usize;
        self.data.get(a..b)
    }

    /// Id-table entry `i` of a gloss/vocab section: (first id, data offset).
    fn id_entry(tab: usize, data: &[u8], i: u32) -> Option<(u32, usize)> {
        Some((u32_at(data, tab + 8 * i as usize)?, u32_at(data, tab + 8 * i as usize + 4)? as usize))
    }

    /// Find the record with `id` in an id-indexed section; returns its bytes copied out.
    fn record(&self, inf: &mut Inflater, sec: u8, id: u32) -> Option<Vec<u8>> {
        let (tab, n) = if sec == SEC_GLOSS { (self.gtab, self.gblocks) } else { (self.vtab, self.vblocks) };
        // Last block whose first id <= id.
        let (mut lo, mut hi) = (0u32, n);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if Self::id_entry(tab, self.data, mid)?.0 <= id {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let bi = lo - 1;
        let (first, a) = Self::id_entry(tab, self.data, bi)?;
        let (_, b) = Self::id_entry(tab, self.data, bi + 1)?;
        let block = inf.block((sec, bi), self.data.get(a..b)?)?;
        let mut p = 0usize;
        for _ in first..id {
            p += 1 + *block.get(p)? as usize;
        }
        let len = *block.get(p)? as usize;
        Some(block.get(p + 1..p + 1 + len)?.to_vec())
    }

    /// Expand tokenised glosses to text. The vocabulary ids of every gloss are resolved
    /// together in sorted order, so each vocab block inflates at most once per lookup.
    fn detokenise(&self, inf: &mut Inflater, glosses: &[Vec<u8>]) -> Option<Vec<String>> {
        let mut ids: Vec<u32> = Vec::new();
        for tokens in glosses {
            let mut i = 0;
            while i < tokens.len() {
                let b = tokens[i];
                if b >= 0xD0 {
                    ids.push((b - 0xD0) as u32);
                } else if b >= 0x80 {
                    let hi = if b >= 0xC0 { b & 0x0F } else { b & 0x3F } as u32;
                    ids.push(hi << 8 | *tokens.get(i + 1)? as u32);
                    i += 1;
                }
                i += 1;
            }
        }
        ids.sort_unstable();
        ids.dedup();
        let mut words: Vec<(u32, String)> = Vec::with_capacity(ids.len());
        for id in ids {
            let w = self.record(inf, SEC_VOCAB, id)?;
            words.push((id, String::from_utf8_lossy(&w).into_owned()));
        }
        let word = |id: u32| words.binary_search_by_key(&id, |w| w.0).ok().map(|i| words[i].1.as_str());
        let mut out = Vec::with_capacity(glosses.len());
        for tokens in glosses {
            let mut text = String::with_capacity(tokens.len() * 3);
            let mut i = 0;
            while i < tokens.len() {
                let b = tokens[i];
                if b >= 0xD0 {
                    text.push(' ');
                    text.push_str(word((b - 0xD0) as u32)?);
                } else if b >= 0x80 {
                    let b1 = *tokens.get(i + 1)? as u32;
                    if b >= 0xC0 {
                        text.push_str(word(((b & 0x0F) as u32) << 8 | b1)?);
                    } else {
                        text.push(' ');
                        text.push_str(word(((b & 0x3F) as u32) << 8 | b1)?);
                    }
                    i += 1;
                } else if b == 0x7F {
                    text.push('…');
                } else {
                    text.push(b as char);
                }
                i += 1;
            }
            out.push(text);
        }
        Some(out)
    }

    /// Look a word up (exact bytes, then ASCII-lowercased).
    pub fn lookup(&self, word: &str) -> Option<Entry> {
        let key = word.trim();
        if key.is_empty() || key.len() > 255 || self.groups == 0 {
            return None;
        }
        let lower = key.to_ascii_lowercase();
        self.lookup_key(lower.as_bytes())
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

    fn lookup_key(&self, target: &[u8]) -> Option<Entry> {
        // Last group whose first key <= target.
        let (mut lo, mut hi) = (0u32, self.groups);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.group(mid)?.3 <= target {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let (block, mut p, mut gloss_id, _) = self.group(lo - 1)?;
        let mut inf = Inflater::new();
        let data = inf.block((SEC_WORD, block), self.word_block(block)?)?;
        let mut key: Vec<u8> = Vec::with_capacity(32);
        let mut senses: Vec<(u8, u32)> = Vec::new();
        for _ in 0..GROUP {
            let hdr = *data.get(p)?;
            p += 1;
            let (prefix, suffix) = if hdr == 0xFF {
                let v = (*data.get(p)? as usize, *data.get(p + 1)? as usize);
                p += 2;
                v
            } else {
                ((hdr >> 4) as usize, (hdr & 15) as usize)
            };
            key.truncate(prefix);
            key.extend_from_slice(data.get(p..p + suffix)?);
            p += suffix;
            let info = *data.get(p)?;
            p += 1;
            let mut mask = 0u64;
            if info & 0x10 != 0 {
                let mut shift = 0;
                loop {
                    let b = *data.get(p)?;
                    p += 1;
                    mask |= ((b & 0x7F) as u64) << shift;
                    shift += 7;
                    if b & 0x80 == 0 || shift >= 63 {
                        break;
                    }
                }
            }
            let hit = key.as_slice() == target;
            for _ in 0..(info & 15) {
                let pos = *data.get(p)?;
                p += 1;
                let id = if pos & 0x80 != 0 {
                    let id = u32::from_le_bytes([*data.get(p)?, *data.get(p + 1)?, *data.get(p + 2)?, 0]);
                    p += 3;
                    id
                } else {
                    gloss_id += 1;
                    gloss_id - 1
                };
                if hit {
                    senses.push((pos & 0x7F, id));
                }
            }
            if hit {
                let headword: String = key
                    .iter()
                    .enumerate()
                    .map(|(i, &b)| if i < 64 && mask >> i & 1 != 0 { b.to_ascii_uppercase() as char } else { b as char })
                    .collect();
                // Gloss ids are sorted for the fetch so each gloss block inflates once.
                let mut order: Vec<usize> = (0..senses.len()).collect();
                order.sort_unstable_by_key(|&i| senses[i].1);
                let mut tokens: Vec<Vec<u8>> = vec![Vec::new(); senses.len()];
                for i in order {
                    tokens[i] = self.record(&mut inf, SEC_GLOSS, senses[i].1)?;
                }
                let glosses = self.detokenise(&mut inf, &tokens)?;
                drop(inf);
                let mut text = String::new();
                for ((pos, _), gloss) in senses.iter().zip(&glosses) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(POS.get(*pos as usize).copied().unwrap_or("?"));
                    text.push(' ');
                    text.push_str(gloss);
                }
                return Some(Entry { headword, text });
            }
            if key.as_slice() > target {
                return None;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    use quire_dict::{Headword, Pos, Sense};
    use std::alloc::{GlobalAlloc, Layout, System};

    /// Counts the heap this thread holds while `measure` runs (other test threads are
    /// invisible to it), so a lookup's peak allocation can be asserted.
    struct Counting;

    std::thread_local! {
        static LIVE: Cell<(bool, usize, usize)> = const { Cell::new((false, 0, 0)) };
    }

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = LIVE.try_with(|c| {
                let (on, live, peak) = c.get();
                if on {
                    let live = live + layout.size();
                    c.set((on, live, peak.max(live)));
                }
            });
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            let _ = LIVE.try_with(|c| {
                let (on, live, peak) = c.get();
                if on {
                    c.set((on, live.saturating_sub(layout.size()), peak));
                }
            });
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static ALLOC: Counting = Counting;

    /// Run `f`, returning its result and the peak heap it held.
    fn measure<T>(f: impl FnOnce() -> T) -> (T, usize) {
        LIVE.with(|c| c.set((true, 0, 0)));
        let out = f();
        let (_, _, peak) = LIVE.with(|c| c.replace((false, 0, 0)));
        (out, peak)
    }

    fn sense(pos: Pos, gloss: &str) -> Sense {
        Sense { pos, gloss: String::from(gloss) }
    }

    #[test]
    fn round_trip_through_writer_and_reader() {
        // Enough distinct words and glosses to span several groups and blocks of every
        // section, plus the corner cases: capitals, shared glosses, ellipses, long keys,
        // duplicate headwords that merge, many senses.
        let mut words: Vec<Headword> = (0..1500)
            .map(|i| Headword {
                word: alloc::format!("word{i:04}"),
                senses: vec![sense(Pos::Noun, &alloc::format!("definition number {i} of the synthetic word list, kept unique"))],
            })
            .collect();
        words.push(Headword { word: String::from("Paris"), senses: vec![sense(Pos::Noun, "the capital of France")] });
        words.push(Headword {
            word: String::from("whale"),
            senses: vec![sense(Pos::Noun, "a large marine mammal"), sense(Pos::Verb, "hunt for whales")],
        });
        words.push(Headword { word: String::from("rorqual"), senses: vec![sense(Pos::Noun, "a large marine mammal")] });
        words.push(Headword {
            word: String::from("aardvark"),
            senses: vec![sense(Pos::Noun, "nocturnal burrowing mammal of the grasslands…")],
        });
        words.push(Headword { word: String::from("aardvark"), senses: vec![sense(Pos::Adj, "of or like an aardvark")] });
        words.push(Headword { word: String::from("z"), senses: vec![sense(Pos::Adv, "last")] });
        let long = "a".repeat(40) + "bcdefghijklmnopqrstuvwxyz";
        words.push(Headword { word: long.clone(), senses: vec![sense(Pos::Adj, "very long")] });
        words.push(Headword { word: long.clone() + "z", senses: vec![sense(Pos::Adj, "even longer")] });
        words.push(Headword {
            word: String::from("many"),
            senses: (0..15).map(|i| sense(Pos::Adj, &alloc::format!("sense {i}"))).collect(),
        });
        let (bytes, stats) = quire_dict::build(words);
        assert_eq!(stats.words, 1500 + 8);
        assert!(stats.blocks[0] >= 2 && stats.blocks[1] >= 2, "{stats:?}");
        let blob = Blob::parse(&bytes).expect("parses");
        assert_eq!(blob.words(), 1508);
        for i in (0..1500).step_by(13) {
            let e = blob.lookup(&alloc::format!("Word{i:04}")).unwrap_or_else(|| panic!("word{i:04}"));
            assert_eq!(e.headword, alloc::format!("word{i:04}"));
            assert_eq!(e.text, alloc::format!("n. definition number {i} of the synthetic word list, kept unique"));
        }
        assert_eq!(blob.lookup("paris").unwrap().headword, "Paris");
        assert_eq!(blob.lookup("PARIS").unwrap().text, "n. the capital of France");
        assert_eq!(blob.lookup("whale").unwrap().text, "n. a large marine mammal\nv. hunt for whales");
        assert_eq!(blob.lookup("rorqual").unwrap().text, "n. a large marine mammal");
        assert_eq!(blob.lookup("aardvark").unwrap().text, "n. nocturnal burrowing mammal of the grasslands…\nadj. of or like an aardvark");
        assert_eq!(blob.lookup("z").unwrap().text, "adv. last");
        assert_eq!(blob.lookup(&long).unwrap().text, "adj. very long");
        assert_eq!(blob.lookup(&(long.clone() + "z")).unwrap().text, "adj. even longer");
        assert_eq!(blob.lookup("many").unwrap().text.lines().count(), 15);
        assert_eq!(blob.lookup_stemmed("whales").unwrap().headword, "whale");
        assert_eq!(blob.lookup_stemmed("Whaling!").unwrap().headword, "whale");
        for miss in ["", " ", "a", "word1500", "word", "zz", "aardvarks!", "Paris "] {
            let e = blob.lookup(miss);
            assert!(e.is_none() || miss == "Paris ", "{miss:?} -> {e:?}");
        }
        assert_eq!(blob.lookup("Paris ").unwrap().headword, "Paris");
        // A truncated or foreign blob is rejected at parse, never read past its end.
        assert!(Blob::parse(&bytes[..bytes.len() - 1]).is_none());
        assert!(Blob::parse(b"QDCT").is_none());
        let mut bad = bytes.clone();
        bad[53] = 0;
        assert!(Blob::parse(&bad).is_none());
    }

    #[test]
    fn empty_dictionary_round_trips() {
        let (bytes, stats) = quire_dict::build(Vec::new());
        assert_eq!(stats.words, 0);
        let blob = Blob::parse(&bytes).expect("parses");
        assert!(blob.lookup("anything").is_none());
    }

    #[cfg(feature = "builtin-dict")]
    #[test]
    fn shipped_blob_resolves_words() {
        let blob = blob().expect("built-in dictionary");
        assert!(blob.words() > 80_000, "{}", blob.words());
        assert!(BLOB.len() <= 1_600_000, "en.qdict is {} bytes", BLOB.len());
        let e = lookup("shepherd").expect("shepherd");
        assert_eq!(e.headword, "shepherd");
        assert!(e.text.starts_with("n. ") && e.text.contains("\nv. watch over"), "{}", e.text);
        let e = lookup_stemmed("reveries").expect("reveries via stems");
        assert!(e.headword == "reverie" || e.headword == "revery", "{}", e.headword);
        assert!(e.text.starts_with("n. ") && e.text.contains("absorption"), "{}", e.text);
        let e = lookup("whale").expect("whale");
        assert!(e.text.contains("n. ") && e.text.contains("v. "), "{}", e.text);
        let e = lookup("pedestrian").expect("pedestrian");
        assert!(e.text.contains("n. ") && e.text.contains("adj. "), "{}", e.text);
        assert!(e.text.contains("by foot"), "{}", e.text);
        // The caps mask restores the display form of proper nouns.
        let e = lookup("ishmael").expect("Ishmael is in WordNet");
        assert_eq!(e.headword, "Ishmael");
        assert!(lookup("zzzz").is_none());
        assert!(lookup_stemmed("Queequeg").is_none());
        assert!(lookup("").is_none());
        // Every gloss line is a part of speech followed by text.
        for w in ["set", "run", "light", "a", "zygote", "aardvark", "yttrium"] {
            let e = lookup(w).unwrap_or_else(|| panic!("{w}"));
            for line in e.text.lines() {
                assert!(POS.iter().any(|p| line.starts_with(p)) && line.len() > 4, "{w}: {line:?}");
            }
        }
    }

    #[cfg(feature = "builtin-dict")]
    #[test]
    fn lookup_heap_is_bounded() {
        // The reader owns one BLOCK-sized inflate buffer and one decoder; the blob's
        // header must agree on the block size or parsing fails.
        assert_eq!(BLOCK, 4096);
        assert_eq!(GROUP, 64);
        assert_eq!(u32_at(BLOB, 52), Some(BLOCK as u32));
        let decoder = core::mem::size_of::<DecompressorOxide>();
        assert!(decoder < 12 * 1024, "decoder state is {decoder} bytes");
        for w in ["pedestrian", "whale", "set", "run", "reveries", "antidisestablishmentarianism"] {
            let (e, peak) = measure(|| lookup_stemmed(w));
            std::eprintln!("{w}: peak heap {peak} bytes");
            assert!(peak < 24 * 1024, "{w}: peak heap {peak} bytes");
            assert_eq!(e.is_some(), w != "antidisestablishmentarianism", "{w}");
        }
    }
}

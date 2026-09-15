//! Writer for the `.qdict` built-in dictionary format read by `quire_ui::dict::builtin`
//! (the format is documented at the top of that module). The writer is deterministic:
//! the same entries always produce the same bytes.

pub mod wordnet;

use std::collections::{BTreeMap, HashMap};

/// Largest uncompressed block; the reader inflates one block at a time into a buffer
/// of exactly this size.
pub const BLOCK: usize = 4096;
/// Headwords per group: the index table holds the first headword of every group.
pub const GROUP: usize = 64;
/// Vocabulary words that get a one-byte token (`0xD0..=0xFF`).
pub const SHORT_TOKENS: usize = 48;
/// Vocabulary words addressable by a two-byte token (`0x80..=0xBF` + byte).
pub const MAX_VOCAB: usize = 16384;
/// Vocabulary words addressable without a leading space (`0xC0..=0xCF` + byte).
pub const NOSPACE_VOCAB: usize = 4096;
/// Byte standing for "…" in a gloss (glosses are otherwise pure ASCII).
pub const ELLIPSIS: u8 = 0x7F;

/// Part of speech of a sense.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pos {
    /// Noun.
    Noun = 0,
    /// Verb.
    Verb = 1,
    /// Adjective (WordNet `a` and `s`).
    Adj = 2,
    /// Adverb.
    Adv = 3,
}

/// One sense of a headword.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sense {
    /// Part of speech.
    pub pos: Pos,
    /// The gloss: ASCII, at most 255 bytes once tokenised; `…` is allowed.
    pub gloss: String,
}

/// A headword with its senses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Headword {
    /// The display form (ASCII; may contain capitals).
    pub word: String,
    /// Senses in display order.
    pub senses: Vec<Sense>,
}

/// Lowercase key of a display form.
pub fn key_of(word: &str) -> String {
    word.to_ascii_lowercase()
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphabetic()
}

/// `(literal_before, word)` runs of a gloss, plus its trailing literal.
type Runs<'a> = (Vec<(&'a [u8], &'a [u8])>, &'a [u8]);

/// Split a gloss into alternating literal / word runs.
fn split_words(g: &[u8]) -> Runs<'_> {
    let mut out = Vec::new();
    let mut lit_start = 0;
    let mut i = 0;
    while i < g.len() {
        if is_word_byte(g[i]) {
            let start = i;
            while i < g.len() && is_word_byte(g[i]) {
                i += 1;
            }
            out.push((&g[lit_start..start], &g[start..i]));
            lit_start = i;
        } else {
            i += 1;
        }
    }
    (out, &g[lit_start..])
}

/// Choose and rank the vocabulary over the unique glosses.
fn vocabulary(glosses: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut count: HashMap<&[u8], u32> = HashMap::new();
    for g in glosses {
        let (words, _) = split_words(g);
        for (_, w) in words {
            *count.entry(w).or_insert(0) += 1;
        }
    }
    // Pick by bytes saved, then order ids by frequency so common words get small ids
    // (which deflate's Huffman coding then makes cheap).
    let mut by_saving: Vec<(&[u8], u32)> = count.into_iter().filter(|(w, c)| *c >= 2 || w.len() >= 4).collect();
    by_saving.sort_by(|a, b| (b.1 as usize * b.0.len()).cmp(&(a.1 as usize * a.0.len())).then(a.0.cmp(b.0)));
    by_saving.truncate(MAX_VOCAB);
    by_saving.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    by_saving.into_iter().map(|(w, _)| w.to_vec()).collect()
}

/// Tokenise one gloss against the vocabulary.
fn encode_gloss(g: &[u8], ids: &HashMap<&[u8], usize>) -> Vec<u8> {
    let mut out = Vec::with_capacity(g.len());
    let (words, tail) = split_words(g);
    for (lit, w) in words {
        let space = lit.last() == Some(&b' ');
        match ids.get(w) {
            Some(&id) if space => {
                out.extend_from_slice(&lit[..lit.len() - 1]);
                if id < SHORT_TOKENS {
                    out.push(0xD0 + id as u8);
                } else {
                    out.push(0x80 | (id >> 8) as u8);
                    out.push(id as u8);
                }
            }
            Some(&id) if id < NOSPACE_VOCAB => {
                out.extend_from_slice(lit);
                out.push(0xC0 | (id >> 8) as u8);
                out.push(id as u8);
            }
            _ => {
                out.extend_from_slice(lit);
                out.extend_from_slice(w);
            }
        }
    }
    out.extend_from_slice(tail);
    out
}

/// Pack length-prefixed records into blocks of at most `BLOCK` bytes; returns the blocks
/// and the first record index of each.
fn pack_records(records: &[Vec<u8>]) -> (Vec<Vec<u8>>, Vec<u32>) {
    let mut blocks = Vec::new();
    let mut firsts = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut cur_first = 0u32;
    for (i, r) in records.iter().enumerate() {
        assert!(r.len() <= 255, "record longer than 255 bytes");
        if cur.len() + 1 + r.len() > BLOCK {
            blocks.push(std::mem::take(&mut cur));
            firsts.push(cur_first);
            cur_first = i as u32;
        }
        cur.push(r.len() as u8);
        cur.extend_from_slice(r);
    }
    if !cur.is_empty() {
        blocks.push(cur);
        firsts.push(cur_first);
    }
    (blocks, firsts)
}

fn deflate(block: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec(block, 10)
}

fn put32(out: &mut Vec<u8>, v: usize) {
    out.extend_from_slice(&u32::try_from(v).expect("offset fits u32").to_le_bytes());
}

/// Statistics about a built dictionary.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Headwords written.
    pub words: usize,
    /// Unique glosses.
    pub glosses: usize,
    /// Vocabulary size.
    pub vocab: usize,
    /// Blocks per section: word, gloss, vocab.
    pub blocks: [usize; 3],
    /// Compressed bytes per section.
    pub section_bytes: [usize; 3],
    /// Total bytes.
    pub bytes: usize,
}

/// Build a `.qdict` blob from headwords (any order; duplicates of a key are merged).
pub fn build(headwords: Vec<Headword>) -> (Vec<u8>, Stats) {
    // Merge and sort by key.
    let mut by_key: BTreeMap<Vec<u8>, Headword> = BTreeMap::new();
    for h in headwords {
        assert!(h.word.is_ascii() && !h.word.is_empty(), "headword must be non-empty ASCII: {:?}", h.word);
        let key = key_of(&h.word).into_bytes();
        match by_key.get_mut(&key) {
            Some(existing) => {
                // Prefer the display form that equals the key; keep the first otherwise.
                if h.word.as_bytes() == key.as_slice() {
                    existing.word = h.word;
                }
                existing.senses.extend(h.senses);
            }
            None => {
                by_key.insert(key, h);
            }
        }
    }
    let words: Vec<(Vec<u8>, Headword)> = by_key.into_iter().collect();

    // Gloss ids in first-use order; senses become (pos, id, fresh).
    let mut gloss_ids: HashMap<String, u32> = HashMap::new();
    let mut glosses: Vec<Vec<u8>> = Vec::new();
    let mut senses: Vec<Vec<(Pos, u32, bool)>> = Vec::with_capacity(words.len());
    for (_, h) in &words {
        let mut list = Vec::with_capacity(h.senses.len());
        for s in &h.senses {
            let text = s.gloss.replace('…', "\u{7f}");
            assert!(text.is_ascii(), "gloss must be ASCII (plus …): {:?}", s.gloss);
            let (id, fresh) = match gloss_ids.get(&text) {
                Some(&id) => (id, false),
                None => {
                    let id = glosses.len() as u32;
                    gloss_ids.insert(text.clone(), id);
                    glosses.push(text.into_bytes());
                    (id, true)
                }
            };
            list.push((s.pos, id, fresh));
        }
        assert!(list.len() < 16, "at most 15 senses per headword");
        senses.push(list);
    }

    assert!(glosses.len() < 1 << 24, "gloss ids are 24-bit");
    // Vocabulary and tokenised glosses.
    let vocab = vocabulary(&glosses);
    let ids: HashMap<&[u8], usize> = vocab.iter().enumerate().map(|(i, w)| (w.as_slice(), i)).collect();
    let tokenised: Vec<Vec<u8>> = glosses.iter().map(|g| encode_gloss(g, &ids)).collect();

    // Word groups → blocks. Each group's front-coding starts from an empty prefix.
    let mut groups: Vec<(u16, u16, u32, Vec<u8>)> = Vec::new(); // (block, offset, gloss_base, key)
    let mut wblocks: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut next_gloss = 0u32;
    for (gi, chunk) in words.chunks(GROUP).enumerate() {
        let mut bytes = Vec::new();
        let mut prev: &[u8] = &[];
        let gloss_base = next_gloss;
        for (wi, (key, h)) in chunk.iter().enumerate() {
            let common = prev.iter().zip(key.iter()).take_while(|(a, b)| a == b).count().min(255);
            let suffix = &key[common..];
            assert!(suffix.len() <= 255, "headword too long: {:?}", h.word);
            if common < 15 && suffix.len() < 15 {
                bytes.push(((common as u8) << 4) | suffix.len() as u8);
            } else {
                bytes.push(0xFF);
                bytes.push(common as u8);
                bytes.push(suffix.len() as u8);
            }
            bytes.extend_from_slice(suffix);
            let list = &senses[gi * GROUP + wi];
            let mut mask = 0u64;
            for (i, (k, d)) in key.iter().zip(h.word.bytes()).enumerate() {
                if *k != d {
                    mask |= 1 << i;
                }
            }
            bytes.push(list.len() as u8 | if mask != 0 { 0x10 } else { 0 });
            while mask != 0 {
                let b = (mask & 0x7F) as u8;
                mask >>= 7;
                bytes.push(if mask != 0 { b | 0x80 } else { b });
            }
            for &(pos, id, fresh) in list {
                if fresh {
                    debug_assert_eq!(id, next_gloss);
                    bytes.push(pos as u8);
                    next_gloss += 1;
                } else {
                    bytes.push(pos as u8 | 0x80);
                    bytes.extend_from_slice(&id.to_le_bytes()[..3]);
                }
            }
            prev = key;
        }
        assert!(bytes.len() <= BLOCK, "group larger than a block");
        if cur.len() + bytes.len() > BLOCK {
            wblocks.push(std::mem::take(&mut cur));
        }
        groups.push((wblocks.len() as u16, cur.len() as u16, gloss_base, chunk[0].0.clone()));
        cur.extend_from_slice(&bytes);
    }
    if !cur.is_empty() {
        wblocks.push(cur);
    }
    let (gblocks, gfirsts) = pack_records(&tokenised);
    let (vblocks, vfirsts) = pack_records(&vocab);

    let wz: Vec<Vec<u8>> = wblocks.iter().map(|b| deflate(b)).collect();
    let gz: Vec<Vec<u8>> = gblocks.iter().map(|b| deflate(b)).collect();
    let vz: Vec<Vec<u8>> = vblocks.iter().map(|b| deflate(b)).collect();

    // Layout: header, group index, group pool, word table, gloss table, vocab table, data.
    const HEADER: usize = 64;
    let group_index_off = HEADER;
    let group_pool_off = group_index_off + 4 * (groups.len() + 1);
    let group_pool_len: usize = groups.iter().map(|g| 8 + g.3.len()).sum();
    let wtab_off = group_pool_off + group_pool_len;
    let gtab_off = wtab_off + 4 * (wz.len() + 1);
    let vtab_off = gtab_off + 8 * (gz.len() + 1);
    let data_off = vtab_off + 8 * (vz.len() + 1);
    let wdata_len: usize = wz.iter().map(Vec::len).sum();
    let gdata_len: usize = gz.iter().map(Vec::len).sum();
    let vdata_len: usize = vz.iter().map(Vec::len).sum();
    let total = data_off + wdata_len + gdata_len + vdata_len;

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"QDCT");
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    put32(&mut out, words.len());
    put32(&mut out, glosses.len());
    put32(&mut out, vocab.len());
    put32(&mut out, groups.len());
    put32(&mut out, group_index_off);
    put32(&mut out, wtab_off);
    put32(&mut out, wz.len());
    put32(&mut out, gtab_off);
    put32(&mut out, gz.len());
    put32(&mut out, vtab_off);
    put32(&mut out, vz.len());
    put32(&mut out, BLOCK);
    put32(&mut out, total);
    put32(&mut out, 0);
    assert_eq!(out.len(), HEADER);
    // Group index: offsets of each group record, plus the end.
    let mut o = group_pool_off;
    for g in &groups {
        put32(&mut out, o);
        o += 8 + g.3.len();
    }
    put32(&mut out, o);
    for (block, off, base, key) in &groups {
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
        out.extend_from_slice(&base.to_le_bytes());
        out.extend_from_slice(key);
    }
    assert_eq!(out.len(), wtab_off);
    let mut o = data_off;
    for z in &wz {
        put32(&mut out, o);
        o += z.len();
    }
    put32(&mut out, o);
    for (z, first) in gz.iter().zip(&gfirsts) {
        put32(&mut out, *first as usize);
        put32(&mut out, o);
        o += z.len();
    }
    put32(&mut out, glosses.len());
    put32(&mut out, o);
    for (z, first) in vz.iter().zip(&vfirsts) {
        put32(&mut out, *first as usize);
        put32(&mut out, o);
        o += z.len();
    }
    put32(&mut out, vocab.len());
    put32(&mut out, o);
    assert_eq!(out.len(), data_off);
    for z in wz.iter().chain(&gz).chain(&vz) {
        out.extend_from_slice(z);
    }
    assert_eq!(out.len(), total);
    let stats = Stats {
        words: words.len(),
        glosses: glosses.len(),
        vocab: vocab.len(),
        blocks: [wz.len(), gz.len(), vz.len()],
        section_bytes: [wdata_len, gdata_len, vdata_len],
        bytes: total,
    };
    (out, stats)
}

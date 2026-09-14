//! Reads the WordNet 3.1 database files (`index.{noun,verb,adj,adv}` and `data.*`) into
//! [`Headword`]s: single-word lemmas only (no phrases with spaces), one gloss per sense,
//! example sentences dropped.

use crate::{Headword, Pos, Sense};
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// What to keep of each lemma.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Senses kept per part of speech (WordNet lists the most frequent first).
    pub senses_per_pos: usize,
    /// Senses kept per headword across all parts of speech.
    pub max_senses: usize,
    /// Longest gloss in characters; longer ones are cut at a word boundary with `…`.
    pub gloss_chars: usize,
    /// `;`-separated gloss segments kept (examples in quotes are always dropped).
    pub segments: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { senses_per_pos: 1, max_senses: 4, gloss_chars: 120, segments: 1 }
    }
}

const FILES: [(Pos, &str); 4] = [(Pos::Noun, "noun"), (Pos::Verb, "verb"), (Pos::Adj, "adj"), (Pos::Adv, "adv")];

/// One synset from a data file: its word forms (as written) and gloss.
struct Synset {
    words: Vec<String>,
    gloss: String,
}

fn read(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    // WordNet is ASCII; decode as Latin-1 so a stray high byte can't fail the parse.
    Ok(bytes.iter().map(|&b| b as char).collect())
}

fn parse_data(text: &str) -> Result<HashMap<u32, Synset>> {
    let mut out = HashMap::new();
    for line in text.lines() {
        if line.starts_with(' ') || line.is_empty() {
            continue;
        }
        let (head, gloss) = line.split_once('|').context("data line without gloss")?;
        let mut f = head.split_whitespace();
        let off: u32 = f.next().context("offset")?.parse()?;
        let _lex = f.next();
        let _ss = f.next();
        let n = usize::from_str_radix(f.next().context("w_cnt")?, 16)?;
        let mut words = Vec::with_capacity(n);
        for _ in 0..n {
            let w = f.next().context("word")?;
            let _lex_id = f.next();
            // Adjective markers: "galore(ip)".
            let w = w.split_once('(').map(|(a, _)| a).unwrap_or(w);
            words.push(w.to_string());
        }
        out.insert(off, Synset { words, gloss: gloss.trim().to_string() });
    }
    Ok(out)
}

/// Trim a gloss to its definition: drop quoted examples, keep the first segments, cap.
pub fn clean_gloss(gloss: &str, opt: &Options) -> String {
    let mut segs: Vec<&str> = gloss.split(';').map(str::trim).filter(|s| !s.is_empty() && !s.starts_with('"')).collect();
    segs.truncate(opt.segments.max(1));
    let mut g: String = segs.join("; ").chars().map(|c| if c.is_ascii() { c } else { '?' }).collect();
    if g.chars().count() > opt.gloss_chars {
        let cut = g[..opt.gloss_chars - 1].rfind(' ').unwrap_or(opt.gloss_chars - 1);
        g.truncate(cut);
        let trimmed = g.trim_end_matches([',', ';', ' ']).len();
        g.truncate(trimmed);
        g.push('…');
    }
    g
}

/// Load every single-word lemma of a WordNet directory.
pub fn load(dir: &Path, opt: &Options) -> Result<Vec<Headword>> {
    // lemma → (display form, senses)
    let mut lemmas: BTreeMap<String, (Option<String>, Vec<Sense>)> = BTreeMap::new();
    for (pos, name) in FILES {
        let data = parse_data(&read(&dir.join(format!("data.{name}")))?)?;
        let index = read(&dir.join(format!("index.{name}")))?;
        for line in index.lines() {
            if line.starts_with(' ') || line.is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 6 {
                bail!("short index line: {line}");
            }
            let lemma = f[0];
            if lemma.contains('_') || !lemma.is_ascii() {
                continue;
            }
            let n_syn: usize = f[2].parse()?;
            let n_ptr: usize = f[3].parse()?;
            let offsets = &f[4 + n_ptr + 2..];
            if offsets.len() != n_syn {
                bail!("synset count mismatch: {line}");
            }
            let entry = lemmas.entry(lemma.to_string()).or_default();
            for off in offsets.iter().take(opt.senses_per_pos) {
                let off: u32 = off.parse()?;
                let syn = data.get(&off).with_context(|| format!("missing synset {off} for {lemma}"))?;
                if let Some(form) = syn.words.iter().find(|w| w.eq_ignore_ascii_case(lemma)) {
                    let better = match &entry.0 {
                        None => true,
                        Some(cur) => cur != lemma && form == lemma,
                    };
                    if better {
                        entry.0 = Some(form.clone());
                    }
                }
                entry.1.push(Sense { pos, gloss: clean_gloss(&syn.gloss, opt) });
            }
        }
    }
    Ok(lemmas
        .into_iter()
        .map(|(lemma, (form, mut senses))| {
            senses.truncate(opt.max_senses.max(1));
            Headword { word: form.unwrap_or(lemma), senses }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX_NOUN: &str = "  1 licence line\n\
        whale n 2 3 @ ~ %p 2 1 00000001 00000002\n\
        blue_whale n 1 1 @ 1 0 00000003\n\
        paris n 1 1 @ 1 0 00000004\n";
    const DATA_NOUN: &str = "  1 licence line\n\
        00000001 05 n 02 whale 0 giant 0 000 | any of the larger cetacean mammals; \"the whale breached\"; having a streamlined body\n\
        00000002 05 n 01 whale 1 000 | a very large person\n\
        00000003 05 n 01 blue_whale 0 000 | largest mammal ever known\n\
        00000004 15 n 02 Paris 0 capital_of_France 0 000 | the capital and largest city of France\n";
    const INDEX_VERB: &str = "whale v 1 1 @ 1 0 00000010\n";
    const DATA_VERB: &str = "00000010 35 v 01 whale 0 000 01 + 02 00 | hunt for whales\n";

    fn tmp() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("quire-dict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in [
            ("index.noun", INDEX_NOUN),
            ("data.noun", DATA_NOUN),
            ("index.verb", INDEX_VERB),
            ("data.verb", DATA_VERB),
            ("index.adj", ""),
            ("data.adj", ""),
            ("index.adv", ""),
            ("data.adv", ""),
        ] {
            std::fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    #[test]
    fn loads_single_word_lemmas_with_trimmed_glosses() {
        let dir = tmp();
        let words = load(&dir, &Options::default()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let names: Vec<&str> = words.iter().map(|w| w.word.as_str()).collect();
        assert_eq!(names, ["Paris", "whale"], "phrases are dropped, display forms keep capitals");
        let whale = &words[1];
        assert_eq!(whale.senses.len(), 2, "one sense per part of speech");
        assert_eq!(whale.senses[0], Sense { pos: Pos::Noun, gloss: String::from("any of the larger cetacean mammals") });
        assert_eq!(whale.senses[1], Sense { pos: Pos::Verb, gloss: String::from("hunt for whales") });
        let two = load(&dir, &Options { senses_per_pos: 2, ..Options::default() });
        assert!(two.is_err(), "the directory is gone");
    }

    #[test]
    fn clean_gloss_cuts_at_a_word_and_drops_examples() {
        let opt = Options::default();
        assert_eq!(clean_gloss("a; \"quoted example\"; b", &opt), "a");
        assert_eq!(clean_gloss("a; b; c", &Options { segments: 2, ..opt }), "a; b");
        let long = "word ".repeat(40);
        let cut = clean_gloss(&long, &Options { gloss_chars: 22, ..opt });
        assert_eq!(cut, "word word word word…");
        assert_eq!(clean_gloss("caf\u{e9} au lait", &opt), "caf? au lait");
    }
}

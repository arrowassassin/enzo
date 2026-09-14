//! `quire-dict build --wordnet DIR --out crates/quire-ui/data/en.qdict` builds the
//! built-in dictionary; `quire-dict info FILE` describes a built one. See README.md.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use quire_dict::wordnet::{self, Options};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "quire-dict", about = "Build Quire's built-in dictionary from WordNet")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build a .qdict from a WordNet 3.1 database directory.
    Build {
        /// Directory holding index.noun, data.noun, ... (the `dict` folder of a WordNet release).
        #[arg(long)]
        wordnet: PathBuf,
        /// Output file.
        #[arg(long)]
        out: PathBuf,
        /// Senses kept per part of speech.
        #[arg(long, default_value_t = Options::default().senses_per_pos)]
        senses_per_pos: usize,
        /// Senses kept per headword in total.
        #[arg(long, default_value_t = Options::default().max_senses)]
        max_senses: usize,
        /// Longest gloss, in characters.
        #[arg(long, default_value_t = Options::default().gloss_chars)]
        gloss_chars: usize,
        /// Gloss segments (split on ';') kept.
        #[arg(long, default_value_t = Options::default().segments)]
        segments: usize,
    },
    /// Describe a .qdict: counts and section layout from its header.
    Info {
        /// The .qdict file.
        file: PathBuf,
    },
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(d.get(o..o + 4).context("truncated header")?.try_into()?))
}

fn info(file: &PathBuf) -> Result<()> {
    let d = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    if d.get(..4) != Some(b"QDCT") {
        bail!("{} is not a .qdict (bad magic)", file.display());
    }
    let version = u16::from_le_bytes([d[4], d[5]]);
    let (words, glosses, vocab, groups) = (u32_at(&d, 8)?, u32_at(&d, 12)?, u32_at(&d, 16)?, u32_at(&d, 20)?);
    let (wtab, wblocks, gtab, gblocks) = (u32_at(&d, 28)?, u32_at(&d, 32)?, u32_at(&d, 36)?, u32_at(&d, 40)?);
    let (vtab, vblocks, block, total) = (u32_at(&d, 44)?, u32_at(&d, 48)?, u32_at(&d, 52)?, u32_at(&d, 56)?);
    if total as usize != d.len() {
        bail!("header says {total} bytes but the file is {}", d.len());
    }
    let data = vtab + 8 * (vblocks + 1);
    let gdata = u32_at(&d, gtab as usize + 4)?;
    let vdata = u32_at(&d, vtab as usize + 4)?;
    println!("{}: qdict v{version}, {total} bytes, block {block}", file.display());
    println!("{words} headwords in {groups} groups of {}; {glosses} glosses; {vocab} vocabulary words", quire_dict::GROUP);
    println!("index: {} B (group index + word/gloss/vocab tables)", data - 64);
    println!("word blocks:  {wblocks:>4} × ≤{block} B, {} B deflated (table at {wtab})", gdata - data);
    println!("gloss blocks: {gblocks:>4} × ≤{block} B, {} B deflated (table at {gtab})", vdata - gdata);
    println!("vocab blocks: {vblocks:>4} × ≤{block} B, {} B deflated (table at {vtab})", total - vdata);
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.cmd {
        Cmd::Build { wordnet, out, senses_per_pos, max_senses, gloss_chars, segments } => {
            let opt = Options { senses_per_pos, max_senses, gloss_chars, segments };
            let words = wordnet::load(&wordnet, &opt)?;
            let (bytes, stats) = quire_dict::build(words);
            std::fs::write(&out, &bytes)?;
            println!(
                "{}: {} bytes, {} headwords, {} glosses, {} vocabulary words",
                out.display(),
                stats.bytes,
                stats.words,
                stats.glosses,
                stats.vocab
            );
            println!(
                "blocks: {} word ({} B), {} gloss ({} B), {} vocab ({} B)",
                stats.blocks[0], stats.section_bytes[0], stats.blocks[1], stats.section_bytes[1], stats.blocks[2], stats.section_bytes[2]
            );
        }
        Cmd::Info { file } => info(&file)?,
    }
    Ok(())
}

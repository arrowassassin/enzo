//! `quire-sleep`: regenerate the sleep-screen art packs under `sleep-packs/`.

use anyhow::{bail, Result};
use clap::Parser;
use quire_sleep::output::{bitmap_to_png, render_pack, write_index, write_pack};
use quire_sleep::packs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "quire-sleep", about = "Generate 1-bit sleep-screen packs with a clean clock slot")]
struct Args {
    /// Output directory (default: the repository's sleep-packs/).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Only regenerate this pack id.
    #[arg(long)]
    pack: Option<String>,
    /// Also write full-size PNGs with a sample time drawn in the slot, into this directory.
    #[arg(long)]
    png: Option<PathBuf>,
    /// The sample time drawn in previews.
    #[arg(long, default_value = "21:47")]
    time: String,
    /// List the packs and exit.
    #[arg(long)]
    list: bool,
    /// With --png: only write the PNGs, leave the packs alone.
    #[arg(long)]
    dry: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let all = packs::all();
    if args.list {
        for p in &all {
            println!("{:<15} {:<15} {} images  {}", p.id, p.name, p.count, p.description);
        }
        return Ok(());
    }
    let out = args.out.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../sleep-packs"));
    let selected: Vec<packs::Pack> = match &args.pack {
        Some(id) => match packs::find(id) {
            Some(p) => vec![p],
            None => bail!("unknown pack {id}; try --list"),
        },
        None => all.clone(),
    };

    if let Some(png_dir) = &args.png {
        std::fs::create_dir_all(png_dir)?;
        for p in &selected {
            for (i, (art, _clean, preview)) in render_pack(p, &args.time).into_iter().enumerate() {
                let path = png_dir.join(format!("{}-{:02}.png", p.id, i + 1));
                bitmap_to_png(&preview, &path)?;
                println!("{}  {}  ({})", path.display(), art.title, art.credit);
            }
        }
        if args.dry {
            return Ok(());
        }
    }

    std::fs::create_dir_all(&out)?;
    let mut entries = Vec::new();
    for p in &all {
        let regenerate = selected.iter().any(|s| s.id == p.id);
        let entry = if regenerate {
            let e = write_pack(&out, p, &args.time)?;
            println!(
                "{:<15} {} images  {:>7} B pbm  {:>7} B z  ink {}",
                p.id,
                e.images,
                e.bytes,
                e.bytes_z,
                ink_summary(&out.join(p.id).join("pack.json"))?
            );
            e
        } else {
            // Keep the existing entry so a single-pack run leaves the index complete.
            let json: quire_sleep::output::PackJson = serde_json::from_slice(&std::fs::read(out.join(p.id).join("pack.json"))?)?;
            quire_sleep::output::IndexPack {
                id: json.id,
                name: json.name,
                description: json.description,
                images: json.images.len(),
                bytes: json.images.iter().map(|i| i.bytes).sum(),
                bytes_z: json.images.iter().map(|i| i.bytes_z).sum(),
                path: format!("{}/pack.json", p.id),
            }
        };
        entries.push(entry);
    }
    let index = write_index(&out, entries)?;
    let total: u64 = index.packs.iter().map(|p| p.bytes + p.bytes_z).sum();
    println!("{} packs, {} images, {} KB in pbm + z", index.packs.len(), index.packs.iter().map(|p| p.images).sum::<usize>(), total / 1024);
    Ok(())
}

fn ink_summary(pack_json: &std::path::Path) -> Result<String> {
    let json: quire_sleep::output::PackJson = serde_json::from_slice(&std::fs::read(pack_json)?)?;
    Ok(json.images.iter().map(|i| format!("{:.0}%", i.ink * 100.0)).collect::<Vec<_>>().join("/"))
}

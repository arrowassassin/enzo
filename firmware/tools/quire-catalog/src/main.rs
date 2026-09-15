//! Builds the Bookshop catalog the device downloads: `bookshop/catalog.json` (a curated,
//! hand-maintained list of public-domain books) laid out with the UI's own encoder into
//! `bookshop/catalog.bin`, byte-for-byte the `/.quire/bookshop/catalog.qcat` file the
//! screens read, so the device streams it straight to the card.
//!
//! `cargo run -p quire-catalog` from `firmware/` rebuilds it; `--check` only validates.

use std::path::PathBuf;

use anyhow::{bail, Context};
use clap::Parser;
use quire_ui::screens::bookshop::{self, Catalog, CatalogFile};

#[derive(Parser)]
#[command(about = "Build the Bookshop catalog file from its JSON source")]
struct Args {
    /// The curated list.
    #[arg(long, default_value_os_t = repo_root().join("bookshop/catalog.json"))]
    input: PathBuf,
    /// Where the catalog file goes.
    #[arg(long, default_value_os_t = repo_root().join("bookshop/catalog.bin"))]
    output: PathBuf,
    /// Validate and report without writing.
    #[arg(long)]
    check: bool,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap_or_else(|_| PathBuf::from("."))
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let text = std::fs::read_to_string(&args.input).with_context(|| format!("read {}", args.input.display()))?;
    let mut cat: Catalog = serde_json::from_str(&text).context("catalog.json")?;
    validate(&cat)?;
    if cat.fetched == 0 {
        cat.fetched = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() as u32;
    }
    let bytes = bookshop::encode(&cat);
    let file = CatalogFile::parse(bytes.as_slice()).context("the encoded catalog does not parse back")?;
    if file.count() as usize != cat.books.len() {
        bail!("encoded {} books, expected {}", file.count(), cat.books.len());
    }
    println!("{} books, {} bytes", cat.books.len(), bytes.len());
    for k in 0..6 {
        let names = ["Start here", "Popular this week", "New editions", "Modern & Creative Commons", "Collections", "By subject"];
        let ids = file.shelf(k);
        let rows = file.rows(&ids[..ids.len().min(3)]);
        println!("  {:<26} {:>2} rows: {}", names[k], ids.len(), rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>().join(" · "));
    }
    if args.check {
        return Ok(());
    }
    std::fs::write(&args.output, &bytes).with_context(|| format!("write {}", args.output.display()))?;
    println!("wrote {}", args.output.display());
    Ok(())
}

fn validate(cat: &Catalog) -> anyhow::Result<()> {
    if cat.books.len() < 60 {
        bail!("only {} books; the shelves need at least 60", cat.books.len());
    }
    let mut ids = std::collections::HashSet::new();
    let mut ranks = std::collections::HashSet::new();
    for b in &cat.books {
        if !ids.insert(b.id.as_str()) {
            bail!("duplicate id {}", b.id);
        }
        if !ranks.insert(b.rank) {
            bail!("duplicate rank {} ({})", b.rank, b.title);
        }
        if b.title.trim().is_empty() || b.author.trim().is_empty() {
            bail!("{}: title and author are required", b.id);
        }
        if !b.url.starts_with("https://") {
            bail!("{}: the download URL must be https", b.id);
        }
        if b.blurb.trim().is_empty() || b.blurb.len() > 400 {
            bail!("{}: blurb missing or over 400 bytes", b.id);
        }
        if b.subjects.is_empty() || b.hours10 == 0 || b.size == 0 {
            bail!("{}: subjects, hours10 and size are required", b.id);
        }
        if b.title.len() > 56 || b.author.len() > 32 {
            eprintln!("note: {}: title/author longer than the row fields; lists show them shortened", b.id);
        }
    }
    if cat.books.iter().filter(|b| b.collection.as_deref() == Some("Start here")).count() < 3 {
        bail!("the Start here shelf needs at least 3 books");
    }
    Ok(())
}

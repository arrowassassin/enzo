//! Quire simulator: boots the real UI on a fixture card, tours every screen, and writes
//! PNG snapshots plus a report; also renders reading pages at several type sizes.

use anyhow::Result;
use clap::Parser;
use quire_layout::{render_page, NoImages, Paginator, Pos, Profile};
use quire_qtx::{ParaKind, Token, Writer};
use quire_sim::{fixture_card, frame_to_png, tour, Sim};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "quire-sim", about = "Render Quire screens and pages headlessly")]
struct Args {
    /// Write a markdown report (per-screen timings, refresh kinds) to this path.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Write PNG snapshots into this directory.
    #[arg(long)]
    snapshots: Option<PathBuf>,
    /// Use an existing card directory instead of the built-in fixture card.
    #[arg(long)]
    card: Option<PathBuf>,
}

const FIXTURE: &str = include_str!("../../../crates/quire-layout/fixtures/middlemarch.txt");

fn chapter() -> Vec<u8> {
    let mut w = Writer::new();
    w.push(&Token::ChapterTitle { number: Some("1".into()), title: Some("Miss Brooke".into()) });
    for para in FIXTURE.lines().filter(|l| !l.trim().is_empty()) {
        w.para(ParaKind::Body);
        w.text(para);
    }
    w.finish()
}

fn reading_page(profile: Profile, start: Pos) -> (quire_gfx::Frame, Option<Pos>, u32) {
    let qtx = chapter();
    let mut frame = quire_gfx::Frame::panel();
    frame.clear(quire_gfx::Ink::White);
    let geom = profile.geometry(frame.width(), frame.height());
    let pg = Paginator::new(&qtx, profile, geom);
    let page = pg.page_from(start).expect("page");
    render_page(&page, &mut frame, &profile, &NoImages);
    (frame, page.next, page.chars)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut report =
        String::from("# Quire simulator report\n\n## Screens\n\n| screen | stack | refresh | ms | ink px |\n|---|---|---|---|---|\n");

    let card = match &args.card {
        Some(c) => c.clone(),
        None => fixture_card("cli"),
    };
    let mut sim = Sim::boot(&card);
    let shots = tour(&mut sim);
    let mut slowest = (0u128, String::new());
    for s in &shots {
        report.push_str(&format!("| {} | {} | {:?} | {} | {} |\n", s.name, s.stack.join(" › "), s.refresh, s.ms, s.frame.ink_count()));
        if s.ms > slowest.0 {
            slowest = (s.ms, s.name.clone());
        }
    }

    report.push_str("\n## Reading pages\n\n| page | size | line height | chars | ink px |\n|---|---|---|---|---|\n");
    let mut pages: Vec<(String, quire_gfx::Frame)> = Vec::new();
    let default = Profile::default();
    let (f1, next, chars) = reading_page(default, Pos::START);
    report.push_str(&format!("| layout-chapter | {} | {} | {} | {} |\n", default.size, default.line_height_pct, chars, f1.ink_count()));
    pages.push(("layout-chapter".into(), f1));
    if let Some(n) = next {
        let (f2, _, chars) = reading_page(default, n);
        report.push_str(&format!("| layout-default | {} | {} | {} | {} |\n", default.size, default.line_height_pct, chars, f2.ink_count()));
        pages.push(("layout-default".into(), f2));
    }
    for (name, p) in [
        ("layout-22", Profile { size: 22, line_height_pct: 130, ..default }),
        ("layout-34", Profile { size: 34, line_height_pct: 160, ..default }),
    ] {
        let (f, _, chars) = reading_page(p, Pos::START);
        report.push_str(&format!("| {name} | {} | {} | {} | {} |\n", p.size, p.line_height_pct, chars, f.ink_count()));
        pages.push((name.into(), f));
    }

    if let Some(dir) = &args.snapshots {
        std::fs::create_dir_all(dir)?;
        for s in &shots {
            frame_to_png(&s.frame, &dir.join(format!("{}.png", s.name)))?;
        }
        for (name, f) in &pages {
            frame_to_png(f, &dir.join(format!("{name}.png")))?;
        }
    }
    if let Some(p) = &args.report {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        report.push_str(&format!(
            "\n- screens captured: {}\n- slowest event: {} ({} ms, host debug/release as built)\n- font packs: {}\n",
            shots.len(),
            slowest.1,
            slowest.0,
            quire_fonts::PACKS.len()
        ));
        std::fs::write(p, report)?;
    }
    println!("ok: {} screens, {} pages rendered; slowest {} at {} ms", shots.len(), pages.len(), slowest.1, slowest.0);
    if args.card.is_none() {
        let _ = std::fs::remove_dir_all(&card);
    }
    Ok(())
}

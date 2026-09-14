//! Quire simulator: renders the real engine to PNG so screens and pages can be reviewed
//! and snapshot-tested without hardware.

use anyhow::Result;
use clap::Parser;
use quire_gfx::{draw_text, Frame, Ink, Rect, TextStyle};
use quire_layout::{render_page, NoImages, Paginator, Pos, Profile};
use quire_qtx::{ParaKind, Token, Writer};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "quire-sim", about = "Render Quire screens and pages headlessly")]
struct Args {
    /// Write a markdown report (budgets, page statistics) to this path.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Write PNG snapshots into this directory.
    #[arg(long)]
    snapshots: Option<PathBuf>,
}

const FIXTURE: &str = include_str!("../../../crates/quire-layout/fixtures/middlemarch.txt");

fn frame_to_png(frame: &Frame, path: &Path) -> Result<()> {
    let (w, h) = (frame.width(), frame.height());
    let mut img = image::GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = if frame.get(x as i32, y as i32) { 0 } else { 255 };
            img.put_pixel(x, y, image::Luma([v]));
        }
    }
    img.save(path)?;
    Ok(())
}

fn chapter() -> Vec<u8> {
    let mut w = Writer::new();
    w.push(&Token::ChapterTitle { number: Some("1".into()), title: Some("Miss Brooke".into()) });
    for para in FIXTURE.lines().filter(|l| !l.trim().is_empty()) {
        w.para(ParaKind::Body);
        w.text(para);
    }
    w.finish()
}

/// The running head and the Spine: the two pieces of chrome the reading page carries.
fn chrome(frame: &mut Frame, profile: &Profile, book: &str, chapter: &str, frac: f32, chapters: &[f32]) {
    let f = quire_fonts::ui::label();
    let st = TextStyle { tracking: 1, ..TextStyle::INK };
    let m = profile.margin as i32;
    if profile.running_head {
        let y = m + f.ascent();
        draw_text(frame, f, m, y, &book.to_uppercase(), st);
        let w = quire_gfx::measure_text(f, &chapter.to_uppercase(), st);
        let right = frame.width() as i32 - m - if profile.spine { (quire_layout::SPINE_W + quire_layout::SPINE_GUTTER) as i32 } else { 0 };
        draw_text(frame, f, right - w, y, &chapter.to_uppercase(), st);
    }
    if profile.spine {
        let x = frame.width() as i32 - m - quire_layout::SPINE_W as i32;
        let top = m + quire_layout::RUNNING_HEAD_H as i32;
        let bottom = frame.height() as i32 - m;
        let h = bottom - top;
        let read_h = (h as f32 * frac) as i32;
        let mut y = top;
        while y < bottom {
            let full = y < top + read_h;
            frame.hline(x, y, if full { 12 } else { 6 }, 1, Ink::Black);
            y += 4;
        }
        for c in chapters {
            let cy = top + (h as f32 * c) as i32;
            frame.hline(x + 9, cy, 3, 3, Ink::Black);
        }
        frame.hline(x - 1, top + read_h, 14, 2, Ink::Black);
    }
}

fn reading_page(profile: Profile, start: Pos) -> (Frame, Option<Pos>, u32) {
    let qtx = chapter();
    let mut frame = Frame::panel();
    frame.clear(Ink::White);
    let geom = profile.geometry(frame.width(), frame.height());
    let pg = Paginator::new(&qtx, profile, geom);
    let page = pg.page_from(start).expect("page");
    render_page(&page, &mut frame, &profile, &NoImages);
    chrome(&mut frame, &profile, "Middlemarch", "Chapter 1", 0.43, &[0.0, 0.28, 0.61, 0.84]);
    (frame, page.next, page.chars)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut report = String::from("# Quire simulator report\n\n| page | size | line height | chars | ink px |\n|---|---|---|---|---|\n");

    let mut shots: Vec<(String, Frame)> = Vec::new();
    let default = Profile::default();
    let (f1, next, chars) = reading_page(default, Pos::START);
    report.push_str(&format!("| 20-reading-chapter | {} | {} | {} | {} |\n", default.size, default.line_height_pct, chars, f1.ink_count()));
    shots.push(("20-reading-chapter".into(), f1));

    if let Some(n) = next {
        let (f2, _, chars) = reading_page(default, n);
        report.push_str(&format!(
            "| 20-reading-default | {} | {} | {} | {} |\n",
            default.size,
            default.line_height_pct,
            chars,
            f2.ink_count()
        ));
        shots.push(("20-reading-default".into(), f2));
    }

    for (name, p) in [
        ("20-reading-22", Profile { size: 22, line_height_pct: 130, ..default }),
        ("20-reading-34", Profile { size: 34, line_height_pct: 160, ..default }),
    ] {
        let (f, _, chars) = reading_page(p, Pos::START);
        report.push_str(&format!("| {name} | {} | {} | {} | {} |\n", p.size, p.line_height_pct, chars, f.ink_count()));
        shots.push((name.into(), f));
    }

    if let Some(dir) = &args.snapshots {
        std::fs::create_dir_all(dir)?;
        for (name, f) in &shots {
            frame_to_png(f, &dir.join(format!("{name}.png")))?;
        }
    }
    if let Some(p) = &args.report {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        report.push_str(&format!("\n- font packs: {}\n", quire_fonts::PACKS.len()));
        std::fs::write(p, report)?;
    }
    println!("ok: {} pages rendered, {} font packs", shots.len(), quire_fonts::PACKS.len());
    let _ = Rect::default();
    Ok(())
}

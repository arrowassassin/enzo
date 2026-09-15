//! Moons: the lunar phases. A large disc with stippled craters and dithered maria,
//! the night side as a faint earthshine, the phase name in small caps, and a strip
//! of phase glyphs along the foot.

use std::f32::consts::PI;

use crate::canvas::{Canvas, Paint, Rect};
use crate::noise::{fbm2, Rng};
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::text;
use crate::{seed_for, Art, H, W};

/// The pack.
pub const PACK: Pack =
    Pack { id: "moons", name: "Moons", description: "Lunar phases with stippled craters and dithered maria", count: PER_PACK, render };

/// Phase as illuminated fraction and whether it is waxing (lit on the right).
const PHASES: [(f32, bool, &str, &str); 5] = [
    (0.22, true, "Waxing Crescent", "Day 4"),
    (0.5, true, "First Quarter", "Day 7"),
    (0.82, true, "Waxing Gibbous", "Day 11"),
    (1.0, true, "Full Moon", "Day 15"),
    (0.3, false, "Waning Crescent", "Day 25"),
];

/// Whether a normalised disc point `(u, v)` is lit at illuminated fraction `k`.
fn lit(u: f32, v: f32, k: f32, waxing: bool) -> bool {
    let u = if waxing { u } else { -u };
    let a = (PI * k).cos();
    let half = (1.0 - v * v).max(0.0).sqrt();
    u > a * half
}

fn crater(c: &mut Canvas, rng: &mut Rng, cx: f32, cy: f32, r: f32, sun_dir: f32) {
    let bb = Rect::new((cx - r - 3.0) as i32, (cy - r - 3.0) as i32, (2.0 * r + 6.0) as i32, (2.0 * r + 6.0) as i32);
    let (sx, sy) = (sun_dir.cos(), sun_dir.sin());
    c.stipple(bb, rng, Paint::Ink, |x, y| {
        let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
        let d = (dx * dx + dy * dy).sqrt() / r;
        if d > 1.08 {
            return 0.0;
        }
        // Rim: dense on the shadow side, sparse on the lit side; floor lightly shaded
        // towards the sun-facing wall.
        let facing = (dx * sx + dy * sy) / (r * d.max(0.05));
        let rim = (-(((d - 0.92) / 0.09).powi(2))).exp();
        let shadow_rim = rim * (0.75 - 0.55 * facing).clamp(0.08, 1.0);
        let floor = if d < 0.85 { 0.3 * (0.5 + 0.5 * facing).clamp(0.0, 1.0) * (1.0 - d) } else { 0.0 };
        (shadow_rim + floor).min(1.0)
    });
    // A paper highlight along the lit rim reads as the crater wall catching light.
    let a0 = sun_dir + PI - 0.9;
    c.arc(cx, cy, r * 0.95, a0, a0 + 1.8, 1.5, Paint::Paper);
}

#[allow(clippy::too_many_arguments)]
fn moon(c: &mut Canvas, rng: &mut Rng, cx: f32, cy: f32, r: f32, k: f32, waxing: bool, seed: u32) {
    let bb = Rect::new((cx - r - 2.0) as i32, (cy - r - 2.0) as i32, (2.0 * r + 4.0) as i32, (2.0 * r + 4.0) as i32);
    // Maria: a blotchy dark-grey field from thresholded noise, only on the lit side.
    c.shade(bb, |x, y| {
        let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
        let d2 = dx * dx + dy * dy;
        if d2 > r * r {
            return None;
        }
        let (u, v) = (dx / r, dy / r);
        if !lit(u, v, k, waxing) {
            return Some(Paint::Gray(0.12));
        }
        let n = fbm2(dx * 0.011 + 3.0, dy * 0.011 + 1.0, 4, seed);
        let m = ((n - 0.47) / 0.08).clamp(0.0, 1.0);
        let limb = (1.0 - d2 / (r * r)).sqrt();
        let tone = 1.0 - 0.42 * m * (0.5 + 0.5 * limb);
        Some(if tone > 0.985 { Paint::Paper } else { Paint::Gray(tone) })
    });
    // Craters, biggest first so small ones sit on top.
    let mut craters: Vec<(f32, f32, f32)> = Vec::new();
    let mut tries = 0;
    while craters.len() < 34 && tries < 4000 {
        tries += 1;
        let a = rng.range(0.0, 2.0 * PI);
        let d = rng.f32().sqrt() * 0.9;
        let (u, v) = (d * a.cos(), d * a.sin());
        let cr = if craters.len() < 4 { rng.range(22.0, 32.0) } else { rng.range(4.0, 14.0) };
        if !lit(u, v, k, waxing) {
            continue;
        }
        let (x, y) = (cx + u * r, cy + v * r);
        if craters.iter().any(|&(ox, oy, or)| ((ox - x).powi(2) + (oy - y).powi(2)).sqrt() < or + cr + 4.0) {
            continue;
        }
        craters.push((x, y, cr));
    }
    let sun_dir = if waxing { 0.0 } else { PI };
    for &(x, y, cr) in &craters {
        crater(c, rng, x, y, cr, sun_dir);
    }
    // Limb: a crisp paper outline against the night sky; the night side is a faint
    // earthshine stipple with no craters.
    c.ring(cx, cy, r + 1.0, 1.5, Paint::Paper);
    c.shade(bb, |x, y| {
        let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
        if dx * dx + dy * dy > (r - 0.5) * (r - 0.5) {
            return None;
        }
        if lit(dx / r, dy / r, k, waxing) {
            None
        } else {
            Some(Paint::Gray(0.1))
        }
    });
}

fn phase_glyph(c: &mut Canvas, cx: f32, cy: f32, r: f32, k: f32, waxing: bool, current: bool) {
    c.disc(cx, cy, r, Paint::Paper);
    c.shade(Rect::new((cx - r) as i32, (cy - r) as i32, (2.0 * r + 1.0) as i32, (2.0 * r + 1.0) as i32), |x, y| {
        let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
        if dx * dx + dy * dy > r * r {
            return None;
        }
        Some(if lit(dx / r, dy / r, k, waxing) { Paint::Paper } else { Paint::Ink })
    });
    c.ring(cx, cy, r, 1.0, Paint::Ink);
    if current {
        c.ring(cx, cy, r + 5.0, 1.0, Paint::Ink);
    }
}

fn render(i: usize) -> Art {
    let seed = seed_for(PACK.id, i);
    let mut rng = Rng::new(seed);
    let s32 = (seed >> 7) as u32;
    let (k, waxing, name, day) = PHASES[i % PHASES.len()];
    let mut c = Canvas::new();
    let (cx, cy, r) = (W as f32 / 2.0, 306.0, 196.0);
    // Night sky above the caption band, with paper stars.
    let split = 546;
    c.fill_rect(Rect::new(0, 0, W as i32, split), Paint::Ink);
    let mut stars = rng.fork(2);
    for _ in 0..90 {
        let (x, y) = (stars.below(W) as i32, 12 + stars.below(split as u32 - 24) as i32);
        if ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() < r + 14.0 {
            continue;
        }
        if stars.chance(0.12) {
            c.sparkle(x, y, 3, Paint::Paper);
        } else {
            c.put(x, y, Paint::Paper);
        }
    }
    moon(&mut c, &mut rng, cx, cy, r, k, waxing, s32);
    // Caption and the phase strip.
    let slot = ClockSlot::centered(W as i32 / 2, 606, ClockStyle::Poster, Surface::Paper);
    c.fill_rect(Rect::new(0, split, W as i32, H as i32 - split), Paint::Paper);
    text::centered(&mut c, quire_fonts::ui::label_bold(), W as i32 / 2, 668, &text::small_caps(name), Paint::Ink, 4);
    text::centered(&mut c, quire_fonts::ui::serif_small(), W as i32 / 2, 692, day, Paint::Ink, 0);
    c.line(cx - 40.0, split as f32 + 7.0, cx + 40.0, split as f32 + 7.0, 1.0, Paint::Ink);
    let strip_y = 748.0;
    let glyphs = [(0.02, true), (0.25, true), (0.5, true), (0.75, true), (1.0, true), (0.75, false), (0.5, false), (0.25, false)];
    let n = glyphs.len();
    for (j, &(gk, gw)) in glyphs.iter().enumerate() {
        let gx = 84.0 + (W as f32 - 168.0) * j as f32 / (n - 1) as f32;
        let current = (gw == waxing || k >= 0.99) && (gk - k).abs() < 0.16;
        phase_glyph(&mut c, gx, strip_y, 9.0, gk, gw, current);
    }
    Art { canvas: c, slot, title: name.into(), credit: CREDIT.into() }
}

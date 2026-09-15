//! Mountains: layered ridgelines with dithered atmospheric perspective — far ranges
//! fade to paper, the nearest is solid ink — under a sun or a moon.

use std::f32::consts::PI;

use crate::canvas::{Canvas, Paint, Rect};
use crate::noise::{fbm1, fbm2, noise1, Rng};
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::{seed_for, Art, H, W};

/// The pack.
pub const PACK: Pack = Pack {
    id: "mountains",
    name: "Mountains",
    description: "Layered ridgelines fading into dithered haze, under a sun or a moon",
    count: PER_PACK,
    render,
};

/// Ridged multifractal in `[0, 1]`: sharp peaks, soft valleys.
fn ridged(x: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        let n = noise1(x * freq, seed.wrapping_add(o * 977));
        let r = 1.0 - (2.0 * n - 1.0).abs();
        sum += amp * r * r;
        norm += amp;
        amp *= 0.55;
        freq *= 2.1;
    }
    sum / norm
}

/// One ridge: `y = base - amp * envelope * ridged(x)`.
#[derive(Clone, Copy)]
struct Ridge {
    base: f32,
    amp: f32,
    freq: f32,
    seed: u32,
}

impl Ridge {
    fn y(&self, x: f32) -> f32 {
        let env = 0.55 + 0.45 * noise1(x * self.freq * 0.35 + 7.0, self.seed ^ 0x55);
        self.base - self.amp * env * ridged(x * self.freq, 5, self.seed)
    }
    fn points(&self) -> Vec<(f32, f32)> {
        let mut pts: Vec<(f32, f32)> = (0..=W as i32 / 2).map(|i| (i as f32 * 2.0, self.y(i as f32 * 2.0))).collect();
        pts.push((W as f32, H as f32 + 2.0));
        pts.push((0.0, H as f32 + 2.0));
        pts
    }
}

/// Paint a ridge: a tone that lightens into mist towards its foot, and a crisp crest.
fn paint_ridge(c: &mut Canvas, r: &Ridge, tone: f32, mist_depth: f32, crest_ink: bool) {
    let pts = r.points();
    let ys: Vec<f32> = (0..W as i32).map(|x| r.y(x as f32)).collect();
    c.polygon_with(&pts, |x, y| {
        let d = (y as f32 - ys[x as usize]).max(0.0) / mist_depth;
        let g = tone + (1.0 - tone) * (d * d).min(1.0);
        if g <= 0.02 {
            Paint::Ink
        } else if g >= 0.98 {
            Paint::Paper
        } else {
            Paint::Gray(g)
        }
    });
    if crest_ink {
        for x in 0..W as i32 - 1 {
            c.line(x as f32, ys[x as usize], x as f32 + 1.0, ys[x as usize + 1], 1.0, Paint::Ink);
        }
    }
}

fn sun(c: &mut Canvas, cx: f32, cy: f32, r: f32, rays: usize, rng: &mut Rng) {
    c.disc(cx, cy, r, Paint::Paper);
    c.ring(cx, cy, r, 2.0, Paint::Ink);
    for i in 0..rays {
        let a = 2.0 * PI * i as f32 / rays as f32 + rng.range(-0.03, 0.03);
        let (r0, r1) = (r + 9.0, r + 9.0 + if i % 2 == 0 { 22.0 } else { 12.0 });
        c.line(cx + r0 * a.cos(), cy + r0 * a.sin(), cx + r1 * a.cos(), cy + r1 * a.sin(), 1.5, Paint::Ink);
    }
}

fn crescent(c: &mut Canvas, cx: f32, cy: f32, r: f32) {
    c.disc(cx, cy, r, Paint::Ink);
    c.disc(cx + r * 0.42, cy - r * 0.18, r * 0.86, Paint::Paper);
}

fn stars(c: &mut Canvas, rng: &mut Rng, area: Rect, n: usize, avoid: impl Fn(i32, i32) -> bool) {
    for _ in 0..n {
        let x = area.x + rng.below(area.w as u32) as i32;
        let y = area.y + rng.below(area.h as u32) as i32;
        if avoid(x, y) {
            continue;
        }
        match rng.below(10) {
            0 => c.sparkle(x, y, 4, Paint::Ink),
            1..=3 => c.disc(x as f32, y as f32, 1.2, Paint::Ink),
            _ => c.put(x, y, Paint::Ink),
        }
    }
}

fn ranges(seed: u32, n: usize, top: f32, bottom: f32, amp0: f32, amp1: f32) -> Vec<Ridge> {
    (0..n)
        .map(|k| {
            let f = k as f32 / (n - 1).max(1) as f32;
            Ridge {
                base: top + (bottom - top) * f,
                amp: amp0 + (amp1 - amp0) * f,
                freq: 0.006 + 0.004 * f,
                seed: seed.wrapping_add(k as u32 * 7919),
            }
        })
        .collect()
}

fn paint_ranges(c: &mut Canvas, ridges: &[Ridge], lightest: f32, darkest: f32, mist: f32) {
    let n = ridges.len();
    for (k, r) in ridges.iter().enumerate() {
        let f = k as f32 / (n - 1).max(1) as f32;
        let tone = lightest + (darkest - lightest) * f;
        let last = k + 1 == n;
        paint_ridge(c, r, if last { 0.0 } else { tone }, if last { 1.0e9 } else { mist }, true);
    }
}

fn dawn(c: &mut Canvas, rng: &mut Rng, seed: u32) -> ClockSlot {
    sun(c, 340.0, 312.0, 54.0, 36, rng);
    let ridges = ranges(seed, 5, 340.0, 610.0, 70.0, 120.0);
    paint_ranges(c, &ridges, 0.9, 0.62, 170.0);
    // Birds: a few double strokes in the high sky.
    for _ in 0..5 {
        let (x, y) = (rng.range(60.0, 470.0), rng.range(215.0, 275.0));
        let s = rng.range(7.0, 13.0);
        c.polyline(
            &[(x - s, y + s * 0.15), (x - s * 0.5, y - s * 0.2), (x, y + s * 0.4), (x + s * 0.5, y - s * 0.2), (x + s, y + s * 0.15)],
            1.5,
            Paint::Ink,
        );
    }
    ClockSlot::centered(W as i32 / 2, 138, ClockStyle::Hero, Surface::Paper)
}

fn night(c: &mut Canvas, rng: &mut Rng, seed: u32) -> ClockSlot {
    let ridges = ranges(seed, 4, 380.0, 620.0, 80.0, 130.0);
    let slot = ClockSlot::centered(W as i32 / 2, 128, ClockStyle::Hero, Surface::Paper);
    let keep = slot.rect().grow(24);
    stars(c, rng, Rect::new(16, 16, W as i32 - 32, 300), 140, |x, y| {
        keep.contains(x, y) || ((x - 150).pow(2) + (y - 250).pow(2)) < 70 * 70
    });
    crescent(c, 150.0, 250.0, 40.0);
    paint_ranges(c, &ridges, 0.88, 0.6, 150.0);
    slot
}

fn mist(c: &mut Canvas, seed: u32) -> ClockSlot {
    let ridges = ranges(seed, 7, 300.0, 640.0, 60.0, 110.0);
    // No black range here: the nearest is a mid tone that the mist swallows.
    let n = ridges.len();
    for (k, r) in ridges.iter().enumerate() {
        let f = k as f32 / (n - 1) as f32;
        paint_ridge(c, r, 0.92 - 0.4 * f, 130.0, true);
    }
    // Drifting mist: soft horizontal bands that eat into the ridges.
    let bands = [(360.0, 22.0, 0.9), (455.0, 30.0, 0.95), (560.0, 26.0, 0.8), (680.0, 30.0, 1.0)];
    c.fade(Rect::new(0, 250, W as i32, H as i32 - 250), |x, y| {
        let mut k: f32 = 0.0;
        for (i, (cy, sigma, strength)) in bands.iter().enumerate() {
            let wob = (fbm1(x as f32 * 0.008 + i as f32 * 31.0, 3, seed ^ 0x9a) - 0.5) * 60.0;
            let d = (y as f32 - cy - wob) / sigma;
            let along = 0.55 + 0.45 * fbm1(x as f32 * 0.012 + i as f32 * 101.0, 3, seed ^ 0x3c);
            k = k.max(strength * along * (-d * d).exp());
        }
        k.max(((y - 660) as f32 / 60.0).clamp(0.0, 1.0))
    });
    ClockSlot::centered(W as i32 / 2, 130, ClockStyle::Hero, Surface::Paper)
}

fn lake(c: &mut Canvas, rng: &mut Rng, seed: u32) -> ClockSlot {
    let shore = 520.0;
    sun(c, 200.0, 300.0, 42.0, 0, rng);
    let ridges = ranges(seed, 4, 330.0, shore, 90.0, 140.0);
    for (k, r) in ridges.iter().enumerate() {
        let f = k as f32 / 3.0;
        let tone = 0.8 - 0.5 * f;
        let last = k == 3;
        // Ridges reach the shore with a hard edge: no mist at their feet.
        paint_ridge(c, r, if last { 0.0 } else { tone }, 900.0, true);
    }
    // Reflection: mirror the scene, wobbled and lightened, then cut ripple lines.
    let scene = c.clone();
    let shore_i = shore as i32;
    // Water is a line screen: every other pair of rows carries the mirrored scene,
    // the rows between stay paper, and the screen coarsens with depth.
    c.shade(Rect::new(0, shore_i, W as i32, H as i32 - shore_i), |x, y| {
        let depth = (y - shore_i) as f32;
        let pitch = 3 + (depth / 90.0) as i32;
        let phase = (y - shore_i).rem_euclid(pitch * 2);
        if phase >= pitch {
            return Some(Paint::Paper);
        }
        let wob = (fbm2(x as f32 * 0.02, y as f32 * 0.12, 3, seed ^ 0x77) - 0.5) * (3.0 + depth * 0.08);
        let sy = (2.0 * shore - y as f32 + wob).round() as i32;
        let v = scene.value((x as f32 + wob * 0.5) as i32, sy);
        Some(Paint::Gray(v + (1.0 - v) * (depth / 900.0)))
    });
    let mut ripples = rng.fork(3);
    for _ in 0..40 {
        let y = shore_i + 8 + ripples.below((H as i32 - shore_i - 16) as u32) as i32;
        let len = ripples.range(30.0, 160.0);
        let x = ripples.range(0.0, W as f32 - len);
        c.line(x, y as f32, x + len, y as f32, 2.0, Paint::Paper);
    }
    c.line(0.0, shore, W as f32, shore, 1.0, Paint::Ink);
    ClockSlot::centered(W as i32 / 2, 150, ClockStyle::Hero, Surface::Paper)
}

fn peak(c: &mut Canvas, rng: &mut Rng, seed: u32) -> ClockSlot {
    // Far ranges in light tone.
    let far = ranges(seed ^ 0x1234, 2, 400.0, 470.0, 60.0, 80.0);
    paint_ranges(c, &far, 0.86, 0.72, 160.0);
    c.fade(Rect::new(0, 300, W as i32, 300), |_, y| ((y - 430) as f32 / 120.0).clamp(0.0, 1.0));
    // The peak: a jagged triangle whose left face is lit, right face hatched.
    let (apex_x, apex_y) = (250.0, 250.0);
    let mut left: Vec<(f32, f32)> = Vec::new();
    let mut right: Vec<(f32, f32)> = Vec::new();
    for i in 0..=40 {
        let t = i as f32 / 40.0;
        let jag = (ridged(t * 9.0 + 3.0, 3, seed ^ 0x51) - 0.5) * 26.0 * t;
        left.push((apex_x - t * 250.0, apex_y + t * 330.0 + jag));
        let jag = (ridged(t * 9.0 + 40.0, 3, seed ^ 0x52) - 0.5) * 26.0 * t;
        right.push((apex_x + t * 300.0, apex_y + t * 330.0 + jag));
    }
    let mut poly: Vec<(f32, f32)> = left.iter().rev().copied().collect();
    poly.extend(right.iter().copied());
    poly.push((W as f32 + 10.0, H as f32));
    poly.push((-10.0, H as f32));
    c.polygon(&poly, Paint::Paper);
    // Shadow face: 55° hatching that thickens towards the ridge.
    let ridge_line: Vec<(f32, f32)> = right.clone();
    let shadow: Vec<(f32, f32)> = ridge_line.iter().copied().chain([(W as f32 + 10.0, H as f32), (apex_x, H as f32)]).collect();
    c.polygon(&shadow, Paint::Paper);
    let shadow_poly = shadow.clone();
    let inside = move |x: i32, y: i32| point_in_polygon(x as f32 + 0.5, y as f32 + 0.5, &shadow_poly);
    c.hatch(Rect::new(0, 200, W as i32, H as i32 - 200), 0.95, 5.0, 1.6, 0.0, Paint::Ink, inside);
    // Lit face: sparse contour strokes following the slope.
    let mut strokes = rng.fork(9);
    for _ in 0..90 {
        let t = strokes.range(0.15, 1.0);
        let x0 = apex_x - t * 250.0 * strokes.range(0.2, 1.0);
        let y0 = apex_y + t * 330.0 + strokes.range(-10.0, 10.0);
        let len = strokes.range(10.0, 40.0);
        c.line(x0, y0, x0 - len * 0.6, y0 + len * 0.8, 1.0, Paint::Ink);
    }
    // Crest.
    c.polyline(&left, 2.0, Paint::Ink);
    c.polyline(&right, 2.0, Paint::Ink);
    // Snow cap: paper wedge with a scalloped lower edge.
    let mut cap: Vec<(f32, f32)> = vec![(apex_x, apex_y - 2.0)];
    for i in 0..=12 {
        let t = i as f32 / 12.0;
        let x = apex_x + 120.0 - t * 200.0;
        let y = apex_y + 95.0 + (t * 6.0 * PI).sin() * 10.0 + (t - 0.5).abs() * 30.0;
        cap.push((x, y));
    }
    c.polygon(&cap, Paint::Paper);
    c.polyline(&cap[1..], 1.0, Paint::Ink);
    c.polyline(&left[..12], 2.0, Paint::Ink);
    c.polyline(&right[..12], 2.0, Paint::Ink);
    // Foreground: a dark forest band with spiky tree tops.
    let mut forest: Vec<(f32, f32)> = Vec::new();
    let mut x = -10.0;
    let mut trees = rng.fork(4);
    while x < W as f32 + 10.0 {
        let w = trees.range(10.0, 22.0);
        let h = trees.range(30.0, 70.0);
        let base = 660.0 + (fbm1(x * 0.01, 3, seed ^ 0x8) - 0.5) * 60.0;
        forest.push((x, base));
        forest.push((x + w / 2.0, base - h));
        x += w;
    }
    forest.push((W as f32 + 10.0, H as f32 + 2.0));
    forest.push((-10.0, H as f32 + 2.0));
    c.polygon(&forest, Paint::Ink);
    sun(c, 420.0, 170.0, 30.0, 24, rng);
    ClockSlot::centered(120, 120, ClockStyle::Poster, Surface::Paper)
}

/// Even-odd point in polygon.
pub fn point_in_polygon(x: f32, y: f32, poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn render(i: usize) -> Art {
    let seed = seed_for(PACK.id, i);
    let mut rng = Rng::new(seed);
    let s32 = (seed >> 11) as u32;
    let mut c = Canvas::new();
    let (slot, title) = match i {
        0 => (dawn(&mut c, &mut rng, s32), "Dawn"),
        1 => (night(&mut c, &mut rng, s32), "Crescent"),
        2 => (mist(&mut c, s32), "Mist"),
        3 => (lake(&mut c, &mut rng, s32), "Lake"),
        _ => (peak(&mut c, &mut rng, s32), "The Peak"),
    };
    Art { canvas: c, slot, title: title.into(), credit: CREDIT.into() }
}

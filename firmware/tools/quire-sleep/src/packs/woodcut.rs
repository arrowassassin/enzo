//! Woodcut: black ground with paper cut-lines — engraved hills whose contour strokes
//! thicken towards the light, a sky of wobbling gouge lines, and a rough-edged block
//! border. Ink density is kept near a half by the density of the cuts.

use std::f32::consts::{PI, TAU};

use crate::canvas::{Canvas, Paint, Rect};
use crate::noise::{fbm1, noise1, Rng};
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::{seed_for, Art, H, W};

/// The pack.
pub const PACK: Pack = Pack {
    id: "woodcut",
    name: "Woodcut",
    description: "Block-print landscapes: engraved hills, gouged skies and a rough border",
    count: PER_PACK,
    render,
};

const INSET: i32 = 16;

/// The rough block border: an ink band whose inner edge wobbles like a hand-cut block.
fn block_border(c: &mut Canvas, seed: u32) {
    let band = 12.0;
    c.shade_all(|x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let dx = fx.min(W as f32 - 1.0 - fx);
        let dy = fy.min(H as f32 - 1.0 - fy);
        let d = dx.min(dy);
        let along = if dx < dy { fy } else { fx };
        let edge = band + 3.5 * (noise1(along * 0.09, seed) - 0.5) + 6.0 * (noise1(along * 0.013, seed ^ 0x9) - 0.5);
        if d < edge {
            Some(Paint::Ink)
        } else if d < edge + 4.0 {
            Some(Paint::Paper)
        } else {
            None
        }
    });
}

fn inner() -> Rect {
    Rect::new(INSET + 8, INSET + 8, W as i32 - 2 * (INSET + 8), H as i32 - 2 * (INSET + 8))
}

/// A ridge line for hill `k`.
fn ridge(x: f32, base: f32, amp: f32, seed: u32) -> f32 {
    let off = (seed % 977) as f32 * 0.37;
    base - amp * (0.2 + 0.8 * fbm1(x * 0.0032 + off, 2, seed))
}

/// Engrave a hill: ink body, paper contour strokes that follow the ridge and thicken
/// on the side facing `light` (−1 left … +1 right).
#[allow(clippy::too_many_arguments)]
fn hill(c: &mut Canvas, base: f32, amp: f32, seed: u32, next: impl Fn(f32) -> f32, pitch: f32, light: f32, area: Rect) {
    let ys: Vec<f32> = (0..W as i32).map(|x| ridge(x as f32, base, amp, seed)).collect();
    let slopes: Vec<f32> = (0..W as i32).map(|x| ys[(x + 4).min(W as i32 - 1) as usize] - ys[(x - 4).max(0) as usize]).collect();
    c.shade(area, |x, y| {
        let top = ys[x as usize];
        let fy = y as f32;
        if fy < top || fy >= next(x as f32) {
            return None;
        }
        let d = fy - top;
        // Facing: a positive slope (falling to the right) faces right.
        let facing = (slopes[x as usize] / 3.0 * light).clamp(-1.0, 1.0);
        let shade = 0.5 + 0.5 * facing;
        let wob = 1.5 * (noise1(x as f32 * 0.04 + d * 0.02, seed ^ 0x77) - 0.5);
        // Cuts are widest at the crest and close up towards the foot of the hill.
        let t = (0.6 + 4.4 * shade) * (1.0 - (d / 260.0).min(0.85));
        let cut = (d + wob).rem_euclid(pitch) < t;
        // Broken strokes: occasional gaps in the cut.
        let gap = noise1(x as f32 * 0.03 + (d / pitch).floor() * 17.0, seed ^ 0x31) < 0.07;
        Some(if cut && !gap && d > 2.0 { Paint::Paper } else { Paint::Ink })
    });
}

/// Gouged sky: paper ground with wobbling horizontal ink lines that break up.
fn gouged_sky(c: &mut Canvas, area: Rect, horizon: impl Fn(f32) -> f32, seed: u32, pitch: f32) {
    c.shade(area, |x, y| {
        let fy = y as f32;
        if fy >= horizon(x as f32) {
            return None;
        }
        let wob = 2.0 * (noise1(x as f32 * 0.02 + fy * 0.01, seed ^ 0x5) - 0.5);
        let row = ((fy + wob) / pitch).floor();
        let along = noise1(x as f32 * 0.012 + row * 13.0, seed ^ 0x6);
        let t = 1.0 + 1.5 * along;
        let cut = (fy + wob).rem_euclid(pitch) < t && along > 0.3;
        if cut {
            Some(Paint::Ink)
        } else {
            None
        }
    });
}

/// Ink sky with paper gouges (night).
fn ink_sky(c: &mut Canvas, area: Rect, horizon: impl Fn(f32) -> f32, seed: u32, pitch: f32) {
    c.shade(area, |x, y| {
        let fy = y as f32;
        if fy >= horizon(x as f32) {
            return None;
        }
        let wob = 2.0 * (noise1(x as f32 * 0.02 + fy * 0.01, seed ^ 0x5) - 0.5);
        let row = ((fy + wob) / pitch).floor();
        let along = noise1(x as f32 * 0.01 + row * 13.0, seed ^ 0x6);
        let t = 1.5 + 2.5 * along;
        let cut = (fy + wob).rem_euclid(pitch) < t && along > 0.2;
        Some(if cut { Paint::Paper } else { Paint::Ink })
    });
}

fn sun(c: &mut Canvas, cx: f32, cy: f32, r: f32, rays: usize, on_ink: bool) {
    let (fg, bg) = if on_ink { (Paint::Paper, Paint::Ink) } else { (Paint::Ink, Paint::Paper) };
    // On paper: a paper disc with an ink ring and rays. On ink: a paper moon.
    for i in 0..rays {
        let a = 2.0 * PI * i as f32 / rays as f32;
        let (r0, r1) = (r + 8.0, r + 8.0 + if i % 2 == 0 { 34.0 } else { 20.0 });
        let w = 0.055;
        c.polygon(
            &[
                (cx + r0 * (a - w).cos(), cy + r0 * (a - w).sin()),
                (cx + r1 * a.cos(), cy + r1 * a.sin()),
                (cx + r0 * (a + w).cos(), cy + r0 * (a + w).sin()),
            ],
            fg,
        );
    }
    c.disc(cx, cy, r + 3.0, bg);
    c.ring(cx, cy, r + 3.0, 3.0, fg);
    if on_ink {
        c.disc(cx, cy, r - 4.0, Paint::Paper);
    }
}

fn pine(c: &mut Canvas, x: f32, base: f32, h: f32, w: f32) {
    // Three stacked triangles with a paper outline so it reads on any ground.
    let tiers = [(0.0, 1.0), (0.3, 0.78), (0.55, 0.55)];
    for &(lift, scale) in &tiers {
        let y0 = base - h * lift;
        let hw = w * scale / 2.0;
        let tip = y0 - h * 0.5 * scale;
        let pts = [(x - hw, y0), (x, tip), (x + hw, y0)];
        let outline = [(x - hw - 3.0, y0 + 3.0), (x, tip - 4.0), (x + hw + 3.0, y0 + 3.0)];
        c.polygon(&outline, Paint::Paper);
        c.polygon(&pts, Paint::Ink);
    }
    c.fill_rect(Rect::new(x as i32 - 2, base as i32, 4, 8), Paint::Ink);
    // Branch cuts.
    let mut y = base - h * 0.15;
    while y > base - h * 0.95 {
        let hw = w * 0.5 * ((base - y) / h).max(0.1);
        c.line(x - hw * 0.3, y, x - hw * 0.9, y + 5.0, 1.0, Paint::Paper);
        c.line(x + hw * 0.3, y, x + hw * 0.9, y + 5.0, 1.0, Paint::Paper);
        y -= 9.0;
    }
}

fn cartouche(c: &mut Canvas, slot: &ClockSlot) {
    let r = slot.rect().grow(10);
    c.fill_rect(r.grow(6), slot.on.paint());
    c.stroke_rect(r, 2, slot.on.paint().inverse());
}

fn render(i: usize) -> Art {
    let seed = seed_for(PACK.id, i);
    let mut rng = Rng::new(seed);
    let s32 = (seed >> 3) as u32;
    let mut c = Canvas::new();
    let area = inner();
    let (slot, title) = match i {
        0 => {
            // Sunrise over four engraved hills.
            let bases = [470.0, 560.0, 650.0, 760.0];
            let amps = [150.0, 130.0, 110.0, 90.0];
            let horizon = move |x: f32| ridge(x, bases[0], amps[0], s32);
            gouged_sky(&mut c, area, horizon, s32, 9.0);
            sun(&mut c, 380.0, 250.0, 46.0, 18, false);
            for k in 0..4 {
                let next_base = if k + 1 < 4 { bases[k + 1] } else { H as f32 + 10.0 };
                let next_amp = if k + 1 < 4 { amps[k + 1] } else { 0.0 };
                let ns = s32.wrapping_add((k as u32 + 1) * 101);
                let next = move |x: f32| if k + 1 < 4 { ridge(x, next_base, next_amp, ns) } else { H as f32 + 10.0 };
                hill(&mut c, bases[k], amps[k], s32.wrapping_add(k as u32 * 101), next, 7.0 + k as f32, 1.0 - 0.3 * k as f32, area);
            }
            let slot = ClockSlot::centered(W as i32 / 2, 120, ClockStyle::Hero, Surface::Paper);
            cartouche(&mut c, &slot);
            (slot, "Sunrise")
        }
        1 => {
            // Night sea: ink sky with paper gouges, a moon, black water with paper waves.
            let horizon = 400.0;
            ink_sky(&mut c, area, move |_| horizon, s32, 7.0);
            sun(&mut c, 160.0, 230.0, 40.0, 0, true);
            c.fill_rect(Rect::new(area.x, horizon as i32, area.w, area.bottom() - horizon as i32), Paint::Ink);
            let mut y = horizon + 4.0;
            let mut k = 0;
            while y < H as f32 {
                let depth = (y - horizon) / (H as f32 - horizon);
                let pitch = 5.0 + 12.0 * depth;
                let amp = pitch * 0.28;
                let lambda = 50.0 + 200.0 * depth;
                let mut pts = Vec::new();
                for xi in (area.x..area.right()).step_by(2) {
                    let x = xi as f32;
                    let v = (x * TAU / lambda + k as f32 * 0.3).sin() + 0.4 * (x * TAU / lambda * 2.7 + k as f32).sin();
                    pts.push((x, y + amp * v));
                }
                for w in pts.windows(2) {
                    let glit = (w[0].0 - 160.0).abs() < 14.0 + 50.0 * depth;
                    let t = if glit { 3.0 + 2.5 * depth } else { 1.5 + 2.5 * depth };
                    if glit && noise1(w[0].0 * 0.2 + y, s32) < 0.4 {
                        continue;
                    }
                    c.line(w[0].0, w[0].1, w[1].0, w[1].1, t, Paint::Paper);
                }
                y += pitch;
                k += 1;
            }
            c.fill_rect(Rect::new(area.x, horizon as i32, area.w, 2), Paint::Paper);
            let slot = ClockSlot::centered(W as i32 / 2, 110, ClockStyle::Hero, Surface::Ink);
            cartouche(&mut c, &slot);
            (slot, "Night sea")
        }
        2 => {
            // Forest: pines on hills, back rows small, front rows large.
            let bases = [500.0, 610.0, 720.0];
            let amps = [110.0, 90.0, 70.0];
            let horizon = move |x: f32| ridge(x, bases[0], amps[0], s32);
            gouged_sky(&mut c, area, horizon, s32, 10.0);
            sun(&mut c, 150.0, 240.0, 40.0, 16, false);
            for k in 0..3 {
                let next_base = if k + 1 < 3 { bases[k + 1] } else { H as f32 + 10.0 };
                let next_amp = if k + 1 < 3 { amps[k + 1] } else { 0.0 };
                let ns = s32.wrapping_add((k as u32 + 1) * 101);
                let next = move |x: f32| if k + 1 < 3 { ridge(x, next_base, next_amp, ns) } else { H as f32 + 10.0 };
                hill(&mut c, bases[k], amps[k], s32.wrapping_add(k as u32 * 101), next, 8.0, -1.0, area);
                let mut trees = rng.fork(k as u64 + 1);
                let n = 14 - k * 3;
                for j in 0..n {
                    let x = area.x as f32 + 10.0 + (area.w as f32 - 20.0) * (j as f32 + trees.range(0.2, 0.8)) / n as f32;
                    let base_y = ridge(x, bases[k], amps[k], s32.wrapping_add(k as u32 * 101)) + trees.range(4.0, 20.0);
                    let h = 40.0 + 30.0 * k as f32 + trees.range(0.0, 20.0);
                    pine(&mut c, x, base_y, h, h * 0.55);
                }
            }
            let slot = ClockSlot::centered(W as i32 / 2, 120, ClockStyle::Hero, Surface::Paper);
            cartouche(&mut c, &slot);
            (slot, "Pines")
        }
        3 => {
            // Wheat: a paper field of ink stalks under engraved hills and a low sun.
            let bases = [400.0, 470.0];
            let amps = [120.0, 90.0];
            let field_top = 480.0;
            let horizon = move |x: f32| ridge(x, bases[0], amps[0], s32);
            gouged_sky(&mut c, area, horizon, s32, 9.0);
            sun(&mut c, 264.0, 300.0, 56.0, 20, false);
            for k in 0..2 {
                let ns = s32.wrapping_add((k as u32 + 1) * 101);
                let next = move |x: f32| if k == 0 { ridge(x, bases[1], amps[1], ns) } else { field_top };
                hill(&mut c, bases[k], amps[k], s32.wrapping_add(k as u32 * 101), next, 7.0, 0.6, area);
            }
            c.fill_rect(Rect::new(area.x, field_top as i32, area.w, area.bottom() - field_top as i32), Paint::Paper);
            let mut stalks = rng.fork(7);
            let mut xs: Vec<f32> = (0..70).map(|_| stalks.range(area.x as f32 + 6.0, area.right() as f32 - 6.0)).collect();
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for (j, &x) in xs.iter().enumerate() {
                let h = 120.0 + 160.0 * (j % 7) as f32 / 6.0 + stalks.range(-10.0, 10.0);
                let lean = stalks.range(-18.0, 18.0);
                let top = (x + lean, H as f32 - 20.0 - h);
                c.line(x, H as f32 - 18.0, top.0, top.1, 1.5, Paint::Ink);
                // The ear: paired grains up the last 40 px.
                for g in 0..7 {
                    let t = g as f32 / 7.0;
                    let gx = top.0 - lean * 0.15 * t;
                    let gy = top.1 + 42.0 * (1.0 - t) - 6.0;
                    c.line(gx, gy, gx - 6.0, gy - 5.0, 2.0, Paint::Ink);
                    c.line(gx, gy, gx + 6.0, gy - 5.0, 2.0, Paint::Ink);
                }
                // Awns.
                c.line(top.0, top.1 - 4.0, top.0 + lean * 0.3, top.1 - 22.0, 1.0, Paint::Ink);
            }
            let slot = ClockSlot::centered(W as i32 / 2, 120, ClockStyle::Hero, Surface::Paper);
            cartouche(&mut c, &slot);
            (slot, "Harvest")
        }
        _ => {
            // Storm: ink sky, paper rain, a lightning fork over black hills.
            let bases = [540.0, 640.0, 740.0];
            let amps = [130.0, 110.0, 90.0];
            let horizon = move |x: f32| ridge(x, bases[0], amps[0], s32);
            ink_sky(&mut c, area, horizon, s32, 8.0);
            // Rain: slanted paper lines with gaps.
            c.hatch(area, 1.35, 7.0, 1.2, 0.0, Paint::Paper, |x, y| {
                (y as f32) < ridge(x as f32, bases[0], amps[0], s32) && noise1(x as f32 * 0.02 + y as f32 * 0.09, s32 ^ 0x44) > 0.45
            });
            // Lightning.
            let mut bolt = vec![(330.0, area.y as f32 + 10.0)];
            let mut b = rng.fork(3);
            let (mut x, mut y) = (330.0, area.y as f32 + 10.0);
            while y < 420.0 {
                x += b.range(-34.0, 26.0);
                y += b.range(24.0, 48.0);
                bolt.push((x, y));
            }
            c.polyline(&bolt, 14.0, Paint::Ink);
            c.polyline(&bolt, 6.0, Paint::Paper);
            let (bx, by) = bolt[bolt.len() / 2];
            c.polyline(&[(bx, by), (bx + 34.0, by + 46.0), (bx + 28.0, by + 84.0)], 3.0, Paint::Paper);
            for k in 0..3 {
                let next_base = if k + 1 < 3 { bases[k + 1] } else { H as f32 + 10.0 };
                let next_amp = if k + 1 < 3 { amps[k + 1] } else { 0.0 };
                let ns = s32.wrapping_add((k as u32 + 1) * 101);
                let next = move |x: f32| if k + 1 < 3 { ridge(x, next_base, next_amp, ns) } else { H as f32 + 10.0 };
                hill(&mut c, bases[k], amps[k], s32.wrapping_add(k as u32 * 101), next, 8.0, 0.4, area);
            }
            let slot = ClockSlot::centered(W as i32 / 2, 110, ClockStyle::Hero, Surface::Ink);
            cartouche(&mut c, &slot);
            (slot, "Storm")
        }
    };
    block_border(&mut c, s32);
    Art { canvas: c, slot, title: title.into(), credit: CREDIT.into() }
}

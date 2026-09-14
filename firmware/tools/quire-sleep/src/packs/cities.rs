//! Cities: skylines at night — ink towers with lit paper windows, hatched back
//! ranks, a moon, and water that carries the lights as broken streaks.

use std::f32::consts::PI;

use crate::canvas::{Canvas, Paint, Rect};
use crate::noise::{fbm1, Rng};
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::{seed_for, Art, H, W};

/// The pack.
pub const PACK: Pack = Pack {
    id: "cities",
    name: "Cities",
    description: "Night skylines with lit windows, a harbour bridge, old-town spires and rain",
    count: PER_PACK,
    render,
};

#[derive(Clone)]
struct Building {
    x: i32,
    w: i32,
    top: i32,
    kind: Kind,
    lit: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Flat,
    Stepped,
    Spire,
    Pitched,
    Dome,
    Tower,
}

fn skyline(rng: &mut Rng, ground: i32, min_h: i32, max_h: i32, min_w: i32, max_w: i32, kinds: &[Kind]) -> Vec<Building> {
    let mut v = Vec::new();
    let mut x = -(rng.below(30) as i32);
    while x < W as i32 {
        let w = min_w + rng.below((max_w - min_w) as u32) as i32;
        let h = min_h + rng.below((max_h - min_h) as u32) as i32;
        let kind = kinds[rng.below(kinds.len() as u32) as usize];
        v.push(Building { x, w, top: ground - h, kind, lit: rng.range(0.25, 0.75) });
        x += w + rng.below(6) as i32;
    }
    v
}

/// Draw a rank of buildings. `front` buildings get windows; back ranks are hatched.
fn draw_rank(c: &mut Canvas, rng: &mut Rng, rank: &[Building], ground: i32, front: bool) {
    for b in rank {
        let body = Rect::new(b.x, b.top, b.w, ground - b.top);
        let cap_h = match b.kind {
            Kind::Stepped => 26,
            Kind::Spire | Kind::Tower => 0,
            Kind::Pitched => b.w / 2,
            Kind::Dome => b.w / 2,
            Kind::Flat => 0,
        };
        let paint = Paint::Ink;
        if front {
            c.fill_rect(body, paint);
        } else {
            let r = body;
            c.fill_rect(r, Paint::Paper);
            c.hatch(r, 0.0, 3.0, 1.0, 0.0, Paint::Ink, |_, _| true);
            c.stroke_rect(r, 1, Paint::Ink);
        }
        let (cx, top) = (b.x as f32 + b.w as f32 / 2.0, b.top as f32);
        match b.kind {
            Kind::Stepped => {
                let w2 = b.w * 2 / 3;
                c.fill_rect(Rect::new(b.x + (b.w - w2) / 2, b.top - cap_h, w2, cap_h), paint);
                let w3 = b.w / 3;
                c.fill_rect(Rect::new(b.x + (b.w - w3) / 2, b.top - cap_h - 16, w3, 16), paint);
            }
            Kind::Spire => {
                c.polygon(&[(b.x as f32, top), (cx, top - b.w as f32 * 1.6), (b.x as f32 + b.w as f32, top)], paint);
                c.line(cx, top - b.w as f32 * 1.6, cx, top - b.w as f32 * 1.6 - 24.0, 1.5, Paint::Ink);
            }
            Kind::Tower => {
                c.line(cx, top, cx, top - 40.0, 2.0, Paint::Ink);
                c.line(cx - 6.0, top - 28.0, cx + 6.0, top - 28.0, 1.0, Paint::Ink);
                c.disc(cx, top - 42.0, 2.0, Paint::Ink);
            }
            Kind::Pitched => {
                c.polygon(&[(b.x as f32 - 3.0, top), (cx, top - cap_h as f32), (b.x as f32 + b.w as f32 + 3.0, top)], paint);
                if b.w > 30 && rng.chance(0.6) {
                    let chx = b.x as f32 + b.w as f32 * 0.7;
                    c.fill_rect(Rect::new(chx as i32, (top - cap_h as f32 * 0.6) as i32, 7, cap_h / 2 + 2), Paint::Ink);
                }
            }
            Kind::Dome => {
                c.arc(cx, top, b.w as f32 / 2.0, PI, 2.0 * PI, 1.0, Paint::Ink);
                c.polygon(
                    &super::super::canvas::circle_points(cx, top, b.w as f32 / 2.0, 40, PI).into_iter().take(21).collect::<Vec<_>>(),
                    paint,
                );
                c.line(cx, top - b.w as f32 / 2.0, cx, top - b.w as f32 / 2.0 - 14.0, 2.0, Paint::Ink);
            }
            Kind::Flat => {
                if front && b.w > 44 && rng.chance(0.5) {
                    water_tower(c, b.x as f32 + b.w as f32 * rng.range(0.25, 0.7), top);
                }
                if rng.chance(0.4) {
                    c.line(cx + 4.0, top, cx + 4.0, top - rng.range(10.0, 26.0), 1.0, Paint::Ink);
                }
            }
        }
        if front {
            windows(c, rng, b, ground);
        }
    }
}

fn water_tower(c: &mut Canvas, x: f32, roof: f32) {
    let (w, h) = (16.0, 18.0);
    c.polygon(
        &[(x - w / 2.0, roof - 8.0), (x + w / 2.0, roof - 8.0), (x + w / 2.0 - 2.0, roof - 8.0 - h), (x - w / 2.0 + 2.0, roof - 8.0 - h)],
        Paint::Ink,
    );
    c.polygon(&[(x - w / 2.0 - 1.0, roof - 8.0 - h), (x + w / 2.0 + 1.0, roof - 8.0 - h), (x, roof - 8.0 - h - 8.0)], Paint::Ink);
    c.line(x - w / 2.0 + 2.0, roof - 8.0, x - w / 2.0 + 2.0, roof, 1.5, Paint::Ink);
    c.line(x + w / 2.0 - 2.0, roof - 8.0, x + w / 2.0 - 2.0, roof, 1.5, Paint::Ink);
}

fn windows(c: &mut Canvas, rng: &mut Rng, b: &Building, ground: i32) {
    let (ww, wh, px, py) = (4, 6, 9, 12);
    let cols = (b.w - 6) / px;
    if cols < 1 {
        return;
    }
    let x0 = b.x + (b.w - cols * px) / 2 + (px - ww) / 2;
    let mut y = b.top + 8;
    while y + wh < ground - 4 {
        for k in 0..cols {
            if rng.chance(b.lit) {
                c.fill_rect(Rect::new(x0 + k * px, y, ww, wh), Paint::Paper);
            }
        }
        y += py;
    }
}

fn glow(c: &mut Canvas, ground: i32, depth: i32) {
    c.shade(Rect::new(0, ground - depth, W as i32, depth), |_, y| {
        let t = (ground - y) as f32 / depth as f32;
        let g = 0.8 + 0.2 * t;
        if g >= 0.995 {
            None
        } else {
            Some(Paint::Gray(g))
        }
    });
}

fn water(c: &mut Canvas, rng: &mut Rng, top: i32, columns: &[f32]) {
    c.fill_rect(Rect::new(0, top, W as i32, H as i32 - top), Paint::Ink);
    c.fill_rect(Rect::new(0, top, W as i32, 1), Paint::Paper);
    let mut y = top + 3;
    while y < H as i32 {
        let depth = (y - top) as f32 / (H as i32 - top) as f32;
        for x in (0..W as i32).step_by(2) {
            let bright = columns[x as usize];
            let p = bright * (0.5 - 0.3 * depth) * (0.4 + 0.6 * fbm1(x as f32 * 0.05 + y as f32 * 0.3, 2, 9));
            if rng.chance(p) {
                let len = 2 + rng.below(6) as i32;
                c.fill_rect(Rect::new(x, y, len, 1), Paint::Paper);
            }
        }
        y += 2 + (depth * 3.0) as i32;
    }
}

/// How much paper (light) sits in each column above `ground`, for reflections.
fn brightness(c: &Canvas, ground: i32, span: i32) -> Vec<f32> {
    (0..W as i32)
        .map(|x| {
            let mut n = 0;
            for y in (ground - span)..ground {
                if c.value(x, y) > 0.5 && c.is_ink(x, y - 1) != c.is_ink(x, y) {
                    n += 1;
                }
            }
            (n as f32 / 12.0).min(1.0)
        })
        .collect()
}

/// A suspension bridge in silhouette across the water, in front of the skyline:
/// drawn on a mask and stamped with inversion, so it is ink against the sky and
/// paper against the water.
fn bridge(target: &mut Canvas, deck_y: f32) {
    let mut mask = Canvas::new();
    let c = &mut mask;
    let (t1, t2) = (132.0, 396.0);
    let top = deck_y - 230.0;
    let sag = deck_y - 30.0;
    // Main cables: a parabola between the towers, straight runs to the anchors.
    let mut main = Vec::new();
    for i in 0..=120 {
        let x = t1 + (t2 - t1) * i as f32 / 120.0;
        let u = (x - (t1 + t2) / 2.0) / ((t2 - t1) / 2.0);
        main.push((x, sag - (sag - top) * u * u));
    }
    for p in main.iter().step_by(6) {
        c.line(p.0, p.1, p.0, deck_y, 1.5, Paint::Ink);
    }
    c.polyline(&main, 4.0, Paint::Ink);
    for (x0, x1) in [(t1, -10.0), (t2, W as f32 + 10.0)] {
        let mut side = Vec::new();
        for i in 0..=40 {
            let x = x0 + (x1 - x0) * i as f32 / 40.0;
            let u = i as f32 / 40.0;
            side.push((x, top + (deck_y + 6.0 - top) * u * u));
        }
        for p in side.iter().step_by(5).skip(1) {
            c.line(p.0, p.1, p.0, deck_y, 1.5, Paint::Ink);
        }
        c.polyline(&side, 4.0, Paint::Ink);
    }
    // Towers with cross-braces, and the deck with its railing.
    for tx in [t1, t2] {
        for leg in [-11.0, 11.0] {
            c.fill_rect(Rect::new((tx + leg) as i32 - 5, top as i32 - 10, 10, (deck_y - top) as i32 + 40), Paint::Ink);
        }
        for k in 0..4 {
            let y = top + 20.0 + k as f32 * 52.0;
            c.fill_rect(Rect::new(tx as i32 - 16, y as i32, 32, 8), Paint::Ink);
        }
        c.fill_rect(Rect::new(tx as i32 - 18, top as i32 - 14, 36, 6), Paint::Ink);
    }
    c.fill_rect(Rect::new(0, deck_y as i32, W as i32, 12), Paint::Ink);
    c.fill_rect(Rect::new(0, deck_y as i32 - 8, W as i32, 2), Paint::Ink);
    let mut x = 6;
    while x < W as i32 {
        c.fill_rect(Rect::new(x, deck_y as i32 - 8, 2, 8), Paint::Ink);
        x += 14;
    }
    target.stamp_invert(&mask);
}

fn moon(c: &mut Canvas, cx: f32, cy: f32, r: f32, full: bool) {
    c.disc(cx, cy, r, Paint::Paper);
    c.ring(cx, cy, r, 1.5, Paint::Ink);
    if !full {
        c.disc(cx - r * 0.35, cy - r * 0.1, r * 0.92, Paint::Paper);
        c.ring(cx - r * 0.35, cy - r * 0.1, r * 0.92, 1.5, Paint::Ink);
        // Erase the part of the inner ring outside the moon.
        c.shade(Rect::new((cx - 1.4 * r) as i32, (cy - 1.2 * r) as i32, (2.8 * r) as i32, (2.4 * r) as i32), |x, y| {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d > r + 0.5 {
                Some(Paint::Paper)
            } else {
                None
            }
        });
    }
}

fn stars(c: &mut Canvas, rng: &mut Rng, n: usize, max_y: i32, avoid: Rect) {
    for _ in 0..n {
        let (x, y) = (rng.below(W) as i32, rng.below(max_y as u32) as i32);
        if avoid.contains(x, y) {
            continue;
        }
        if rng.chance(0.1) {
            c.sparkle(x, y, 3, Paint::Ink);
        } else {
            c.disc(x as f32, y as f32, 1.0, Paint::Ink);
        }
    }
}

fn render(i: usize) -> Art {
    let seed = seed_for(PACK.id, i);
    let mut rng = Rng::new(seed);
    let mut c = Canvas::new();
    let (slot, title) = match i {
        0 => {
            let slot = ClockSlot::centered(W as i32 / 2, 120, ClockStyle::Hero, Surface::Paper);
            let ground = 700;
            stars(&mut c, &mut rng, 60, 380, slot.rect().grow(30));
            moon(&mut c, 410.0, 250.0, 34.0, false);
            glow(&mut c, ground - 260, 120);
            let back = skyline(&mut rng, ground, 220, 400, 24, 60, &[Kind::Flat, Kind::Flat, Kind::Stepped, Kind::Spire]);
            draw_rank(&mut c, &mut rng, &back, ground, false);
            let front = skyline(&mut rng, ground, 80, 300, 30, 80, &[Kind::Flat, Kind::Flat, Kind::Stepped, Kind::Tower, Kind::Spire]);
            draw_rank(&mut c, &mut rng, &front, ground, true);
            c.fill_rect(Rect::new(0, ground, W as i32, H as i32 - ground), Paint::Ink);
            (slot, "Downtown")
        }
        1 => {
            let slot = ClockSlot::centered(W as i32 / 2, 110, ClockStyle::Hero, Surface::Paper);
            let ground = 520;
            moon(&mut c, 120.0, 250.0, 40.0, true);
            glow(&mut c, ground, 110);
            // A low waterfront so the bridge stands against the sky.
            let back = skyline(&mut rng, ground, 90, 190, 22, 50, &[Kind::Flat, Kind::Stepped, Kind::Spire]);
            draw_rank(&mut c, &mut rng, &back, ground, false);
            let front = skyline(&mut rng, ground, 40, 120, 28, 70, &[Kind::Flat, Kind::Flat, Kind::Stepped, Kind::Tower]);
            draw_rank(&mut c, &mut rng, &front, ground, true);
            let cols = brightness(&c, ground, 170);
            water(&mut c, &mut rng, ground + 2, &cols);
            bridge(&mut c, 504.0);
            (slot, "Harbour")
        }
        2 => {
            let slot = ClockSlot::centered(W as i32 / 2, 120, ClockStyle::Hero, Surface::Paper);
            let ground = 640;
            // Hills behind the old town.
            c.shade(Rect::new(0, 380, W as i32, 260), |x, y| {
                let h = 470.0 + 60.0 * fbm1(x as f32 * 0.006, 3, 21);
                if (y as f32) > h {
                    Some(Paint::Gray(0.86))
                } else {
                    None
                }
            });
            let back = skyline(&mut rng, ground, 90, 180, 24, 56, &[Kind::Pitched, Kind::Pitched, Kind::Spire, Kind::Dome]);
            draw_rank(&mut c, &mut rng, &back, ground, false);
            let front = skyline(&mut rng, ground, 40, 110, 30, 64, &[Kind::Pitched, Kind::Pitched, Kind::Pitched, Kind::Flat]);
            draw_rank(&mut c, &mut rng, &front, ground, true);
            // One tall church spire.
            let sx = 330.0;
            c.fill_rect(Rect::new(sx as i32 - 16, 470, 32, ground - 470), Paint::Ink);
            c.polygon(&[(sx - 20.0, 470.0), (sx, 360.0), (sx + 20.0, 470.0)], Paint::Ink);
            c.line(sx, 360.0, sx, 340.0, 2.0, Paint::Ink);
            c.line(sx - 6.0, 348.0, sx + 6.0, 348.0, 2.0, Paint::Ink);
            c.fill_rect(Rect::new(sx as i32 - 5, 490, 10, 16), Paint::Paper);
            c.fill_rect(Rect::new(0, ground, W as i32, H as i32 - ground), Paint::Ink);
            // Cobbles: paper arcs in the street.
            let mut cob = rng.fork(4);
            for _ in 0..140 {
                let (x, y) = (cob.range(0.0, W as f32), cob.range(ground as f32 + 6.0, H as f32 - 4.0));
                let r = 3.0 + (y - ground as f32) * 0.03;
                c.arc(x, y, r, PI, 2.0 * PI, 1.0, Paint::Paper);
            }
            (slot, "Old town")
        }
        3 => {
            let slot = ClockSlot::centered(W as i32 / 2, 110, ClockStyle::Hero, Surface::Paper);
            let ground = 780;
            stars(&mut c, &mut rng, 40, 300, slot.rect().grow(30));
            moon(&mut c, 264.0, 400.0, 120.0, true);
            let front = skyline(&mut rng, ground, 180, 380, 60, 130, &[Kind::Flat, Kind::Flat, Kind::Stepped]);
            draw_rank(&mut c, &mut rng, &front, ground, true);
            c.fill_rect(Rect::new(0, ground, W as i32, H as i32 - ground), Paint::Ink);
            (slot, "Rooftops")
        }
        _ => {
            let slot = ClockSlot::centered(W as i32 / 2, 110, ClockStyle::Hero, Surface::Paper);
            let ground = 600;
            let s32 = (seed >> 5) as u32;
            c.hatch(Rect::new(0, 190, W as i32, ground - 190), 1.2, 11.0, 1.0, 0.0, Paint::Ink, |x, y| {
                fbm1(x as f32 * 0.02 + y as f32 * 0.04, 2, s32) > 0.42
            });
            let back = skyline(&mut rng, ground, 160, 320, 22, 52, &[Kind::Flat, Kind::Stepped, Kind::Spire]);
            draw_rank(&mut c, &mut rng, &back, ground, false);
            let front = skyline(&mut rng, ground, 60, 220, 28, 76, &[Kind::Flat, Kind::Flat, Kind::Stepped, Kind::Tower]);
            draw_rank(&mut c, &mut rng, &front, ground, true);
            let cols = brightness(&c, ground, 220);
            water(&mut c, &mut rng, ground, &cols);
            (slot, "Rain")
        }
    };
    Art { canvas: c, slot, title: title.into(), credit: CREDIT.into() }
}

//! Tides: fields of wave lines that thicken and spread towards the viewer, under a
//! sun or a moon whose light breaks up on the water. Line pitch never drops below
//! 4 px and neighbouring lines stay in phase, so the field is moiré-safe.

use std::f32::consts::TAU;

use crate::canvas::{Canvas, Paint, Rect};
use crate::noise::{fbm1, Rng};
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::{seed_for, Art, H, W};

/// The pack.
pub const PACK: Pack = Pack {
    id: "tides",
    name: "Tides",
    description: "Wave-line seas under a sun or a moon, drawn as a moiré-safe line field",
    count: PER_PACK,
    render,
};

#[derive(Clone, Copy)]
struct Sea {
    horizon: f32,
    /// Amplitude as a fraction of the line pitch (keep ≤ 0.3).
    amp: f32,
    /// Base wavelength at the horizon.
    lambda: f32,
    /// Extra short-wavelength chop in `[0, 1]`.
    chop: f32,
    /// Where the light path is, if any: (x, half-width at the bottom).
    glitter: Option<(f32, f32)>,
    seed: u32,
}

fn draw_sea(c: &mut Canvas, rng: &mut Rng, sea: &Sea, keep_out: Option<Rect>) {
    let mut y = sea.horizon + 3.0;
    let mut phase = rng.range(0.0, TAU);
    let depth_span = H as f32 - sea.horizon;
    while y < H as f32 + 10.0 {
        let depth = ((y - sea.horizon) / depth_span).clamp(0.0, 1.0);
        let pitch = 4.0 + 13.0 * depth.powf(1.35);
        let amp = sea.amp * pitch;
        let lambda = sea.lambda * (0.6 + 2.2 * depth);
        let thick = 1.0 + 1.6 * depth;
        let mut pts: Vec<(f32, f32)> = Vec::with_capacity(W as usize / 2 + 2);
        for i in 0..=(W as i32 / 2 + 1) {
            let x = i as f32 * 2.0;
            let k = TAU / lambda;
            let mut v = (x * k + phase).sin() + 0.35 * (x * k * 2.3 + phase * 1.7).sin();
            v += sea.chop * 0.5 * (x * k * 5.1 + phase * 0.3).sin();
            let drift = (fbm1(x * 0.004 + y * 0.01, 2, sea.seed) - 0.5) * pitch * 0.4;
            pts.push((x, y + amp * v + drift));
        }
        // The light path: the line breaks into bright dashes under the sun or moon.
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if let Some(r) = keep_out {
                if r.contains(a.0 as i32, a.1 as i32) || r.contains(b.0 as i32, b.1 as i32) {
                    continue;
                }
            }
            let mut t = thick;
            if let Some((gx, gw)) = sea.glitter {
                let half = 6.0 + gw * depth;
                let dx = (a.0 - gx).abs();
                if dx < half {
                    let gap = fbm1(a.0 * 0.15 + y * 0.9, 2, sea.seed ^ 0x33);
                    if gap < 0.5 {
                        continue;
                    }
                    t = thick + 1.0;
                }
            }
            c.line(a.0, a.1, b.0, b.1, t, Paint::Ink);
        }
        // Foam ticks on the crests of the nearer lines.
        if depth > 0.45 {
            for _ in 0..(3.0 + depth * 6.0) as usize {
                let i = rng.below(pts.len() as u32 - 1) as usize;
                let (x, py) = pts[i];
                if pts[i.saturating_sub(1)].1 > py && pts[(i + 1).min(pts.len() - 1)].1 > py {
                    let len = 3.0 + depth * 5.0;
                    c.line(x, py - 2.0, x - len, py - 2.0 - len * 0.5, 1.0, Paint::Ink);
                }
            }
        }
        phase += 0.22 + rng.range(-0.04, 0.04);
        y += pitch;
    }
}

fn sun(c: &mut Canvas, cx: f32, cy: f32, r: f32) {
    c.disc(cx, cy, r, Paint::Paper);
    c.ring(cx, cy, r, 2.0, Paint::Ink);
}

fn moon(c: &mut Canvas, cx: f32, cy: f32, r: f32, rng: &mut Rng) {
    c.disc(cx, cy, r, Paint::Paper);
    c.ring(cx, cy, r, 2.0, Paint::Ink);
    // A few stippled maria.
    let mut s = rng.fork(5);
    for _ in 0..4 {
        let (mx, my) = (cx + s.range(-r * 0.5, r * 0.5), cy + s.range(-r * 0.5, r * 0.5));
        let mr = s.range(r * 0.15, r * 0.3);
        c.stipple(
            Rect::new((mx - mr) as i32, (my - mr) as i32, (2.0 * mr) as i32 + 1, (2.0 * mr) as i32 + 1),
            &mut s,
            Paint::Ink,
            |x, y| {
                let d = ((x as f32 - mx).powi(2) + (y as f32 - my).powi(2)).sqrt() / mr;
                if d < 1.0 {
                    0.28 * (1.0 - d * d)
                } else {
                    0.0
                }
            },
        );
    }
}

fn clouds(c: &mut Canvas, rng: &mut Rng, y0: f32, y1: f32, n: usize) {
    // Long horizontal cloud streaks, a woodcut convention.
    for _ in 0..n {
        let y = rng.range(y0, y1);
        let len = rng.range(60.0, 220.0);
        let x = rng.range(-20.0, W as f32 - len + 20.0);
        for k in 0..3 {
            let yy = y + k as f32 * 4.0;
            let inset = k as f32 * 12.0;
            c.line(x + inset, yy, x + len - inset, yy, 1.0, Paint::Ink);
        }
    }
}

fn birds(c: &mut Canvas, rng: &mut Rng, n: usize, area: Rect) {
    for _ in 0..n {
        let (x, y) = (rng.range(area.x as f32, area.right() as f32), rng.range(area.y as f32, area.bottom() as f32));
        let s = rng.range(6.0, 12.0);
        c.polyline(
            &[(x - s, y + s * 0.15), (x - s * 0.5, y - s * 0.2), (x, y + s * 0.4), (x + s * 0.5, y - s * 0.2), (x + s, y + s * 0.15)],
            1.5,
            Paint::Ink,
        );
    }
}

fn render(i: usize) -> Art {
    let seed = seed_for(PACK.id, i);
    let mut rng = Rng::new(seed);
    let s32 = (seed >> 9) as u32;
    let mut c = Canvas::new();
    let (slot, title) = match i {
        0 => {
            let sea = Sea { horizon: 330.0, amp: 0.22, lambda: 90.0, chop: 0.0, glitter: Some((264.0, 70.0)), seed: s32 };
            sun(&mut c, 264.0, 296.0, 46.0);
            draw_sea(&mut c, &mut rng, &sea, None);
            c.line(0.0, sea.horizon, W as f32, sea.horizon, 1.0, Paint::Ink);
            birds(&mut c, &mut rng, 4, Rect::new(40, 200, 200, 60));
            (ClockSlot::centered(W as i32 / 2, 120, ClockStyle::Hero, Surface::Paper), "Calm")
        }
        1 => {
            let sea = Sea { horizon: 300.0, amp: 0.3, lambda: 140.0, chop: 0.15, glitter: None, seed: s32 };
            clouds(&mut c, &mut rng, 200.0, 280.0, 5);
            draw_sea(&mut c, &mut rng, &sea, None);
            c.line(0.0, sea.horizon, W as f32, sea.horizon, 1.0, Paint::Ink);
            (ClockSlot::centered(W as i32 / 2, 120, ClockStyle::Hero, Surface::Paper), "Swell")
        }
        2 => {
            // The clock sits in a chart cartouche among the waves.
            let sea = Sea { horizon: 240.0, amp: 0.26, lambda: 60.0, chop: 0.6, glitter: None, seed: s32 };
            let slot = ClockSlot::centered(W as i32 / 2, 560, ClockStyle::Poster, Surface::Paper);
            let frame = slot.rect().grow(10);
            draw_sea(&mut c, &mut rng, &sea, Some(frame.grow(4)));
            c.line(0.0, sea.horizon, W as f32, sea.horizon, 1.0, Paint::Ink);
            c.fill_rect(frame.grow(4), Paint::Paper);
            c.stroke_rect(frame, 2, Paint::Ink);
            c.stroke_rect(frame.grow(-5), 1, Paint::Ink);
            birds(&mut c, &mut rng, 6, Rect::new(60, 120, 400, 90));
            (slot, "Chop")
        }
        3 => {
            let sea = Sea { horizon: 350.0, amp: 0.2, lambda: 100.0, chop: 0.0, glitter: Some((372.0, 60.0)), seed: s32 };
            moon(&mut c, 372.0, 262.0, 54.0, &mut rng);
            let mut stars = rng.fork(2);
            for _ in 0..70 {
                let (x, y) = (stars.below(W) as i32, 20 + stars.below(300) as i32);
                if (x - 372).pow(2) + (y - 250).pow(2) < 80 * 80 || (x - 140).abs() < 130 && y < 190 {
                    continue;
                }
                if stars.chance(0.15) {
                    c.sparkle(x, y, 3, Paint::Ink);
                } else {
                    c.put(x, y, Paint::Ink);
                }
            }
            draw_sea(&mut c, &mut rng, &sea, None);
            c.line(0.0, sea.horizon, W as f32, sea.horizon, 1.0, Paint::Ink);
            (ClockSlot::centered(140, 130, ClockStyle::Poster, Surface::Paper), "Moonrise")
        }
        _ => {
            let sea = Sea { horizon: 310.0, amp: 0.3, lambda: 70.0, chop: 0.8, glitter: None, seed: s32 };
            // A squall: a dense cloud band and slanting rain.
            c.hatch(Rect::new(0, 190, W as i32, 70), 0.0, 3.0, 1.0, 0.0, Paint::Ink, |x, y| {
                let edge = 190.0 + 70.0 * fbm1(x as f32 * 0.01, 3, s32 ^ 0x5) * 0.6;
                let bottom = 262.0 - 30.0 * fbm1(x as f32 * 0.02 + 9.0, 3, s32 ^ 0x6);
                (y as f32) > edge && (y as f32) < bottom
            });
            c.hatch(Rect::new(0, 240, W as i32, 72), 1.25, 9.0, 1.0, 0.0, Paint::Ink, |x, y| {
                fbm1(x as f32 * 0.03 + y as f32 * 0.1, 2, s32 ^ 0x7) > 0.45
            });
            draw_sea(&mut c, &mut rng, &sea, None);
            c.line(0.0, sea.horizon, W as f32, sea.horizon, 1.0, Paint::Ink);
            (ClockSlot::centered(W as i32 / 2, 110, ClockStyle::Hero, Surface::Paper), "Squall")
        }
    };
    Art { canvas: c, slot, title: title.into(), credit: CREDIT.into() }
}

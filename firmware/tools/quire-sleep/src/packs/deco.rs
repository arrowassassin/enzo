//! Deco: art-deco frames, sunbursts, fans, chevrons and ziggurats — all crisp
//! geometry, with the clock in a central plaque.

use std::f32::consts::PI;

use crate::canvas::{Canvas, Paint, Rect};
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::{seed_for, Art, H, W};

/// The pack.
pub const PACK: Pack = Pack {
    id: "deco",
    name: "Deco",
    description: "Art-deco sunbursts, fans, chevrons and stepped frames around a clock plaque",
    count: PER_PACK,
    render,
};

const CX: f32 = W as f32 / 2.0;

/// The outer frame: a heavy border with an inner rule.
fn frame(c: &mut Canvas) -> Rect {
    let outer = Rect::new(14, 14, W as i32 - 28, H as i32 - 28);
    c.stroke_rect(outer, 6, Paint::Ink);
    c.stroke_rect(outer.grow(-10), 1, Paint::Ink);
    outer.grow(-11)
}

/// A plaque around the slot: paper field, double border, stepped corners.
fn plaque(c: &mut Canvas, slot: &ClockSlot) {
    let r = slot.rect().grow(14);
    c.fill_rect(r.grow(8), Paint::Paper);
    c.stroke_rect(r, 3, Paint::Ink);
    c.stroke_rect(r.grow(-6), 1, Paint::Ink);
    // Stepped corners: three decreasing squares outside each corner.
    for (sx, sy) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
        for k in 0..3 {
            let s = 10 - k * 3;
            let px = if sx > 0 { r.x - 8 - k * 7 } else { r.right() + 8 + k * 7 - s };
            let py = if sy > 0 { r.y - 8 - k * 7 } else { r.bottom() + 8 + k * 7 - s };
            c.fill_rect(Rect::new(px, py, s, s), Paint::Ink);
        }
    }
}

fn sunburst(c: &mut Canvas, inner: Rect, slot: &ClockSlot) {
    // Rays from a point below the plaque, alternating ink and paper wedges.
    let (ox, oy) = (CX, 560.0);
    let n = 28;
    for k in 0..n {
        if k % 2 == 1 {
            continue;
        }
        let a0 = PI + PI * k as f32 / n as f32;
        let a1 = PI + PI * (k + 1) as f32 / n as f32;
        let r = 900.0;
        c.polygon(&[(ox, oy), (ox + r * a0.cos(), oy + r * a0.sin()), (ox + r * a1.cos(), oy + r * a1.sin())], Paint::Ink);
    }
    // Keep the rays inside the frame.
    c.fill_rect(Rect::new(0, oy as i32, W as i32, H as i32 - oy as i32), Paint::Paper);
    c.fill_rect(Rect::new(0, 0, inner.x, H as i32), Paint::Paper);
    c.fill_rect(Rect::new(inner.right(), 0, W as i32 - inner.right(), H as i32), Paint::Paper);
    c.fill_rect(Rect::new(0, 0, W as i32, inner.y), Paint::Paper);
    // Concentric fan at the origin.
    for k in 0..5 {
        let r = 30.0 + k as f32 * 22.0;
        c.arc(ox, oy, r, PI, 2.0 * PI, if k % 2 == 0 { 6.0 } else { 2.0 }, Paint::Ink);
    }
    c.disc(ox, oy, 18.0, Paint::Ink);
    // Lower field: horizontal bands narrowing towards the bottom.
    let mut y = oy as i32 + 30;
    let mut k = 0;
    while y < inner.bottom() - 10 {
        let inset = k * 26;
        c.fill_rect(Rect::new(inner.x + inset, y, inner.w - 2 * inset, 8), Paint::Ink);
        y += 26;
        k += 1;
    }
    plaque(c, slot);
    frame(c);
}

fn fans(c: &mut Canvas, inner: Rect, slot: &ClockSlot) {
    // Overlapping scales, each a semicircle with nested arcs.
    let r = 44.0;
    let mut row = 0;
    let mut y = inner.y as f32 + r;
    while y < inner.bottom() as f32 + r {
        let offset = if row % 2 == 0 { 0.0 } else { r };
        let mut x = inner.x as f32 + offset;
        while x < inner.right() as f32 + r {
            c.disc(x, y, r, Paint::Paper);
            c.arc(x, y, r - 1.0, PI, 2.0 * PI, 2.5, Paint::Ink);
            for k in 1..4 {
                c.arc(x, y, r - 1.0 - k as f32 * 9.0, PI, 2.0 * PI, 1.0, Paint::Ink);
            }
            // Radial ribs.
            for k in 0..7 {
                let a = PI + PI * (k as f32 + 0.5) / 7.0;
                c.line(x + 8.0 * a.cos(), y + 8.0 * a.sin(), x + (r - 30.0) * a.cos(), y + (r - 30.0) * a.sin(), 1.0, Paint::Ink);
            }
            x += 2.0 * r;
        }
        y += r * 0.62;
        row += 1;
    }
    // Clip to the frame.
    c.fill_rect(Rect::new(0, 0, inner.x, H as i32), Paint::Paper);
    c.fill_rect(Rect::new(inner.right(), 0, W as i32 - inner.right(), H as i32), Paint::Paper);
    c.fill_rect(Rect::new(0, 0, W as i32, inner.y), Paint::Paper);
    c.fill_rect(Rect::new(0, inner.bottom(), W as i32, H as i32 - inner.bottom()), Paint::Paper);
    plaque(c, slot);
    frame(c);
}

fn chevrons(c: &mut Canvas, inner: Rect, slot: &ClockSlot) {
    let amp = 46.0;
    let period = 176.0;
    let mut y = inner.y as f32 - amp;
    let mut band = 0;
    while y < inner.bottom() as f32 + amp {
        let h = if band % 3 == 0 { 26.0 } else { 12.0 };
        let mut top: Vec<(f32, f32)> = Vec::new();
        let mut x = inner.x as f32 - period;
        while x <= inner.right() as f32 + period {
            let ph = ((x - CX) / period).rem_euclid(1.0);
            let tri = if ph < 0.5 { ph * 2.0 } else { 2.0 - ph * 2.0 };
            top.push((x, y + amp * tri));
            x += 4.0;
        }
        let mut poly = top.clone();
        for p in top.iter().rev() {
            poly.push((p.0, p.1 + h));
        }
        if band % 3 == 2 {
            let poly2 = poly.clone();
            c.hatch(Rect::new(inner.x, y as i32 - 2, inner.w, (2.0 * amp + h) as i32 + 4), 0.0, 4.0, 1.5, 0.0, Paint::Ink, |x, yy| {
                super::mountains::point_in_polygon(x as f32 + 0.5, yy as f32 + 0.5, &poly2)
            });
        } else {
            c.polygon(&poly, Paint::Ink);
        }
        y += h + 22.0;
        band += 1;
    }
    c.fill_rect(Rect::new(0, 0, inner.x, H as i32), Paint::Paper);
    c.fill_rect(Rect::new(inner.right(), 0, W as i32 - inner.right(), H as i32), Paint::Paper);
    c.fill_rect(Rect::new(0, 0, W as i32, inner.y), Paint::Paper);
    c.fill_rect(Rect::new(0, inner.bottom(), W as i32, H as i32 - inner.bottom()), Paint::Paper);
    plaque(c, slot);
    frame(c);
}

fn ziggurat(c: &mut Canvas, inner: Rect, slot: &ClockSlot) {
    // Nested stepped rectangles converging on the plaque, with fluted columns
    // filling the sides.
    let plaque_r = slot.rect().grow(30);
    let mut k = 0;
    loop {
        let inset = 18 + k * 22;
        let r = Rect::new(inner.x + inset, inner.y + inset, inner.w - 2 * inset, inner.h - 2 * inset);
        if r.w <= plaque_r.w + 40 || r.h <= plaque_r.h + 40 {
            break;
        }
        let t = if k % 2 == 0 { 5 } else { 2 };
        // A rectangle with stepped (notched) corners.
        let n = 14;
        c.fill_rect(Rect::new(r.x + n, r.y, r.w - 2 * n, t), Paint::Ink);
        c.fill_rect(Rect::new(r.x + n, r.bottom() - t, r.w - 2 * n, t), Paint::Ink);
        c.fill_rect(Rect::new(r.x, r.y + n, t, r.h - 2 * n), Paint::Ink);
        c.fill_rect(Rect::new(r.right() - t, r.y + n, t, r.h - 2 * n), Paint::Ink);
        for (sx, sy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let x = if sx == 0 { r.x } else { r.right() - n - t };
            let y = if sy == 0 { r.y + n - t } else { r.bottom() - n };
            c.fill_rect(Rect::new(x, y, n + t, t), Paint::Ink);
            let x = if sx == 0 { r.x + n - t } else { r.right() - n };
            let y = if sy == 0 { r.y } else { r.bottom() - n - t };
            c.fill_rect(Rect::new(x, y, t, n + t), Paint::Ink);
        }
        k += 1;
    }
    // Fluting: vertical lines between the innermost ring and the plaque, top and bottom.
    let inset = 18 + k * 22;
    let ring = Rect::new(inner.x + inset, inner.y + inset, inner.w - 2 * inset, inner.h - 2 * inset);
    let mut x = ring.x + 10;
    while x < ring.right() - 6 {
        c.fill_rect(Rect::new(x, ring.y + 8, 2, plaque_r.y - ring.y - 16), Paint::Ink);
        c.fill_rect(Rect::new(x, plaque_r.bottom() + 8, 2, ring.bottom() - plaque_r.bottom() - 16), Paint::Ink);
        x += 8;
    }
    plaque(c, slot);
    frame(c);
}

fn arches(c: &mut Canvas, inner: Rect, slot: &ClockSlot) {
    // Nested pointed arches rising from the foot of the frame, alternating solid
    // and hatched bands, with a sunburst of thin rays behind the tallest.
    let base_y = inner.bottom() as f32 - 30.0;
    for k in 0..14 {
        let a = 4.0 + k as f32 * 0.5;
        let r0 = inner.w as f32 / 2.0 + 60.0 - k as f32 * 32.0;
        let r1 = r0 - 16.0;
        if r1 < 20.0 {
            break;
        }
        let ang = (a * 40.0).to_radians().min(1.2);
        let _ = ang;
        c.arc(CX, base_y, r0, PI, 2.0 * PI, 2.0, Paint::Ink);
        if k % 2 == 0 {
            let (o, i) = (r0, r1);
            c.hatch(Rect::new(0, 0, W as i32, base_y as i32), 0.0, 5.0, 1.5, 0.0, Paint::Ink, |x, y| {
                let d = ((x as f32 - CX).powi(2) + (y as f32 - base_y).powi(2)).sqrt();
                d < o && d > i
            });
        }
    }
    // Thin rays from the origin through the arches, clipped to the frame.
    for k in 0..25 {
        let a = PI + PI * (k as f32 + 0.5) / 25.0;
        let r0 = inner.w as f32 / 2.0 + 60.0;
        c.line(CX + r0 * a.cos(), base_y + r0 * a.sin(), CX + 900.0 * a.cos(), base_y + 900.0 * a.sin(), 1.5, Paint::Ink);
    }
    c.fill_rect(Rect::new(0, base_y as i32, W as i32, H as i32 - base_y as i32), Paint::Paper);
    c.fill_rect(Rect::new(0, 0, inner.x, H as i32), Paint::Paper);
    c.fill_rect(Rect::new(inner.right(), 0, W as i32 - inner.right(), H as i32), Paint::Paper);
    c.fill_rect(Rect::new(0, 0, W as i32, inner.y), Paint::Paper);
    // Bottom band of small stepped blocks.
    let mut x = inner.x + 8;
    while x < inner.right() - 12 {
        c.fill_rect(Rect::new(x, base_y as i32 + 8, 8, 14), Paint::Ink);
        c.fill_rect(Rect::new(x + 2, base_y as i32 + 4, 4, 4), Paint::Ink);
        x += 16;
    }
    plaque(c, slot);
    frame(c);
}

fn render(i: usize) -> Art {
    let _seed = seed_for(PACK.id, i);
    let mut c = Canvas::new();
    let inner = Rect::new(25, 25, W as i32 - 50, H as i32 - 50);
    let (slot, title) = match i {
        0 => {
            let slot = ClockSlot::centered(W as i32 / 2, 330, ClockStyle::Hero, Surface::Paper);
            sunburst(&mut c, inner, &slot);
            (slot, "Sunburst")
        }
        1 => {
            let slot = ClockSlot::centered(W as i32 / 2, 396, ClockStyle::Hero, Surface::Paper);
            fans(&mut c, inner, &slot);
            (slot, "Fans")
        }
        2 => {
            let slot = ClockSlot::centered(W as i32 / 2, 396, ClockStyle::Hero, Surface::Paper);
            chevrons(&mut c, inner, &slot);
            (slot, "Chevrons")
        }
        3 => {
            let slot = ClockSlot::centered(W as i32 / 2, 396, ClockStyle::Hero, Surface::Paper);
            ziggurat(&mut c, inner, &slot);
            (slot, "Ziggurat")
        }
        _ => {
            let slot = ClockSlot::centered(W as i32 / 2, 300, ClockStyle::Hero, Surface::Paper);
            arches(&mut c, inner, &slot);
            (slot, "Arches")
        }
    };
    Art { canvas: c, slot, title: title.into(), credit: CREDIT.into() }
}

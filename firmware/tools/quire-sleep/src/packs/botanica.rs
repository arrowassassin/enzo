//! Botanica: ferns, bushes and grasses grown from L-systems and drawn as botanical
//! plates — tapered ink strokes, small leaves at the tips, a double-rule frame and a
//! Latin caption in the device's italic.

use std::f32::consts::PI;

use crate::canvas::{Canvas, Paint, Rect};
use crate::noise::Rng;
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::text;
use crate::{seed_for, Art, H, W};

/// The pack.
pub const PACK: Pack = Pack {
    id: "botanica",
    name: "Botanica",
    description: "Ferns, bushes and grasses grown from L-systems, drawn as botanical plates",
    count: PER_PACK,
    render,
};

/// Expand an L-system `iters` times. `rule` may pick stochastically.
fn expand(axiom: &str, iters: usize, rng: &mut Rng, rule: impl Fn(char, &mut Rng) -> Option<&'static str>) -> String {
    let mut s = axiom.to_string();
    for _ in 0..iters {
        let mut next = String::with_capacity(s.len() * 3);
        for ch in s.chars() {
            match rule(ch, rng) {
                Some(r) => next.push_str(r),
                None => next.push(ch),
            }
        }
        s = next;
    }
    s
}

/// One stroke of the turtle: endpoints, bracket depth, and whether it ends a branch.
struct Seg {
    a: (f32, f32),
    b: (f32, f32),
    depth: usize,
    tip: bool,
}

/// Walk the string with a turtle. Returns unit-scale segments.
fn turtle(s: &str, angle_deg: f32, heading_deg: f32, wobble: f32, rng: &mut Rng) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut stack: Vec<((f32, f32), f32)> = Vec::new();
    let mut pos = (0.0f32, 0.0f32);
    let mut heading = heading_deg.to_radians();
    let angle = angle_deg.to_radians();
    let mut depth = 0usize;
    let mut last_seg_at_depth: Option<usize> = None;
    for ch in s.chars() {
        match ch {
            'F' => {
                let np = (pos.0 + heading.cos(), pos.1 + heading.sin());
                segs.push(Seg { a: pos, b: np, depth, tip: false });
                last_seg_at_depth = Some(segs.len() - 1);
                pos = np;
            }
            '+' => heading += angle * (1.0 + rng.range(-wobble, wobble)),
            '-' => heading -= angle * (1.0 + rng.range(-wobble, wobble)),
            '[' => {
                stack.push((pos, heading));
                depth += 1;
            }
            ']' => {
                if let Some(i) = last_seg_at_depth.take() {
                    if segs[i].depth == depth {
                        segs[i].tip = true;
                    }
                }
                if let Some((p, h)) = stack.pop() {
                    pos = p;
                    heading = h;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    segs
}

/// Fit the segments into `target` (preserving aspect) and draw them with tapered strokes.
fn draw_plant(c: &mut Canvas, segs: &[Seg], target: Rect, base_t: f32, leaves: Option<(f32, f32)>, rng: &mut Rng) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for s in segs {
        for p in [s.a, s.b] {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
    }
    let scale = (target.w as f32 / (x1 - x0).max(1e-3)).min(target.h as f32 / (y1 - y0).max(1e-3));
    let ox = target.x as f32 + (target.w as f32 - (x1 - x0) * scale) / 2.0 - x0 * scale;
    let oy = target.y as f32 + (target.h as f32 - (y1 - y0) * scale) / 2.0 - y0 * scale;
    let map = |p: (f32, f32)| (ox + p.0 * scale, oy + p.1 * scale);
    let max_depth = segs.iter().map(|s| s.depth).max().unwrap_or(0).max(1) as f32;
    for s in segs {
        let (a, b) = (map(s.a), map(s.b));
        let t = (base_t * (1.0 - s.depth as f32 / (max_depth + 1.0)).powf(1.6)).max(1.0);
        c.line(a.0, a.1, b.0, b.1, t, Paint::Ink);
    }
    if let Some((leaf_len, chance)) = leaves {
        for s in segs.iter().filter(|s| s.tip) {
            if !rng.chance(chance) {
                continue;
            }
            let (a, b) = (map(s.a), map(s.b));
            let h = (b.1 - a.1).atan2(b.0 - a.0) + rng.range(-0.3, 0.3);
            leaf(c, b.0, b.1, h, leaf_len * rng.range(0.8, 1.2), leaf_len * 0.42);
        }
    }
}

/// A leaf: pointed ellipse along `heading` with a paper midrib.
fn leaf(c: &mut Canvas, x: f32, y: f32, heading: f32, len: f32, half_w: f32) {
    let (ch, sh) = (heading.cos(), heading.sin());
    let mut pts = Vec::new();
    for i in 0..=16 {
        let t = i as f32 / 16.0;
        let w = half_w * (PI * t).sin();
        pts.push((x + ch * len * t - sh * w, y + sh * len * t + ch * w));
    }
    for i in (0..=16).rev() {
        let t = i as f32 / 16.0;
        let w = half_w * (PI * t).sin();
        pts.push((x + ch * len * t + sh * w, y + sh * len * t - ch * w));
    }
    c.polygon(&pts, Paint::Ink);
    c.line(x + ch * len * 0.1, y + sh * len * 0.1, x + ch * len * 0.85, y + sh * len * 0.85, 1.0, Paint::Paper);
}

fn plate(c: &mut Canvas, number: &str, latin: &str, common: &str, caption_y: i32) {
    let frame = Rect::new(22, 22, W as i32 - 44, H as i32 - 44);
    c.stroke_rect(frame, 2, Paint::Ink);
    c.stroke_rect(frame.grow(-5), 1, Paint::Ink);
    let label = quire_fonts::ui::label_bold();
    let ital = quire_fonts::nearest(quire_fonts::Family::Literata, quire_fonts::Style::Italic, 24);
    text::centered(c, label, W as i32 / 2, caption_y, &text::small_caps(number), Paint::Ink, 3);
    text::centered(c, ital, W as i32 / 2, caption_y + 32, latin, Paint::Ink, 0);
    text::centered(c, quire_fonts::ui::serif_small(), W as i32 / 2, caption_y + 54, common, Paint::Ink, 0);
}

fn render(i: usize) -> Art {
    let seed = seed_for(PACK.id, i);
    let mut rng = Rng::new(seed);
    let mut c = Canvas::new();
    let slot = ClockSlot::centered(W as i32 / 2, 112, ClockStyle::Hero, Surface::Paper);
    let body = Rect::new(60, 190, W as i32 - 120, 470);
    let title = match i {
        0 => {
            let s = expand("X", 7, &mut rng, |ch, _| match ch {
                'X' => Some("F+[[X]-X]-F[-FX]+X"),
                'F' => Some("FF"),
                _ => None,
            });
            let segs = turtle(&s, 25.0, -82.0, 0.0, &mut rng);
            draw_plant(&mut c, &segs, body, 3.0, None, &mut rng);
            plate(&mut c, "Plate I", "Dryopteris filix-mas", "Male fern", 690);
            "Fern"
        }
        1 => {
            let s = expand("F", 4, &mut rng, |ch, _| match ch {
                'F' => Some("FF-[-F+F+F]+[+F-F-F]"),
                _ => None,
            });
            let segs = turtle(&s, 22.5, -90.0, 0.12, &mut rng);
            draw_plant(&mut c, &segs, body, 6.0, Some((12.0, 0.22)), &mut rng);
            plate(&mut c, "Plate II", "Corylus avellana", "Hazel", 690);
            "Hazel"
        }
        2 => {
            let s = expand("F", 5, &mut rng, |ch, rng| match ch {
                'F' => Some(match rng.below(3) {
                    0 => "F[+F]F[-F]F",
                    1 => "F[+F]F",
                    _ => "F[-F]F",
                }),
                _ => None,
            });
            let segs = turtle(&s, 25.7, -90.0, 0.15, &mut rng);
            draw_plant(&mut c, &segs, body, 5.0, Some((10.0, 0.45)), &mut rng);
            plate(&mut c, "Plate III", "Betula pendula", "Silver birch, sapling", 690);
            "Sapling"
        }
        3 => {
            // A meadow: tufts of grass of varied height across the foot of the plate.
            let mut tufts = rng.fork(11);
            let n = 9;
            for k in 0..n {
                let s = expand("F", 3, &mut tufts, |ch, _| match ch {
                    'F' => Some("FF+[+F-F-F]-[-F+F+F]"),
                    _ => None,
                });
                let lean = tufts.range(-14.0, 14.0);
                let segs = turtle(&s, 20.0 + tufts.range(-4.0, 4.0), -90.0 + lean, 0.2, &mut tufts);
                let h = tufts.range(140.0, 300.0) as i32;
                let w = (h as f32 * 0.55) as i32;
                let cx = 70 + (k * (W as i32 - 140)) / (n - 1) + tufts.range(-12.0, 12.0) as i32;
                let target = Rect::new(cx - w / 2, 650 - h, w, h);
                draw_plant(&mut c, &segs, target, 2.2, None, &mut tufts);
            }
            c.line(50.0, 652.0, W as f32 - 50.0, 652.0, 1.0, Paint::Ink);
            plate(&mut c, "Plate IV", "Deschampsia cespitosa", "Tufted hair-grass", 690);
            "Meadow"
        }
        _ => {
            let s = expand("F", 4, &mut rng, |ch, _| match ch {
                'F' => Some("F[+F]F[-F][F]"),
                _ => None,
            });
            let segs = turtle(&s, 20.0, -90.0, 0.18, &mut rng);
            draw_plant(&mut c, &segs, body, 4.5, Some((11.0, 0.5)), &mut rng);
            plate(&mut c, "Plate V", "Salix caprea", "Goat willow", 690);
            "Willow"
        }
    };
    // Keep the plate frame clean around the slot.
    c.fill_rect(slot.rect().grow(6), Paint::Paper);
    Art { canvas: c, slot, title: title.into(), credit: CREDIT.into() }
}

//! The drawing surface: a painter's model over two layers.
//!
//! Every pixel is either *crisp* (forced ink or forced paper — lines, fills, text) or
//! *grey* (a continuous value that is Floyd–Steinberg dithered when the canvas is
//! finished). Painting is sequential and overriding, like a painter: a grey wash over
//! a crisp line replaces it; a crisp line over a wash cuts through it. The dither
//! never diffuses error across crisp pixels, so lines stay sharp next to gradients.

use quire_gfx::Bitmap;
use std::f32::consts::PI;

use crate::noise::Rng;

/// What a drawing operation paints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    /// Solid ink (black).
    Ink,
    /// Solid paper (white).
    Paper,
    /// A grey level, `0.0` = ink … `1.0` = paper, dithered at the end.
    Gray(f32),
}

impl Paint {
    /// The opposite surface, for drawing on top of a fill.
    pub fn inverse(self) -> Paint {
        match self {
            Paint::Ink => Paint::Paper,
            Paint::Paper => Paint::Ink,
            Paint::Gray(v) => Paint::Gray(1.0 - v),
        }
    }
}

const NONE: u8 = 0;
const INK: u8 = 1;
const PAPER: u8 = 2;

/// An axis-aligned integer rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

impl Rect {
    /// A rectangle from its top-left corner and size.
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
    /// A rectangle of the given size centred at a point.
    pub fn centered(cx: i32, cy: i32, w: i32, h: i32) -> Rect {
        Rect { x: cx - w / 2, y: cy - h / 2, w, h }
    }
    /// Grow (negative: shrink) on every side.
    pub fn grow(&self, n: i32) -> Rect {
        Rect { x: self.x - n, y: self.y - n, w: self.w + 2 * n, h: self.h + 2 * n }
    }
    /// Whether the point lies inside.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    /// Horizontal centre.
    pub fn cx(&self) -> i32 {
        self.x + self.w / 2
    }
    /// Vertical centre.
    pub fn cy(&self) -> i32 {
        self.y + self.h / 2
    }
    /// Right edge (exclusive).
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    /// Bottom edge (exclusive).
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

/// The 528 × 792 drawing surface.
#[derive(Clone)]
pub struct Canvas {
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
    gray: Vec<f32>,
    crisp: Vec<u8>,
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas::new()
    }
}

impl Canvas {
    /// A blank paper canvas at panel size.
    pub fn new() -> Canvas {
        Canvas::sized(crate::W, crate::H)
    }

    /// A blank paper canvas of any size.
    pub fn sized(w: u32, h: u32) -> Canvas {
        let n = (w * h) as usize;
        Canvas { w, h, gray: vec![1.0; n], crisp: vec![PAPER; n] }
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            None
        } else {
            Some(y as usize * self.w as usize + x as usize)
        }
    }

    /// Paint one pixel.
    #[inline]
    pub fn put(&mut self, x: i32, y: i32, p: Paint) {
        if let Some(i) = self.idx(x, y) {
            match p {
                Paint::Ink => self.crisp[i] = INK,
                Paint::Paper => self.crisp[i] = PAPER,
                Paint::Gray(v) => {
                    self.crisp[i] = NONE;
                    self.gray[i] = v.clamp(0.0, 1.0);
                }
            }
        }
    }

    /// The current value of a pixel (`0` ink … `1` paper); outside reads as paper.
    pub fn value(&self, x: i32, y: i32) -> f32 {
        match self.idx(x, y) {
            Some(i) => match self.crisp[i] {
                INK => 0.0,
                PAPER => 1.0,
                _ => self.gray[i],
            },
            None => 1.0,
        }
    }

    /// Whether the pixel is currently crisp ink.
    pub fn is_ink(&self, x: i32, y: i32) -> bool {
        self.idx(x, y).is_some_and(|i| self.crisp[i] == INK)
    }

    /// Fill the whole canvas.
    pub fn fill(&mut self, p: Paint) {
        let r = Rect::new(0, 0, self.w as i32, self.h as i32);
        self.fill_rect(r, p);
    }

    /// Fill a rectangle.
    pub fn fill_rect(&mut self, r: Rect, p: Paint) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.put(x, y, p);
            }
        }
    }

    /// Stroke a rectangle's outline with the given thickness, inside the rect.
    pub fn stroke_rect(&mut self, r: Rect, t: i32, p: Paint) {
        self.fill_rect(Rect::new(r.x, r.y, r.w, t), p);
        self.fill_rect(Rect::new(r.x, r.bottom() - t, r.w, t), p);
        self.fill_rect(Rect::new(r.x, r.y, t, r.h), p);
        self.fill_rect(Rect::new(r.right() - t, r.y, t, r.h), p);
    }

    /// Run a shader over a rectangle: `f(x, y)` returns what to paint, or `None` to
    /// leave the pixel alone.
    pub fn shade<F: FnMut(i32, i32) -> Option<Paint>>(&mut self, r: Rect, mut f: F) {
        let r = self.clip(r);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                if let Some(p) = f(x, y) {
                    self.put(x, y, p);
                }
            }
        }
    }

    /// Run a shader over the whole canvas.
    pub fn shade_all<F: FnMut(i32, i32) -> Option<Paint>>(&mut self, f: F) {
        let r = Rect::new(0, 0, self.w as i32, self.h as i32);
        self.shade(r, f);
    }

    /// Blend every pixel of a region towards paper by `k(x, y)` in `[0, 1]` (fog, haze).
    /// Crisp pixels become grey when partially blended.
    pub fn fade<F: FnMut(i32, i32) -> f32>(&mut self, r: Rect, mut k: F) {
        let r = self.clip(r);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let k = k(x, y).clamp(0.0, 1.0);
                if k <= 0.0 {
                    continue;
                }
                let v = self.value(x, y);
                let nv = v + (1.0 - v) * k;
                if nv >= 0.999 {
                    self.put(x, y, Paint::Paper);
                } else if nv <= 0.001 {
                    self.put(x, y, Paint::Ink);
                } else {
                    self.put(x, y, Paint::Gray(nv));
                }
            }
        }
    }

    fn clip(&self, r: Rect) -> Rect {
        let x0 = r.x.max(0);
        let y0 = r.y.max(0);
        let x1 = r.right().min(self.w as i32);
        let y1 = r.bottom().min(self.h as i32);
        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    /// Filled disc.
    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, p: Paint) {
        let r2 = r * r;
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r2 {
                    self.put(x, y, p);
                }
            }
        }
    }

    /// Ring of outer radius `r` and thickness `t`.
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, t: f32, p: Paint) {
        let (ro2, ri2) = (r * r, (r - t).max(0.0) * (r - t).max(0.0));
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let d2 = dx * dx + dy * dy;
                if d2 <= ro2 && d2 >= ri2 {
                    self.put(x, y, p);
                }
            }
        }
    }

    /// Arc of a circle (angles in radians, screen orientation: `y` down, angles grow
    /// clockwise), stroked with thickness `t`.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, t: f32, p: Paint) {
        let steps = ((a1 - a0).abs() * r.max(1.0) * 1.5).ceil().max(2.0) as usize;
        let mut prev: Option<(f32, f32)> = None;
        for i in 0..=steps {
            let a = a0 + (a1 - a0) * i as f32 / steps as f32;
            let pt = (cx + r * a.cos(), cy + r * a.sin());
            if let Some(q) = prev {
                self.line(q.0, q.1, pt.0, pt.1, t, p);
            }
            prev = Some(pt);
        }
    }

    /// Straight line with thickness `t` (1 px lines use Bresenham; thicker lines stamp
    /// discs so the joins are round).
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, p: Paint) {
        if t <= 1.2 {
            self.line1(x0.round() as i32, y0.round() as i32, x1.round() as i32, y1.round() as i32, p);
            return;
        }
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt();
        let steps = (len / 0.6).ceil().max(1.0) as usize;
        for i in 0..=steps {
            let f = i as f32 / steps as f32;
            self.disc(x0 + dx * f, y0 + dy * f, t / 2.0, p);
        }
    }

    fn line1(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, p: Paint) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.put(x0, y0, p);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    /// Connected line segments.
    pub fn polyline(&mut self, pts: &[(f32, f32)], t: f32, p: Paint) {
        for w in pts.windows(2) {
            self.line(w[0].0, w[0].1, w[1].0, w[1].1, t, p);
        }
    }

    /// Fill a polygon (even-odd rule) with a paint.
    pub fn polygon(&mut self, pts: &[(f32, f32)], p: Paint) {
        self.polygon_with(pts, |_, _| p);
    }

    /// Fill a polygon (even-odd rule) with a per-pixel shader.
    pub fn polygon_with<F: FnMut(i32, i32) -> Paint>(&mut self, pts: &[(f32, f32)], mut f: F) {
        if pts.len() < 3 {
            return;
        }
        let y_min = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let y_max = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(self.h as f32 - 1.0) as i32;
        let mut xs: Vec<f32> = Vec::new();
        for y in y_min..=y_max {
            let sy = y as f32 + 0.5;
            xs.clear();
            let n = pts.len();
            for i in 0..n {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                if (a.1 <= sy && b.1 > sy) || (b.1 <= sy && a.1 > sy) {
                    let t = (sy - a.1) / (b.1 - a.1);
                    xs.push(a.0 + t * (b.0 - a.0));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for pair in xs.chunks(2) {
                if pair.len() < 2 {
                    break;
                }
                let x0 = pair[0].round().max(0.0) as i32;
                let x1 = pair[1].round().min(self.w as f32) as i32;
                for x in x0..x1 {
                    let paint = f(x, y);
                    self.put(x, y, paint);
                }
            }
        }
    }

    /// Straight hatching over a region: parallel lines at `angle` (radians), `spacing`
    /// pixels apart, `t` pixels thick. `inside(x, y)` selects the region.
    #[allow(clippy::too_many_arguments)]
    pub fn hatch<F: FnMut(i32, i32) -> bool>(&mut self, r: Rect, angle: f32, spacing: f32, t: f32, phase: f32, p: Paint, mut inside: F) {
        let (c, s) = (angle.cos(), angle.sin());
        let r = self.clip(r);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let d = x as f32 * c + y as f32 * s + phase;
                if d.rem_euclid(spacing) < t && inside(x, y) {
                    self.put(x, y, p);
                }
            }
        }
    }

    /// Random stipple: each pixel of the region gets a dot with probability
    /// `density(x, y)` (`0` none … `1` solid).
    pub fn stipple<F: FnMut(i32, i32) -> f32>(&mut self, r: Rect, rng: &mut Rng, p: Paint, mut density: F) {
        let r = self.clip(r);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let d = density(x, y);
                if d > 0.0 && rng.f32() < d {
                    self.put(x, y, p);
                }
            }
        }
    }

    /// Copy a bitmap's set bits onto the canvas as `p`.
    pub fn blit_bits(&mut self, bm: &Bitmap, ox: i32, oy: i32, p: Paint) {
        for y in 0..bm.h {
            for x in 0..bm.w {
                if bm.get(x, y) {
                    self.put(ox + x as i32, oy + y as i32, p);
                }
            }
        }
    }

    /// Stamp another canvas's ink pixels onto this one, inverting whatever is under
    /// them: ink on paper, paper on ink. Silhouettes drawn this way stay readable
    /// across a light sky and a dark sea.
    pub fn stamp_invert(&mut self, mask: &Canvas) {
        for y in 0..self.h.min(mask.h) as i32 {
            for x in 0..self.w.min(mask.w) as i32 {
                if mask.is_ink(x, y) {
                    let p = if self.value(x, y) < 0.5 { Paint::Paper } else { Paint::Ink };
                    self.put(x, y, p);
                }
            }
        }
    }

    /// A small four-point star (for star fields and sparkles).
    pub fn sparkle(&mut self, cx: i32, cy: i32, r: i32, p: Paint) {
        for d in -r..=r {
            let arm = r - d.abs();
            for e in -(arm / 3)..=(arm / 3) {
                self.put(cx + d, cy + e, p);
                self.put(cx + e, cy + d, p);
            }
        }
    }

    /// Dither the canvas to a 1-bit bitmap (serpentine Floyd–Steinberg; crisp pixels
    /// are copied verbatim and absorb no error).
    pub fn finish(&self) -> Bitmap {
        let (w, h) = (self.w as usize, self.h as usize);
        let mut out = Bitmap::new(self.w, self.h);
        let mut err_cur = vec![0f32; w + 2];
        let mut err_next = vec![0f32; w + 2];
        for y in 0..h {
            err_next.fill(0.0);
            let ltr = y % 2 == 0;
            for i in 0..w {
                let x = if ltr { i } else { w - 1 - i };
                let idx = y * w + x;
                let (ink, e) = match self.crisp[idx] {
                    INK => (true, 0.0),
                    PAPER => (false, 0.0),
                    _ => {
                        let v = self.gray[idx] + err_cur[x + 1];
                        if v < 0.5 {
                            (true, v)
                        } else {
                            (false, v - 1.0)
                        }
                    }
                };
                if ink {
                    out.set(x as u32, y as u32, true);
                }
                let e = e.clamp(-1.0, 1.0);
                let (fwd, back) = if ltr { (x + 2, x) } else { (x, x + 2) };
                err_cur[fwd] += e * 7.0 / 16.0;
                err_next[back] += e * 3.0 / 16.0;
                err_next[x + 1] += e * 5.0 / 16.0;
                err_next[fwd] += e / 16.0;
            }
            std::mem::swap(&mut err_cur, &mut err_next);
        }
        out
    }
}

/// Points of a regular polygon / circle sampled `n` times, starting at `a0` radians.
pub fn circle_points(cx: f32, cy: f32, r: f32, n: usize, a0: f32) -> Vec<(f32, f32)> {
    (0..n)
        .map(|i| {
            let a = a0 + 2.0 * PI * i as f32 / n as f32;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// Fraction of set bits in a bitmap.
pub fn ink_density(bm: &Bitmap) -> f32 {
    let total = (bm.w * bm.h) as f32;
    let set: u32 = bm.bits.iter().map(|b| b.count_ones()).sum();
    set as f32 / total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crisp_pixels_survive_dithering_next_to_grey() {
        let mut c = Canvas::sized(64, 64);
        c.fill(Paint::Gray(0.5));
        c.fill_rect(Rect::new(8, 8, 16, 16), Paint::Paper);
        c.fill_rect(Rect::new(32, 8, 16, 16), Paint::Ink);
        let bm = c.finish();
        for y in 8..24 {
            for x in 8..24 {
                assert!(!bm.get(x, y));
            }
            for x in 32..48 {
                assert!(bm.get(x, y));
            }
        }
        let d = ink_density(&bm);
        assert!((0.4..0.6).contains(&d), "{d}");
    }

    #[test]
    fn polygon_fills_a_square() {
        let mut c = Canvas::sized(32, 32);
        c.polygon(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)], Paint::Ink);
        assert!(c.is_ink(10, 10));
        assert!(!c.is_ink(25, 25));
    }
}

//! Maze / Labyrinth: perfect mazes on square, polar and hexagonal grids grown by
//! recursive backtracking around an island that holds the clock, and the classical
//! seven-circuit labyrinth drawn from its path's distance field.

use std::collections::{HashMap, HashSet, VecDeque};
use std::f32::consts::PI;

use crate::canvas::{Canvas, Paint, Rect};
use crate::noise::Rng;
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::{seed_for, Art, H, W};

/// The pack.
pub const PACK: Pack = Pack {
    id: "maze",
    name: "Labyrinths",
    description: "Perfect mazes on square, polar and hex grids, and the classical seven-circuit labyrinth",
    count: PER_PACK,
    render,
};

// ---------------------------------------------------------------- square grid

struct Grid {
    cols: usize,
    rows: usize,
    blocked: Vec<bool>,
    open_right: Vec<bool>,
    open_down: Vec<bool>,
}

impl Grid {
    fn new(cols: usize, rows: usize, blocked: impl Fn(usize, usize) -> bool) -> Grid {
        let n = cols * rows;
        let mut b = vec![false; n];
        for y in 0..rows {
            for x in 0..cols {
                b[y * cols + x] = blocked(x, y);
            }
        }
        Grid { cols, rows, blocked: b, open_right: vec![false; n], open_down: vec![false; n] }
    }
    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.cols + x
    }
    fn neighbours(&self, x: usize, y: usize) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        if x > 0 {
            v.push((x - 1, y));
        }
        if x + 1 < self.cols {
            v.push((x + 1, y));
        }
        if y > 0 {
            v.push((x, y - 1));
        }
        if y + 1 < self.rows {
            v.push((x, y + 1));
        }
        v.retain(|&(nx, ny)| !self.blocked[self.idx(nx, ny)]);
        v
    }
    fn open(&mut self, a: (usize, usize), b: (usize, usize)) {
        if a.1 == b.1 {
            let x = a.0.min(b.0);
            let i = self.idx(x, a.1);
            self.open_right[i] = true;
        } else {
            let y = a.1.min(b.1);
            let i = self.idx(a.0, y);
            self.open_down[i] = true;
        }
    }
    fn is_open(&self, a: (usize, usize), b: (usize, usize)) -> bool {
        if a.1 == b.1 {
            self.open_right[self.idx(a.0.min(b.0), a.1)]
        } else {
            self.open_down[self.idx(a.0, a.1.min(b.1))]
        }
    }
    fn carve(&mut self, rng: &mut Rng, start: (usize, usize)) {
        let mut visited = vec![false; self.cols * self.rows];
        let mut stack = vec![start];
        visited[self.idx(start.0, start.1)] = true;
        while let Some(&cur) = stack.last() {
            let mut opts: Vec<(usize, usize)> =
                self.neighbours(cur.0, cur.1).into_iter().filter(|&(x, y)| !visited[self.idx(x, y)]).collect();
            if opts.is_empty() {
                stack.pop();
                continue;
            }
            let next = opts.swap_remove(rng.below(opts.len() as u32) as usize);
            self.open(cur, next);
            visited[self.idx(next.0, next.1)] = true;
            stack.push(next);
        }
    }
    fn solve(&self, from: (usize, usize), to: (usize, usize)) -> Vec<(usize, usize)> {
        let mut prev: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
        let mut q = VecDeque::from([from]);
        let mut seen = HashSet::from([from]);
        while let Some(cur) = q.pop_front() {
            if cur == to {
                break;
            }
            for n in self.neighbours(cur.0, cur.1) {
                if self.is_open(cur, n) && seen.insert(n) {
                    prev.insert(n, cur);
                    q.push_back(n);
                }
            }
        }
        let mut path = vec![to];
        let mut cur = to;
        while let Some(&p) = prev.get(&cur) {
            path.push(p);
            cur = p;
        }
        path.reverse();
        path
    }
}

fn square_maze(c: &mut Canvas, rng: &mut Rng, cell: i32, wall: i32, slot: &ClockSlot, solution: bool) {
    let cols = ((W as i32 - 2 * 24) / cell) as usize;
    let rows = ((H as i32 - 2 * 24) / cell) as usize;
    let ox = (W as i32 - cols as i32 * cell) / 2;
    let oy = (H as i32 - rows as i32 * cell) / 2;
    let island = slot.rect().grow(cell / 2 + 6);
    let blocked = |x: usize, y: usize| {
        let r = Rect::new(ox + x as i32 * cell, oy + y as i32 * cell, cell, cell);
        r.right() > island.x && r.x < island.right() && r.bottom() > island.y && r.y < island.bottom()
    };
    let mut g = Grid::new(cols, rows, blocked);
    g.carve(rng, (0, 0));
    let exit = (cols - 1, rows - 1);
    let half = wall / 2;
    let p = Paint::Ink;
    for y in 0..rows {
        for x in 0..cols {
            let i = g.idx(x, y);
            let (px, py) = (ox + x as i32 * cell, oy + y as i32 * cell);
            let b = g.blocked[i];
            // Right wall.
            let rb = x + 1 < cols && g.blocked[g.idx(x + 1, y)];
            if x + 1 < cols && !g.open_right[i] && !(b && rb) {
                c.fill_rect(Rect::new(px + cell - half, py - half, wall, cell + wall), p);
            }
            let db = y + 1 < rows && g.blocked[g.idx(x, y + 1)];
            if y + 1 < rows && !g.open_down[i] && !(b && db) {
                c.fill_rect(Rect::new(px - half, py + cell - half, cell + wall, wall), p);
            }
        }
    }
    // Outer walls with the entrance (top of cell 0) and the exit (bottom of the last).
    let (x0, y0, x1, y1) = (ox - half, oy - half, ox + cols as i32 * cell - half, oy + rows as i32 * cell - half);
    c.fill_rect(Rect::new(x0 + cell, y0, x1 - x0 - cell + wall, wall), p);
    c.fill_rect(Rect::new(x0, y1, x1 - x0 - cell, wall), p);
    c.fill_rect(Rect::new(x0, y0, wall, y1 - y0 + wall), p);
    c.fill_rect(Rect::new(x1, y0, wall, y1 - y0 + wall), p);
    if solution {
        let path = g.solve((0, 0), exit);
        let centre = |(x, y): (usize, usize)| (ox as f32 + (x as f32 + 0.5) * cell as f32, oy as f32 + (y as f32 + 0.5) * cell as f32);
        let mut pts: Vec<(f32, f32)> = vec![(centre((0, 0)).0, y0 as f32 - 6.0)];
        pts.extend(path.iter().map(|&p| centre(p)));
        pts.push((centre(exit).0, y1 as f32 + wall as f32 + 6.0));
        dotted(c, &pts, 4.0);
    }
    // The island: a plaque with a double rule.
    c.fill_rect(island, Paint::Paper);
    c.stroke_rect(slot.rect().grow(8), 2, p);
    c.stroke_rect(slot.rect().grow(3), 1, p);
}

fn dotted(c: &mut Canvas, pts: &[(f32, f32)], pitch: f32) {
    let mut carry = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let mut d = carry;
        while d <= len {
            let t = d / len.max(1e-3);
            c.disc(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, 1.1, Paint::Ink);
            d += pitch;
        }
        carry = d - len;
    }
}

// ---------------------------------------------------------------- polar grid

fn theta_maze(c: &mut Canvas, rng: &mut Rng, cx: f32, cy: f32, r0: f32, ring_h: f32, rings: usize) {
    // Cells per ring double whenever a cell would get wider than 1.6 × its height.
    let mut counts = vec![24usize];
    for k in 1..rings {
        let r = r0 + k as f32 * ring_h;
        let prev = counts[k - 1];
        let width = 2.0 * PI * r / prev as f32;
        counts.push(if width > 1.6 * ring_h { prev * 2 } else { prev });
    }
    let id = |k: usize, j: usize| (k, j);
    let mut open: HashSet<((usize, usize), (usize, usize))> = HashSet::new();
    let mark = |a: (usize, usize), b: (usize, usize), open: &mut HashSet<_>| {
        open.insert((a, b));
        open.insert((b, a));
    };
    let neighbours = |k: usize, j: usize| -> Vec<(usize, usize)> {
        let n = counts[k];
        let mut v = vec![id(k, (j + 1) % n), id(k, (j + n - 1) % n)];
        if k > 0 {
            v.push(id(k - 1, j * counts[k - 1] / n));
        }
        if k + 1 < rings {
            let ratio = counts[k + 1] / n;
            for m in 0..ratio {
                v.push(id(k + 1, j * ratio + m));
            }
        }
        v
    };
    let mut visited: HashSet<(usize, usize)> = HashSet::new();
    let start = (0usize, rng.below(counts[0] as u32) as usize);
    let mut stack = vec![start];
    visited.insert(start);
    while let Some(&cur) = stack.last() {
        let mut opts: Vec<(usize, usize)> = neighbours(cur.0, cur.1).into_iter().filter(|n| !visited.contains(n)).collect();
        if opts.is_empty() {
            stack.pop();
            continue;
        }
        let next = opts.swap_remove(rng.below(opts.len() as u32) as usize);
        mark(cur, next, &mut open);
        visited.insert(next);
        stack.push(next);
    }
    let t = 3.0;
    for k in 0..rings {
        let n = counts[k];
        let r_in = r0 + k as f32 * ring_h;
        for j in 0..n {
            let a0 = 2.0 * PI * j as f32 / n as f32;
            let a1 = 2.0 * PI * (j + 1) as f32 / n as f32;
            // Inner wall (towards the centre): open only if carved to the parent, or
            // it is the entrance to the centre chamber.
            let inner_open = if k == 0 { (k, j) == start } else { open.contains(&((k, j), (k - 1, j * counts[k - 1] / n))) };
            if !inner_open {
                c.arc(cx, cy, r_in, a0, a1, t, Paint::Ink);
            }
            // Clockwise wall.
            if !open.contains(&((k, j), (k, (j + 1) % n))) {
                c.line(
                    cx + r_in * a1.cos(),
                    cy + r_in * a1.sin(),
                    cx + (r_in + ring_h) * a1.cos(),
                    cy + (r_in + ring_h) * a1.sin(),
                    t,
                    Paint::Ink,
                );
            }
        }
    }
    // Outer wall with one exit.
    let r_out = r0 + rings as f32 * ring_h;
    let n = counts[rings - 1];
    let exit = rng.below(n as u32) as usize;
    for j in 0..n {
        if j == exit {
            continue;
        }
        let a0 = 2.0 * PI * j as f32 / n as f32;
        let a1 = 2.0 * PI * (j + 1) as f32 / n as f32;
        c.arc(cx, cy, r_out, a0, a1, t, Paint::Ink);
    }
}

// ------------------------------------------------------------ hexagonal grid

fn hex_maze(c: &mut Canvas, rng: &mut Rng, size: f32, island: Rect) {
    let (ox, oy) = (W as f32 / 2.0, H as f32 / 2.0);
    let centre = |q: i32, r: i32| (ox + 1.5 * size * q as f32, oy + 3f32.sqrt() * size * (r as f32 + q as f32 / 2.0));
    let dirs: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];
    let inside = |q: i32, r: i32| {
        let (x, y) = centre(q, r);
        x > 30.0 + size
            && x < W as f32 - 30.0 - size
            && y > 30.0 + size
            && y < H as f32 - 30.0 - size
            && !island.grow(size as i32).contains(x as i32, y as i32)
    };
    let mut cells: Vec<(i32, i32)> = Vec::new();
    for q in -20..=20 {
        for r in -30..=30 {
            if inside(q, r) {
                cells.push((q, r));
            }
        }
    }
    let set: HashSet<(i32, i32)> = cells.iter().copied().collect();
    let mut open: HashSet<((i32, i32), usize)> = HashSet::new();
    let mut visited: HashSet<(i32, i32)> = HashSet::new();
    let start = cells[0];
    let mut stack = vec![start];
    visited.insert(start);
    while let Some(&cur) = stack.last() {
        let mut opts: Vec<(usize, (i32, i32))> = dirs
            .iter()
            .enumerate()
            .map(|(i, d)| (i, (cur.0 + d.0, cur.1 + d.1)))
            .filter(|(_, n)| set.contains(n) && !visited.contains(n))
            .collect();
        if opts.is_empty() {
            stack.pop();
            continue;
        }
        let (i, next) = opts.swap_remove(rng.below(opts.len() as u32) as usize);
        open.insert((cur, i));
        open.insert((next, (i + 3) % 6));
        visited.insert(next);
        stack.push(next);
    }
    // Entrance and exit on the outer boundary: the first and last cells' outer edges.
    let last = *cells.last().unwrap();
    for &(q, r) in &cells {
        let (x, y) = centre(q, r);
        for (i, d) in dirs.iter().enumerate() {
            if open.contains(&((q, r), i)) {
                continue;
            }
            let neighbour = (q + d.0, r + d.1);
            let boundary = !set.contains(&neighbour);
            if boundary && (((q, r) == start && i == 3) || ((q, r) == last && i == 0)) {
                continue;
            }
            let a0 = (i as f32) * PI / 3.0;
            let a1 = a0 + PI / 3.0;
            c.line(x + size * a0.cos(), y + size * a0.sin(), x + size * a1.cos(), y + size * a1.sin(), 3.0, Paint::Ink);
        }
    }
}

// ------------------------------------------------------- classical labyrinth

enum Curve {
    Arc { c: (f32, f32), r: f32, a0: f32, a1: f32 },
    Seg { a: (f32, f32), b: (f32, f32) },
    Disc { c: (f32, f32), r: f32 },
}

impl Curve {
    fn dist(&self, x: f32, y: f32) -> f32 {
        match *self {
            Curve::Disc { c, r } => (((x - c.0).powi(2) + (y - c.1).powi(2)).sqrt() - r).max(0.0),
            Curve::Seg { a, b } => {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                ((x - a.0 - t * dx).powi(2) + (y - a.1 - t * dy).powi(2)).sqrt()
            }
            Curve::Arc { c, r, a0, a1 } => {
                let (dx, dy) = (x - c.0, y - c.1);
                let ang = dy.atan2(dx);
                let span = a1 - a0;
                let rel = (ang - a0).rem_euclid(2.0 * PI);
                if rel <= span {
                    ((dx * dx + dy * dy).sqrt() - r).abs()
                } else {
                    let p0 = (c.0 + r * a0.cos(), c.1 + r * a0.sin());
                    let p1 = (c.0 + r * a1.cos(), c.1 + r * a1.sin());
                    ((x - p0.0).powi(2) + (y - p0.1).powi(2)).sqrt().min(((x - p1.0).powi(2) + (y - p1.1).powi(2)).sqrt())
                }
            }
        }
    }
}

fn classical_labyrinth(c: &mut Canvas, cx: f32, cy: f32, r_centre: f32, pitch: f32, hw: f32) {
    // Circuits 1 (outermost) … 7, then the centre chamber "C".
    let radius = |level: usize| if level == 8 { r_centre } else { r_centre + (8 - level) as f32 * pitch };
    let g = 1.5 * pitch + pitch; // half-width of the axis wedge
    let mut curves: Vec<Curve> = Vec::new();
    let end_y = |level: usize| cy + (radius(level).powi(2) - g * g).sqrt();
    for level in 1..=7 {
        let r = radius(level);
        let d = (g / r).asin();
        let a0 = PI / 2.0 + d;
        let a1 = if level == 3 { 2.0 * PI + PI / 2.0 } else { 2.0 * PI + PI / 2.0 - d };
        curves.push(Curve::Arc { c: (cx, cy), r, a0, a1 });
    }
    // U-turns: (from, to, right side?)
    for &(a, b, right) in &[(3usize, 2usize, true), (2, 1, false), (1, 4, true), (4, 7, false), (7, 6, true), (6, 5, false), (5, 8, true)] {
        let (ya, yb) = (end_y(a), end_y(b));
        let x = if right { cx + g } else { cx - g };
        let centre = (x, (ya + yb) / 2.0);
        let r = (ya - yb).abs() / 2.0;
        let (a0, a1) = if right { (PI / 2.0, 3.0 * PI / 2.0) } else { (-PI / 2.0, PI / 2.0) };
        curves.push(Curve::Arc { c: centre, r, a0, a1 });
    }
    // Entrance channel along the axis into circuit 3, and the centre chamber.
    curves.push(Curve::Seg { a: (cx, cy + radius(3)), b: (cx, cy + radius(1) + pitch) });
    curves.push(Curve::Disc { c: (cx, cy), r: r_centre });
    let t = pitch - 2.0 * hw;
    let bound = radius(1) + pitch + hw + t;
    c.shade(Rect::new((cx - bound) as i32, (cy - bound) as i32, (2.0 * bound) as i32 + 1, (2.0 * bound) as i32 + 1), |x, y| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let d = curves.iter().map(|k| k.dist(fx, fy)).fold(f32::MAX, f32::min);
        if d >= hw && d < hw + t {
            Some(Paint::Ink)
        } else {
            None
        }
    });
}

fn render(i: usize) -> Art {
    let seed = seed_for(PACK.id, i);
    let mut rng = Rng::new(seed);
    let mut c = Canvas::new();
    let (slot, title) = match i {
        0 => {
            let slot = ClockSlot::centered(W as i32 / 2, H as i32 / 2, ClockStyle::Hero, Surface::Paper);
            square_maze(&mut c, &mut rng, 16, 3, &slot, true);
            (slot, "Square, solved")
        }
        1 => {
            let slot = ClockSlot::centered(W as i32 / 2, H as i32 / 2, ClockStyle::Poster, Surface::Paper);
            square_maze(&mut c, &mut rng, 11, 2, &slot, false);
            (slot, "Square, fine")
        }
        2 => {
            let slot = ClockSlot::centered(W as i32 / 2, 396, ClockStyle::Poster, Surface::Paper);
            theta_maze(&mut c, &mut rng, W as f32 / 2.0, 396.0, 104.0, 22.0, 7);
            (slot, "Polar")
        }
        3 => {
            let slot = ClockSlot::centered(W as i32 / 2, 712, ClockStyle::Poster, Surface::Paper);
            classical_labyrinth(&mut c, W as f32 / 2.0, 342.0, 44.0, 26.0, 10.5);
            let r = slot.rect().grow(8);
            c.stroke_rect(r, 1, Paint::Ink);
            (slot, "Classical")
        }
        _ => {
            let slot = ClockSlot::centered(W as i32 / 2, H as i32 / 2, ClockStyle::Hero, Surface::Paper);
            let island = slot.rect().grow(12);
            hex_maze(&mut c, &mut rng, 13.0, island);
            c.fill_rect(island, Paint::Paper);
            c.stroke_rect(island, 2, Paint::Ink);
            c.stroke_rect(island.grow(-5), 1, Paint::Ink);
            (slot, "Hex")
        }
    };
    Art { canvas: c, slot, title: title.into(), credit: CREDIT.into() }
}

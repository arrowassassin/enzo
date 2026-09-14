//! The icon set: 24 px grid, 2 px strokes, drawn procedurally so they invert cleanly.

use quire_gfx::{Frame, Ink, Rect};

/// The icons of the brief.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// Battery at a level 0–4 (quarters).
    Battery(u8),
    /// Charging bolt.
    Charging,
    /// Wi-Fi.
    Wifi,
    /// Wi-Fi off.
    WifiOff,
    /// Bookmark outline.
    Bookmark,
    /// Bookmark filled.
    BookmarkSet,
    /// Clock.
    Clock,
    /// Moon.
    Moon,
    /// Sun.
    Sun,
    /// Check mark.
    Check,
    /// Close cross.
    Close,
    /// Chevron left.
    ChevronLeft,
    /// Chevron right.
    ChevronRight,
    /// Chevron up.
    ChevronUp,
    /// Chevron down.
    ChevronDown,
    /// Arrow left.
    ArrowLeft,
    /// Arrow right.
    ArrowRight,
    /// Arrow up.
    ArrowUp,
    /// Arrow down.
    ArrowDown,
    /// Lock.
    Lock,
    /// Warning triangle.
    Warning,
}

fn line(f: &mut Frame, x0: i32, y0: i32, x1: i32, y1: i32, ink: Ink) {
    // Bresenham with a 2 px brush.
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        f.fill_rect(Rect::new(x, y, 2, 2), ink);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Draw an icon with its 24 px box at (x, y).
pub fn draw(f: &mut Frame, icon: Icon, x: i32, y: i32, ink: Ink) {
    let r = |f: &mut Frame, rx: i32, ry: i32, w: u32, h: u32| f.fill_rect(Rect::new(x + rx, y + ry, w, h), ink);
    match icon {
        Icon::Battery(level) => {
            f.stroke_rect(Rect::new(x + 1, y + 6, 19, 12), 2, ink);
            r(f, 20, 9, 3, 6);
            let cells = level.min(4) as i32;
            for i in 0..cells {
                r(f, 4 + i * 4, 9, 3, 6);
            }
        }
        Icon::Charging => {
            line(f, x + 13, y + 2, x + 7, y + 13, ink);
            line(f, x + 7, y + 13, x + 15, y + 13, ink);
            line(f, x + 15, y + 13, x + 9, y + 22, ink);
        }
        Icon::Wifi | Icon::WifiOff => {
            for (i, (w, yy)) in [(20i32, 6i32), (14, 11), (8, 16)].iter().enumerate() {
                let x0 = x + 12 - w / 2;
                f.fill_rect(Rect::new(x0, y + yy, *w as u32, 2), ink);
                f.fill_rect(Rect::new(x0 - 2, y + yy + 2, 2, 2), ink);
                f.fill_rect(Rect::new(x0 + w, y + yy + 2, 2, 2), ink);
                let _ = i;
            }
            r(f, 11, 20, 3, 3);
            if icon == Icon::WifiOff {
                line(f, x + 3, y + 3, x + 21, y + 21, ink);
            }
        }
        Icon::Bookmark | Icon::BookmarkSet => {
            f.stroke_rect(Rect::new(x + 5, y + 2, 14, 20), 2, ink);
            line(f, x + 5, y + 21, x + 12, y + 15, ink);
            line(f, x + 12, y + 15, x + 18, y + 21, ink);
            // Open the bottom edge into a notch.
            f.fill_rect(Rect::new(x + 7, y + 20, 10, 2), if ink == Ink::Black { Ink::White } else { Ink::Black });
            if icon == Icon::BookmarkSet {
                f.fill_rect(Rect::new(x + 7, y + 4, 10, 12), ink);
                for i in 0..5 {
                    f.fill_rect(Rect::new(x + 7 + i, y + 16 + i, (10 - 2 * i) as u32, 1), ink);
                }
            }
        }
        Icon::Clock => {
            circle(f, x + 12, y + 12, 10, ink);
            line(f, x + 12, y + 6, x + 12, y + 12, ink);
            line(f, x + 12, y + 12, x + 16, y + 15, ink);
        }
        Icon::Moon => {
            circle(f, x + 12, y + 12, 10, ink);
            let paper = if ink == Ink::Black { Ink::White } else { Ink::Black };
            fill_circle(f, x + 16, y + 9, 8, paper);
        }
        Icon::Sun => {
            fill_circle(f, x + 12, y + 12, 5, ink);
            for (dx, dy) in [(0, -9), (0, 9), (-9, 0), (9, 0), (-6, -6), (6, 6), (-6, 6), (6, -6)] {
                r(f, 11 + dx, 11 + dy, 2, 2);
            }
        }
        Icon::Check => {
            line(f, x + 3, y + 12, x + 9, y + 18, ink);
            line(f, x + 9, y + 18, x + 20, y + 5, ink);
        }
        Icon::Close => {
            line(f, x + 4, y + 4, x + 19, y + 19, ink);
            line(f, x + 19, y + 4, x + 4, y + 19, ink);
        }
        Icon::ChevronLeft => {
            line(f, x + 15, y + 4, x + 7, y + 12, ink);
            line(f, x + 7, y + 12, x + 15, y + 20, ink);
        }
        Icon::ChevronRight => {
            line(f, x + 8, y + 4, x + 16, y + 12, ink);
            line(f, x + 16, y + 12, x + 8, y + 20, ink);
        }
        Icon::ChevronUp => {
            line(f, x + 4, y + 15, x + 12, y + 7, ink);
            line(f, x + 12, y + 7, x + 20, y + 15, ink);
        }
        Icon::ChevronDown => {
            line(f, x + 4, y + 8, x + 12, y + 16, ink);
            line(f, x + 12, y + 16, x + 20, y + 8, ink);
        }
        Icon::ArrowLeft => {
            line(f, x + 3, y + 11, x + 20, y + 11, ink);
            line(f, x + 3, y + 11, x + 10, y + 4, ink);
            line(f, x + 3, y + 11, x + 10, y + 18, ink);
        }
        Icon::ArrowRight => {
            line(f, x + 2, y + 11, x + 19, y + 11, ink);
            line(f, x + 19, y + 11, x + 12, y + 4, ink);
            line(f, x + 19, y + 11, x + 12, y + 18, ink);
        }
        Icon::ArrowUp => {
            line(f, x + 11, y + 3, x + 11, y + 20, ink);
            line(f, x + 11, y + 3, x + 4, y + 10, ink);
            line(f, x + 11, y + 3, x + 18, y + 10, ink);
        }
        Icon::ArrowDown => {
            line(f, x + 11, y + 2, x + 11, y + 19, ink);
            line(f, x + 11, y + 19, x + 4, y + 12, ink);
            line(f, x + 11, y + 19, x + 18, y + 12, ink);
        }
        Icon::Lock => {
            f.stroke_rect(Rect::new(x + 4, y + 11, 16, 11), 2, ink);
            line(f, x + 7, y + 11, x + 7, y + 6, ink);
            line(f, x + 7, y + 6, x + 15, y + 6, ink);
            line(f, x + 15, y + 6, x + 15, y + 11, ink);
            r(f, 11, 15, 2, 4);
        }
        Icon::Warning => {
            line(f, x + 12, y + 2, x + 2, y + 21, ink);
            line(f, x + 2, y + 21, x + 21, y + 21, ink);
            line(f, x + 21, y + 21, x + 12, y + 2, ink);
            r(f, 11, 9, 2, 6);
            r(f, 11, 17, 2, 2);
        }
    }
}

fn circle(f: &mut Frame, cx: i32, cy: i32, r: i32, ink: Ink) {
    let (mut x, mut y, mut d) = (r, 0, 1 - r);
    while x >= y {
        for (px, py) in [(x, y), (y, x), (-y, x), (-x, y), (-x, -y), (-y, -x), (y, -x), (x, -y)] {
            f.fill_rect(Rect::new(cx + px - 1, cy + py - 1, 2, 2), ink);
        }
        y += 1;
        if d < 0 {
            d += 2 * y + 1;
        } else {
            x -= 1;
            d += 2 * (y - x) + 1;
        }
    }
}

fn fill_circle(f: &mut Frame, cx: i32, cy: i32, r: i32, ink: Ink) {
    for dy in -r..=r {
        let half = ((r * r - dy * dy) as f32).max(0.0);
        // Integer square root.
        let mut w = 0i32;
        while (w + 1) * (w + 1) <= half as i32 {
            w += 1;
        }
        f.fill_rect(Rect::new(cx - w, cy + dy, (2 * w + 1) as u32, 1), ink);
    }
}

/// Battery icon level from a percentage.
pub fn battery_level(percent: u8) -> u8 {
    match percent {
        0..=10 => 0,
        11..=37 => 1,
        38..=62 => 2,
        63..=87 => 3,
        _ => 4,
    }
}

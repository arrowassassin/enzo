//! Constellations: star-atlas plates. Real star positions (J2000, bright stars only)
//! projected stereographically, the classical figure lines, a faint dithered band of
//! the Milky Way, a dotted graticule and the name set in small caps.

use std::f32::consts::PI;

use crate::canvas::{Canvas, Paint, Rect};
use crate::noise::{fbm2, Rng};
use crate::packs::{Pack, CREDIT, PER_PACK};
use crate::slot::{ClockSlot, ClockStyle, Surface};
use crate::text;
use crate::{seed_for, Art, H, W};

/// The pack.
pub const PACK: Pack = Pack {
    id: "constellations",
    name: "Constellations",
    description: "Star-atlas plates of Orion, the Plough, Cassiopeia, Cygnus and Scorpius",
    count: PER_PACK,
    render,
};

/// A star: right ascension (hours), declination (degrees), visual magnitude.
type Star = (f32, f32, f32);

struct Figure {
    name: &'static str,
    epithet: &'static str,
    stars: &'static [Star],
    lines: &'static [(usize, usize)],
    /// Milky Way band angle (radians) and offset from the centre, or `None`.
    milky_way: Option<(f32, f32)>,
    /// Target box height for the figure in pixels.
    height: f32,
}

const ORION: Figure = Figure {
    name: "Orion",
    epithet: "The Hunter",
    stars: &[
        (5.919, 7.407, 0.5),  // Betelgeuse
        (5.242, -8.202, 0.1), // Rigel
        (5.419, 6.350, 1.6),  // Bellatrix
        (5.533, -0.299, 2.2), // Mintaka
        (5.604, -1.202, 1.7), // Alnilam
        (5.679, -1.943, 1.8), // Alnitak
        (5.796, -9.670, 2.1), // Saiph
        (5.586, 9.934, 3.4),  // Meissa
        (4.830, 6.961, 3.2),  // Pi3 Ori (shield)
        (4.853, 5.605, 3.7),  // Pi4
        (4.904, 2.441, 3.7),  // Pi5
        (4.844, 8.900, 4.4),  // Pi2
        (4.915, 10.151, 4.7), // Pi1
        (5.418, -2.397, 2.8), // Eta Ori
        (5.795, -5.910, 4.6), // Theta Ori (sword)
        (5.679, -5.387, 2.8), // Iota Ori
    ],
    lines: &[
        (0, 2),
        (2, 3),
        (3, 4),
        (4, 5),
        (5, 6),
        (6, 1),
        (1, 3),
        (0, 5),
        (2, 7),
        (7, 0),
        (2, 8),
        (8, 9),
        (9, 10),
        (8, 11),
        (11, 12),
        (4, 15),
        (15, 14),
    ],
    milky_way: Some((1.2, 260.0)),
    height: 400.0,
};

const PLOUGH: Figure = Figure {
    name: "Ursa Major",
    epithet: "The Great Bear",
    stars: &[
        (11.062, 61.751, 1.8), // Dubhe
        (11.031, 56.382, 2.4), // Merak
        (11.897, 53.695, 2.4), // Phecda
        (12.257, 57.033, 3.3), // Megrez
        (12.900, 55.960, 1.8), // Alioth
        (13.399, 54.925, 2.2), // Mizar
        (13.792, 49.313, 1.9), // Alkaid
        (13.420, 54.988, 4.0), // Alcor
        (9.525, 63.062, 3.4),  // Muscida (nose)
        (9.850, 59.039, 3.8),  // Upsilon UMa
        (9.548, 51.677, 3.2),  // Theta UMa
        (8.986, 48.042, 3.1),  // Talitha
        (9.061, 47.157, 3.6),  // Kappa UMa
        (10.285, 42.914, 3.4), // Lambda UMa
        (10.372, 41.499, 3.0), // Tania Australis
        (11.162, 44.498, 3.0), // Psi UMa
        (11.303, 33.094, 3.5), // Alula Borealis
        (11.303, 31.529, 4.3), // Alula Australis
        (11.767, 47.779, 3.7), // Chi UMa
    ],
    lines: &[
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (3, 4),
        (4, 5),
        (5, 6),
        (0, 9),
        (9, 8),
        (8, 10),
        (10, 11),
        (11, 12),
        (2, 18),
        (18, 15),
        (15, 13),
        (13, 14),
        (18, 16),
        (16, 17),
    ],
    milky_way: None,
    height: 380.0,
};

const CASSIOPEIA: Figure = Figure {
    name: "Cassiopeia",
    epithet: "The Queen",
    stars: &[
        (0.153, 59.150, 2.3), // Caph
        (0.675, 56.537, 2.2), // Schedar
        (0.945, 60.717, 2.2), // Navi
        (1.430, 60.235, 2.7), // Ruchbah
        (1.907, 63.670, 3.4), // Segin
        (0.617, 53.897, 3.4), // Zeta Cas
        (0.818, 57.816, 3.7), // Eta Cas
        (1.185, 55.150, 4.2), // Theta Cas
        (1.734, 68.130, 4.6), // Iota Cas
        (0.031, 62.283, 4.0), // Kappa Cas
    ],
    lines: &[(0, 1), (1, 2), (2, 3), (3, 4), (1, 6), (6, 2)],
    milky_way: Some((0.35, 40.0)),
    height: 240.0,
};

const CYGNUS: Figure = Figure {
    name: "Cygnus",
    epithet: "The Swan",
    stars: &[
        (20.690, 45.280, 1.3), // Deneb
        (20.370, 40.257, 2.2), // Sadr
        (20.770, 33.970, 2.5), // Gienah
        (19.749, 45.131, 2.9), // Delta Cyg
        (19.512, 27.960, 3.1), // Albireo
        (19.495, 51.729, 3.8), // Iota Cyg
        (19.285, 53.368, 3.8), // Kappa Cyg
        (21.216, 30.227, 3.2), // Zeta Cyg
        (19.938, 35.083, 3.9), // Eta Cyg
        (20.953, 41.167, 3.7), // Nu Cyg
        (21.246, 38.045, 3.7), // Tau Cyg
    ],
    lines: &[(0, 1), (1, 8), (8, 4), (1, 2), (2, 7), (1, 3), (3, 5), (5, 6), (0, 9), (9, 10)],
    milky_way: Some((1.05, 0.0)),
    height: 400.0,
};

const SCORPIUS: Figure = Figure {
    name: "Scorpius",
    epithet: "The Scorpion",
    stars: &[
        (16.490, -26.432, 1.0), // Antares
        (16.005, -22.622, 2.3), // Dschubba
        (16.091, -19.805, 2.6), // Acrab
        (15.981, -26.114, 2.9), // Pi Sco
        (16.353, -25.593, 2.9), // Sigma Sco
        (16.598, -28.216, 2.8), // Tau Sco
        (16.836, -34.293, 2.3), // Epsilon Sco
        (16.864, -38.048, 3.0), // Mu1 Sco
        (16.909, -42.362, 3.6), // Zeta2 Sco
        (17.203, -43.239, 3.3), // Eta Sco
        (17.622, -42.998, 1.9), // Sargas
        (17.793, -40.127, 3.0), // Iota1 Sco
        (17.708, -39.030, 2.4), // Kappa Sco
        (17.513, -37.296, 2.7), // Lesath
        (17.560, -37.104, 1.6), // Shaula
        (15.898, -26.328, 4.0), // Rho Sco
        (15.949, -29.214, 3.9), // approximate claw
    ],
    lines: &[
        (2, 1),
        (1, 3),
        (3, 15),
        (15, 16),
        (1, 4),
        (4, 0),
        (0, 5),
        (5, 6),
        (6, 7),
        (7, 8),
        (8, 9),
        (9, 10),
        (10, 11),
        (11, 12),
        (12, 13),
        (13, 14),
    ],
    milky_way: Some((0.55, 190.0)),
    height: 440.0,
};

const FIGURES: [&Figure; 5] = [&ORION, &PLOUGH, &CASSIOPEIA, &CYGNUS, &SCORPIUS];

/// Stereographic projection about `(ra0, dec0)`; returns (x, y) with x growing east.
fn project(ra: f32, dec: f32, ra0: f32, dec0: f32) -> (f32, f32) {
    let (a, d) = (ra * 15.0_f32.to_radians(), dec.to_radians());
    let (a0, d0) = (ra0 * 15.0_f32.to_radians(), dec0.to_radians());
    let da = a - a0;
    let k = 2.0 / (1.0 + d0.sin() * d.sin() + d0.cos() * d.cos() * da.cos());
    (k * d.cos() * da.sin(), k * (d0.cos() * d.sin() - d0.sin() * d.cos() * da.cos()))
}

fn star_radius(mag: f32) -> f32 {
    (7.0 - mag * 1.25).clamp(1.5, 7.0)
}

fn draw_star(c: &mut Canvas, x: f32, y: f32, r: f32) {
    c.disc(x, y, r + 2.5, Paint::Paper);
    c.disc(x, y, r, Paint::Ink);
    if r >= 5.0 {
        // Bright stars get a small four-point flare.
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            c.line(x + dx * (r + 3.0), y + dy * (r + 3.0), x + dx * (r + 7.0), y + dy * (r + 7.0), 1.0, Paint::Ink);
        }
    }
}

fn figure_points(fig: &Figure, cx: f32, cy: f32) -> Vec<(f32, f32)> {
    let n = fig.stars.len() as f32;
    let ra0 = fig.stars.iter().map(|s| s.0).sum::<f32>() / n;
    let dec0 = fig.stars.iter().map(|s| s.1).sum::<f32>() / n;
    let raw: Vec<(f32, f32)> = fig.stars.iter().map(|s| project(s.0, s.1, ra0, dec0)).collect();
    let (min_y, max_y) = raw.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    let (min_x, max_x) = raw.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    let scale = (fig.height / (max_y - min_y)).min(420.0 / (max_x - min_x));
    let (mx, my) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    raw.iter().map(|p| (cx - (p.0 - mx) * scale, cy - (p.1 - my) * scale)).collect()
}

fn render(i: usize) -> Art {
    let seed = seed_for(PACK.id, i);
    let mut rng = Rng::new(seed);
    let s32 = (seed >> 13) as u32;
    let fig = FIGURES[i % FIGURES.len()];
    let mut c = Canvas::new();
    let slot_top = i % 2 == 1;
    let slot = if slot_top {
        ClockSlot::centered(W as i32 / 2, 96, ClockStyle::Poster, Surface::Paper)
    } else {
        ClockSlot::centered(W as i32 / 2, 712, ClockStyle::Poster, Surface::Paper)
    };
    let (cx, cy) = (W as f32 / 2.0, if slot_top { 430.0 } else { 350.0 });

    // A faint paper tint over the whole plate, then the Milky Way: a soft diagonal
    // band with cloudy structure, kept to light tones.
    c.fill_rect(Rect::new(22, 22, W as i32 - 44, H as i32 - 44), Paint::Gray(0.93));
    if let Some((angle, offset)) = fig.milky_way {
        let (ca, sa) = (angle.cos(), angle.sin());
        c.shade_all(|x, y| {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            let d = dx * ca + dy * sa - offset;
            let band = (-(d / 110.0).powi(2)).exp();
            let cloud = fbm2(x as f32 * 0.012, y as f32 * 0.012, 4, s32 ^ 0x77);
            let dens = band * (0.05 + 0.22 * cloud);
            if dens > 0.02 {
                Some(Paint::Gray(0.93 - dens))
            } else {
                None
            }
        });
    }
    // Field stars.
    let mut field = rng.fork(1);
    for _ in 0..260 {
        let (x, y) = (12 + field.below(W - 24) as i32, 12 + field.below(H - 24) as i32);
        match field.below(24) {
            0 => c.sparkle(x, y, 3, Paint::Ink),
            1..=5 => c.disc(x as f32 + 0.5, y as f32 + 0.5, 1.4, Paint::Ink),
            _ => c.put(x, y, Paint::Ink),
        }
    }
    // Graticule: a dotted circle and tick marks, like a planisphere window.
    let gr = 236.0;
    for k in 0..720 {
        let a = k as f32 * PI / 360.0;
        if k % 3 == 0 {
            c.put((cx + gr * a.cos()) as i32, (cy + gr * a.sin()) as i32, Paint::Ink);
        }
        if k % 30 == 0 {
            let len = if k % 90 == 0 { 8.0 } else { 4.0 };
            c.line(
                cx + (gr - len) * a.cos(),
                cy + (gr - len) * a.sin(),
                cx + (gr + 1.0) * a.cos(),
                cy + (gr + 1.0) * a.sin(),
                1.0,
                Paint::Ink,
            );
        }
    }
    // The figure.
    let pts = figure_points(fig, cx, cy);
    for &(a, b) in fig.lines {
        c.line(pts[a].0, pts[a].1, pts[b].0, pts[b].1, 1.0, Paint::Ink);
    }
    for (k, s) in fig.stars.iter().enumerate() {
        draw_star(&mut c, pts[k].0, pts[k].1, star_radius(s.2));
    }
    // Plate frame, then a clean halo for the title and the slot so no field star
    // touches them.
    let frame = Rect::new(18, 18, W as i32 - 36, H as i32 - 36);
    c.stroke_rect(frame, 1, Paint::Ink);
    c.stroke_rect(frame.grow(4), 1, Paint::Ink);
    let label_y = if slot_top { 720 } else { 100 };
    c.fill_rect(Rect::new(W as i32 / 2 - 120, label_y - 22, 240, 70), Paint::Paper);
    c.fill_rect(slot.rect().grow(10), Paint::Paper);
    c.stroke_rect(slot.rect().grow(6), 1, Paint::Ink);
    let name = text::small_caps(fig.name);
    text::centered(&mut c, quire_fonts::ui::label_bold(), W as i32 / 2, label_y, &name, Paint::Ink, 4);
    let ital = quire_fonts::nearest(quire_fonts::Family::Literata, quire_fonts::Style::Italic, 20);
    text::centered(&mut c, ital, W as i32 / 2, label_y + 26, fig.epithet, Paint::Ink, 0);
    let rule_w = 60.0;
    c.line(cx - rule_w, label_y as f32 + 40.0, cx + rule_w, label_y as f32 + 40.0, 1.0, Paint::Ink);
    c.disc(cx, label_y as f32 + 40.0, 2.0, Paint::Ink);
    Art { canvas: c, slot, title: fig.name.into(), credit: CREDIT.into() }
}

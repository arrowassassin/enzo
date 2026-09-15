//! Panel plane geometry and the framebuffer → plane rotation helper.
//!
//! The glass is a landscape 792 × 528 raster: 99 bytes per row, 528 rows, 52,272 bytes per
//! 1-bit plane, **1 = white, 0 = black** (x3-uc8279-driver-reference.md "Data Planes",
//! x3-specifications.md "Framebuffer"). The driver streams rows in reverse order (row 527 first)
//! exactly as the stock firmware and PapyriX do; the plane buffer itself is stored in natural
//! row order, so callers never see the mirroring.
//!
//! Quire draws into a portrait `quire_gfx::Frame` (528 × 792, MSB first, **1 = ink**). The
//! helpers here turn such a frame into the panel plane for each [`Rotation`], inverting the
//! polarity on the way. No allocation: the caller owns both buffers.

/// Panel width in its native (landscape) raster, pixels.
pub const PANEL_W: usize = 792;
/// Panel height in its native (landscape) raster, pixels (= number of gate lines / rows sent).
pub const PANEL_H: usize = 528;
/// Bytes per panel row (792 / 8).
pub const ROW_BYTES: usize = PANEL_W / 8;
/// Bytes per 1-bit plane (99 × 528).
pub const PLANE_BYTES: usize = ROW_BYTES * PANEL_H;

/// Portrait frame width (device held upright, keys at the bottom), pixels.
pub const FRAME_W: usize = 528;
/// Portrait frame height, pixels.
pub const FRAME_H: usize = 792;
/// Bytes per portrait frame row (528 / 8).
pub const FRAME_STRIDE: usize = FRAME_W / 8;

const _: () = assert!(PLANE_BYTES == 52_272);
const _: () = assert!(FRAME_STRIDE * FRAME_H == PLANE_BYTES);

/// How the content of a frame is turned when it is placed on the glass.
///
/// Mirrors `quire_gfx::Rotation` variant-for-variant so the firmware can map one to the other
/// with a trivial `match`; this crate deliberately has no dependency on `quire-gfx`.
///
/// The frame handed to [`rotate_frame_to_plane`] has the dimensions of [`Rotation::frame_size`]:
/// portrait 528 × 792 for `Portrait` / `Flip180`, landscape 792 × 528 for `Cw90` / `Ccw90`.
/// In every case the frame is what the reader sees, upright in *its* orientation; the rotation
/// says where that content's top edge ends up on the device:
///
/// | Rotation   | Content top edge on the device | Typical use                     |
/// |------------|--------------------------------|---------------------------------|
/// | `Portrait` | top (keys below the text)      | default reading                 |
/// | `Flip180`  | bottom                         | left-handed portrait            |
/// | `Cw90`     | right                          | landscape, keys on the left     |
/// | `Ccw90`    | left                           | landscape, keys on the right    |
///
/// Which physical edge of the glass is "top" is fixed by [`portrait_to_panel`]; see the note there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Rotation {
    /// Portrait, keys along the bottom.
    #[default]
    Portrait,
    /// Content turned 90° clockwise (landscape, keys on the left).
    Cw90,
    /// Upside down (left-handed portrait).
    Flip180,
    /// Content turned 90° counter-clockwise (landscape, keys on the right).
    Ccw90,
}

impl Rotation {
    /// `(width, height)` of the content frame this rotation expects.
    pub const fn frame_size(self) -> (usize, usize) {
        match self {
            Rotation::Portrait | Rotation::Flip180 => (FRAME_W, FRAME_H),
            Rotation::Cw90 | Rotation::Ccw90 => (FRAME_H, FRAME_W),
        }
    }

    /// Bytes per row of the content frame this rotation expects.
    pub const fn frame_stride(self) -> usize {
        self.frame_size().0 / 8
    }

    /// Map a content pixel `(x, y)` to device-portrait coordinates `(dx, dy)`,
    /// `dx ∈ 0..528`, `dy ∈ 0..792`, `dy = 0` at the top edge of the upright device.
    #[inline]
    pub const fn to_device(self, x: usize, y: usize) -> (usize, usize) {
        match self {
            Rotation::Portrait => (x, y),
            Rotation::Flip180 => (FRAME_W - 1 - x, FRAME_H - 1 - y),
            // Content turned clockwise: its top-left corner lands at the device's top-right.
            Rotation::Cw90 => (FRAME_W - 1 - y, x),
            // Content turned counter-clockwise: its top-left corner lands at the device's bottom-left.
            Rotation::Ccw90 => (y, FRAME_H - 1 - x),
        }
    }
}

/// Map a device-portrait pixel to the panel's native raster `(px, py)`, `px ∈ 0..792` (source
/// column), `py ∈ 0..528` (gate row, in the *stored* plane order — the driver sends row 527 first).
///
/// **Unverified on hardware.** The open-source drivers only ever show the panel a landscape
/// buffer whose rows they mirror on send; none of them documents which glass edge gate 0 sits
/// on. This is the single place to fix if the first bring-up shows the page mirrored or turned:
/// the alternatives are `(PANEL_W - 1 - dy, dx)` (180°), `(dy, dx)` and `(PANEL_W - 1 - dy,
/// PANEL_H - 1 - dx)` (mirrored). Everything else in the crate composes with this function.
#[inline]
pub const fn portrait_to_panel(dx: usize, dy: usize) -> (usize, usize) {
    (dy, PANEL_H - 1 - dx)
}

/// Rotate a portrait `quire_gfx::Frame` bit image (1 = ink) into a panel plane (1 = white).
///
/// `frame` is MSB-first packed with the dimensions of `rotation.frame_size()`; it may be longer
/// than needed (extra bytes are ignored). Panics if it is shorter — that is a programming error,
/// not a runtime condition.
///
/// Cost: one pass over the source bytes; fully white source bytes are skipped, so a typical
/// text page costs a few milliseconds on the ESP32-C3.
pub fn rotate_frame_to_plane(frame: &[u8], out_plane: &mut [u8; PLANE_BYTES], rotation: Rotation) {
    rotate_bits_to_plane(frame, out_plane, rotation, true);
}

/// General form of [`rotate_frame_to_plane`].
///
/// * `set_is_ink == true`: a set source bit is ink; the plane starts all-white (`0xFF`) and set
///   bits clear their pixel (the `Frame` convention).
/// * `set_is_ink == false`: a set source bit is *light*; the plane starts all-black (`0x00`) and set
///   bits set their pixel. Use this for the two grey planes produced by `quire_gfx::dither`
///   (value 0 = black … 3 = white, so both the MSB and LSB planes have 1 = lighter).
pub fn rotate_bits_to_plane(src: &[u8], out_plane: &mut [u8; PLANE_BYTES], rotation: Rotation, set_is_ink: bool) {
    let (w, h) = rotation.frame_size();
    let stride = w / 8;
    assert!(src.len() >= stride * h, "quire-epd: frame buffer too small for {rotation:?} ({} < {})", src.len(), stride * h);

    out_plane.fill(if set_is_ink { 0xFF } else { 0x00 });
    for y in 0..h {
        let row = &src[y * stride..(y + 1) * stride];
        for (bx, &byte) in row.iter().enumerate() {
            if byte == 0 {
                continue;
            }
            for bit in 0..8 {
                if byte & (0x80 >> bit) == 0 {
                    continue;
                }
                let (dx, dy) = rotation.to_device(bx * 8 + bit, y);
                let (px, py) = portrait_to_panel(dx, dy);
                let idx = py * ROW_BYTES + px / 8;
                let mask = 0x80 >> (px % 8);
                if set_is_ink {
                    out_plane[idx] &= !mask;
                } else {
                    out_plane[idx] |= mask;
                }
            }
        }
    }
}

/// Read one pixel of a plane in its native raster (`true` = white).
pub fn plane_pixel(plane: &[u8; PLANE_BYTES], px: usize, py: usize) -> bool {
    plane[py * ROW_BYTES + px / 8] & (0x80 >> (px % 8)) != 0
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;
    use std::vec::Vec;

    fn frame_with(rot: Rotation, pixels: &[(usize, usize)]) -> Vec<u8> {
        let (w, h) = rot.frame_size();
        let stride = w / 8;
        let mut f = vec![0u8; stride * h];
        for &(x, y) in pixels {
            f[y * stride + x / 8] |= 0x80 >> (x % 8);
        }
        f
    }

    fn count_black(plane: &[u8; PLANE_BYTES]) -> usize {
        plane.iter().map(|b| b.count_zeros() as usize).sum()
    }

    #[test]
    fn white_frame_gives_all_white_plane() {
        let f = vec![0u8; FRAME_STRIDE * FRAME_H];
        let mut p = [0u8; PLANE_BYTES];
        rotate_frame_to_plane(&f, &mut p, Rotation::Portrait);
        assert!(p.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn known_pixel_lands_where_expected_in_all_rotations() {
        // Content pixel (3, 5) in a frame of the rotation's size.
        for rot in [Rotation::Portrait, Rotation::Flip180, Rotation::Cw90, Rotation::Ccw90] {
            let f = frame_with(rot, &[(3, 5)]);
            let mut p = [0u8; PLANE_BYTES];
            rotate_frame_to_plane(&f, &mut p, rot);
            assert_eq!(count_black(&p), 1, "{rot:?}: exactly one black pixel");
            let (dx, dy) = match rot {
                Rotation::Portrait => (3, 5),
                Rotation::Flip180 => (527 - 3, 791 - 5),
                Rotation::Cw90 => (527 - 5, 3),
                Rotation::Ccw90 => (5, 791 - 3),
            };
            assert_eq!(rot.to_device(3, 5), (dx, dy), "{rot:?}: device coords");
            let (px, py) = portrait_to_panel(dx, dy);
            assert_eq!((px, py), (dy, 527 - dx));
            assert!(!plane_pixel(&p, px, py), "{rot:?}: pixel is black at ({px}, {py})");
        }
    }

    #[test]
    fn corners_map_to_panel_corners() {
        // Upright device: top-left content pixel → panel (0, 527); bottom-right → (791, 0).
        assert_eq!(portrait_to_panel(0, 0), (0, PANEL_H - 1));
        assert_eq!(portrait_to_panel(FRAME_W - 1, FRAME_H - 1), (PANEL_W - 1, 0));
        // Landscape content 792 × 528 turned clockwise: its top edge runs down the device's right edge.
        assert_eq!(Rotation::Cw90.to_device(0, 0), (FRAME_W - 1, 0));
        assert_eq!(Rotation::Cw90.to_device(791, 0), (FRAME_W - 1, FRAME_H - 1));
        assert_eq!(Rotation::Ccw90.to_device(0, 0), (0, FRAME_H - 1));
        assert_eq!(Rotation::Ccw90.to_device(791, 527), (FRAME_W - 1, 0));
    }

    #[test]
    fn rotations_are_bijections_and_flip_round_trips() {
        // Every content pixel maps to a distinct device pixel and back.
        for rot in [Rotation::Portrait, Rotation::Flip180, Rotation::Cw90, Rotation::Ccw90] {
            let (w, h) = rot.frame_size();
            let mut seen = vec![false; FRAME_W * FRAME_H];
            for y in 0..h {
                for x in 0..w {
                    let (dx, dy) = rot.to_device(x, y);
                    assert!(dx < FRAME_W && dy < FRAME_H);
                    let i = dy * FRAME_W + dx;
                    assert!(!seen[i], "{rot:?}: ({x},{y}) collides");
                    seen[i] = true;
                }
            }
            assert!(seen.iter().all(|&s| s));
        }
        // Flip180 twice is the identity.
        for &(x, y) in &[(0, 0), (17, 300), (527, 791)] {
            let (dx, dy) = Rotation::Flip180.to_device(x, y);
            assert_eq!(Rotation::Flip180.to_device(dx, dy), (x, y));
        }
        // Cw90 then Ccw90 (as content in device coordinates) is the identity.
        for &(x, y) in &[(0, 0), (40, 3), (791, 527)] {
            let (dx, dy) = Rotation::Cw90.to_device(x, y);
            // dx ∈ 0..528, dy ∈ 0..792 as portrait device coords; feed them to Ccw90 as content coords
            // by swapping axes (a 528 × 792 device image seen as 792 × 528 content is the transpose).
            let (ex, ey) = Rotation::Ccw90.to_device(dy, dx);
            // Ccw90(dy, dx) = (dx, 791 - dy); for Cw90 (dx, dy) = (527 - y, x) so ex = 527 - y, ey = 791 - x.
            assert_eq!((ex, ey), (527 - y, 791 - x));
        }
    }

    #[test]
    fn plane_round_trips_through_inverse_mapping() {
        // Build a pattern, rotate it, then read every plane pixel back through the maps.
        let rot = Rotation::Portrait;
        let pixels: Vec<(usize, usize)> = (0..200).map(|i| ((i * 37) % FRAME_W, (i * 91) % FRAME_H)).collect();
        let f = frame_with(rot, &pixels);
        let mut p = [0u8; PLANE_BYTES];
        rotate_frame_to_plane(&f, &mut p, rot);
        for (x, y) in pixels.iter().copied() {
            let (px, py) = portrait_to_panel(x, y);
            assert!(!plane_pixel(&p, px, py));
        }
        let mut black = 0;
        for py in 0..PANEL_H {
            for px in 0..PANEL_W {
                if !plane_pixel(&p, px, py) {
                    black += 1;
                }
            }
        }
        let mut uniq = pixels.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(black, uniq.len());
    }

    #[test]
    fn grey_planes_keep_light_polarity() {
        // set_is_ink == false: a set bit means "lighter" and becomes a 1 in the plane.
        let f = frame_with(Rotation::Portrait, &[(0, 0)]);
        let mut p = [0xAAu8; PLANE_BYTES];
        rotate_bits_to_plane(&f, &mut p, Rotation::Portrait, false);
        assert_eq!(p.iter().map(|b| b.count_ones() as usize).sum::<usize>(), 1);
        assert!(plane_pixel(&p, 0, 527));
    }

    #[test]
    #[should_panic(expected = "frame buffer too small")]
    fn short_frame_panics() {
        let f = vec![0u8; 10];
        let mut p = [0u8; PLANE_BYTES];
        rotate_frame_to_plane(&f, &mut p, Rotation::Portrait);
    }
}

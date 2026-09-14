//! The packed 1-bit frame and the drawing primitives every screen is built from.

use alloc::vec;
use alloc::vec::Vec;

/// Ink or paper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// Black.
    Black,
    /// White.
    White,
}

impl Ink {
    #[inline]
    fn bit(self) -> bool {
        matches!(self, Ink::Black)
    }
}

/// An axis-aligned rectangle in frame coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
}

impl Rect {
    /// Construct a rectangle.
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Rect { x, y, w, h }
    }
    /// Right edge (exclusive).
    pub const fn right(&self) -> i32 {
        self.x + self.w as i32
    }
    /// Bottom edge (exclusive).
    pub const fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }
    /// Shrink by `n` on every side (saturating at zero size).
    pub fn inset(&self, n: i32) -> Rect {
        let w = (self.w as i32 - 2 * n).max(0) as u32;
        let h = (self.h as i32 - 2 * n).max(0) as u32;
        Rect::new(self.x + n, self.y + n, w, h)
    }
    /// Intersection with another rectangle, or an empty rectangle.
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 <= x0 || y1 <= y0 {
            Rect::default()
        } else {
            Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
        }
    }
    /// True when the rectangle has no area.
    pub const fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
    /// True when the point lies inside.
    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

/// Fill patterns that stand in for grey on a 1-bit panel (brief §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// Solid ink.
    Solid,
    /// Exactly 50 %: a 2 px checkerboard. Disabled content sits beneath it.
    Dots50,
    /// About 25 %: one dot in a 2 × 2 cell.
    Dots25,
    /// Sparse dots, one in a 4 × 4 cell (~6 %).
    Sparse,
    /// Horizontal hatch: a 1 px line every `pitch` rows.
    Hatch {
        /// Row pitch in pixels (6 for the secondary surface).
        pitch: u8,
    },
}

impl Pattern {
    /// Whether the pattern paints ink at an absolute frame coordinate.
    #[inline]
    pub fn ink_at(self, x: i32, y: i32) -> bool {
        match self {
            Pattern::Solid => true,
            Pattern::Dots50 => ((x >> 1) + (y >> 1)) & 1 == 0,
            Pattern::Dots25 => (x & 1 == 0) && (y & 1 == 0),
            Pattern::Sparse => (x & 3 == 0) && (y & 3 == 0),
            Pattern::Hatch { pitch } => y.rem_euclid(pitch.max(1) as i32) == 0,
        }
    }
}

/// How a bitmap combines with what is already on the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlitMode {
    /// Set bits paint ink; clear bits leave the frame alone.
    Or,
    /// Set bits paint paper (white glyphs on an inverted row).
    Clear,
    /// Set bits flip the frame.
    Xor,
    /// Copy every bit, ink and paper.
    Copy,
}

/// Orientation of the panel relative to the portrait frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Rotation {
    /// Portrait, keys along the bottom.
    #[default]
    Portrait,
    /// Rotated 90° clockwise (landscape, keys on the left).
    Cw90,
    /// Upside down (left-handed portrait).
    Flip180,
    /// Rotated 90° counter-clockwise (landscape, keys on the right).
    Ccw90,
}

/// A borrowed packed 1-bit image, MSB first, rows padded to whole bytes.
#[derive(Clone, Copy, Debug)]
pub struct BitmapRef<'a> {
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
    /// Packed rows; each row is `(w + 7) / 8` bytes.
    pub bits: &'a [u8],
}

impl<'a> BitmapRef<'a> {
    /// Row stride in bytes.
    #[inline]
    pub const fn stride(&self) -> usize {
        (self.w as usize).div_ceil(8)
    }
    /// Pixel at (x, y); out of range reads as clear.
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> bool {
        if x >= self.w || y >= self.h {
            return false;
        }
        let i = y as usize * self.stride() + (x as usize >> 3);
        self.bits.get(i).is_some_and(|b| b & (0x80 >> (x & 7)) != 0)
    }
}

/// An owned packed 1-bit image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
    /// Packed rows.
    pub bits: Vec<u8>,
}

impl Bitmap {
    /// A clear bitmap.
    pub fn new(w: u32, h: u32) -> Self {
        let stride = (w as usize).div_ceil(8);
        Bitmap { w, h, bits: vec![0; stride * h as usize] }
    }
    /// Borrow as a [`BitmapRef`].
    pub fn as_ref(&self) -> BitmapRef<'_> {
        BitmapRef { w: self.w, h: self.h, bits: &self.bits }
    }
    /// Set a pixel.
    #[inline]
    pub fn set(&mut self, x: u32, y: u32, on: bool) {
        if x >= self.w || y >= self.h {
            return;
        }
        let stride = (self.w as usize).div_ceil(8);
        let i = y as usize * stride + (x as usize >> 3);
        let m = 0x80 >> (x & 7);
        if on {
            self.bits[i] |= m;
        } else {
            self.bits[i] &= !m;
        }
    }
    /// Read a pixel.
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> bool {
        self.as_ref().get(x, y)
    }
}

/// The 1-bit frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    w: u32,
    h: u32,
    stride: usize,
    bits: Vec<u8>,
}

impl Frame {
    /// A white frame of the given size.
    pub fn new(w: u32, h: u32) -> Self {
        let stride = (w as usize).div_ceil(8);
        Frame { w, h, stride, bits: vec![0; stride * h as usize] }
    }
    /// A white portrait panel frame.
    pub fn panel() -> Self {
        Frame::new(crate::PANEL_W, crate::PANEL_H)
    }
    /// Width in pixels.
    #[inline]
    pub const fn width(&self) -> u32 {
        self.w
    }
    /// Height in pixels.
    #[inline]
    pub const fn height(&self) -> u32 {
        self.h
    }
    /// The whole frame as a rectangle.
    pub const fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }
    /// Row stride in bytes.
    #[inline]
    pub const fn stride(&self) -> usize {
        self.stride
    }
    /// Packed pixel data, rows top to bottom.
    #[inline]
    pub fn bits(&self) -> &[u8] {
        &self.bits
    }
    /// Mutable packed pixel data.
    #[inline]
    pub fn bits_mut(&mut self) -> &mut [u8] {
        &mut self.bits
    }
    /// Borrow the frame as a bitmap.
    pub fn as_bitmap(&self) -> BitmapRef<'_> {
        BitmapRef { w: self.w, h: self.h, bits: &self.bits }
    }
    /// Paint the whole frame.
    pub fn clear(&mut self, ink: Ink) {
        let v = if ink.bit() { 0xFF } else { 0x00 };
        self.bits.fill(v);
    }
    /// Read a pixel; outside the frame reads as paper.
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return false;
        }
        let i = y as usize * self.stride + (x as usize >> 3);
        self.bits[i] & (0x80 >> (x & 7)) != 0
    }
    /// Set a pixel; outside the frame is ignored.
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, ink: Ink) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = y as usize * self.stride + (x as usize >> 3);
        let m = 0x80 >> (x & 7);
        if ink.bit() {
            self.bits[i] |= m;
        } else {
            self.bits[i] &= !m;
        }
    }
    /// Flip a pixel.
    #[inline]
    pub fn flip(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = y as usize * self.stride + (x as usize >> 3);
        self.bits[i] ^= 0x80 >> (x & 7);
    }

    /// Fill a rectangle with solid ink or paper.
    pub fn fill_rect(&mut self, r: Rect, ink: Ink) {
        let r = r.intersect(&self.bounds());
        if r.is_empty() {
            return;
        }
        for y in r.y..r.bottom() {
            self.fill_span(y, r.x, r.right(), ink.bit());
        }
    }

    /// Fill a rectangle with a pattern (ink where the pattern is set, paper elsewhere).
    pub fn pattern_rect(&mut self, r: Rect, p: Pattern) {
        if let Pattern::Solid = p {
            return self.fill_rect(r, Ink::Black);
        }
        let r = r.intersect(&self.bounds());
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.set(x, y, if p.ink_at(x, y) { Ink::Black } else { Ink::White });
            }
        }
    }

    /// Overlay a pattern: only the pattern's ink pixels are painted (a dot screen over content).
    pub fn screen_rect(&mut self, r: Rect, p: Pattern) {
        let r = r.intersect(&self.bounds());
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                if p.ink_at(x, y) {
                    self.set(x, y, Ink::Black);
                }
            }
        }
    }

    /// Invert a rectangle (the focus treatment).
    pub fn invert_rect(&mut self, r: Rect) {
        let r = r.intersect(&self.bounds());
        if r.is_empty() {
            return;
        }
        for y in r.y..r.bottom() {
            self.xor_span(y, r.x, r.right());
        }
    }

    /// Horizontal rule of the given thickness, top edge at `y`.
    pub fn hline(&mut self, x: i32, y: i32, len: u32, thickness: u32, ink: Ink) {
        self.fill_rect(Rect::new(x, y, len, thickness), ink);
    }
    /// Vertical rule of the given thickness, left edge at `x`.
    pub fn vline(&mut self, x: i32, y: i32, len: u32, thickness: u32, ink: Ink) {
        self.fill_rect(Rect::new(x, y, thickness, len), ink);
    }
    /// Rectangle outline drawn inside `r`.
    pub fn stroke_rect(&mut self, r: Rect, thickness: u32, ink: Ink) {
        if r.is_empty() {
            return;
        }
        let t = thickness.min(r.w).min(r.h);
        self.fill_rect(Rect::new(r.x, r.y, r.w, t), ink);
        self.fill_rect(Rect::new(r.x, r.bottom() - t as i32, r.w, t), ink);
        self.fill_rect(Rect::new(r.x, r.y, t, r.h), ink);
        self.fill_rect(Rect::new(r.right() - t as i32, r.y, t, r.h), ink);
    }

    /// Blit a packed bitmap with its top-left corner at (x, y).
    pub fn blit(&mut self, x: i32, y: i32, bm: BitmapRef<'_>, mode: BlitMode) {
        let dst = Rect::new(x, y, bm.w, bm.h).intersect(&self.bounds());
        if dst.is_empty() {
            return;
        }
        for dy in dst.y..dst.bottom() {
            let sy = (dy - y) as u32;
            for dx in dst.x..dst.right() {
                let sx = (dx - x) as u32;
                let on = bm.get(sx, sy);
                match mode {
                    BlitMode::Or => {
                        if on {
                            self.set(dx, dy, Ink::Black)
                        }
                    }
                    BlitMode::Clear => {
                        if on {
                            self.set(dx, dy, Ink::White)
                        }
                    }
                    BlitMode::Xor => {
                        if on {
                            self.flip(dx, dy)
                        }
                    }
                    BlitMode::Copy => self.set(dx, dy, if on { Ink::Black } else { Ink::White }),
                }
            }
        }
    }

    /// Blit a bitmap dilated by one pixel to the right and down ("darker text").
    pub fn blit_bold(&mut self, x: i32, y: i32, bm: BitmapRef<'_>, mode: BlitMode) {
        self.blit(x, y, bm, mode);
        self.blit(x + 1, y, bm, mode);
    }

    /// Copy a rectangle of another frame onto this one at the same coordinates.
    pub fn copy_rect_from(&mut self, src: &Frame, r: Rect) {
        let r = r.intersect(&self.bounds()).intersect(&src.bounds());
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.set(x, y, if src.get(x, y) { Ink::Black } else { Ink::White });
            }
        }
    }

    /// A rotated copy, as the panel expects for the given orientation.
    pub fn rotated(&self, rot: Rotation) -> Frame {
        match rot {
            Rotation::Portrait => self.clone(),
            Rotation::Flip180 => {
                let mut out = Frame::new(self.w, self.h);
                for y in 0..self.h as i32 {
                    for x in 0..self.w as i32 {
                        if self.get(x, y) {
                            out.set(self.w as i32 - 1 - x, self.h as i32 - 1 - y, Ink::Black);
                        }
                    }
                }
                out
            }
            Rotation::Cw90 => {
                let mut out = Frame::new(self.h, self.w);
                for y in 0..self.h as i32 {
                    for x in 0..self.w as i32 {
                        if self.get(x, y) {
                            out.set(self.h as i32 - 1 - y, x, Ink::Black);
                        }
                    }
                }
                out
            }
            Rotation::Ccw90 => {
                let mut out = Frame::new(self.h, self.w);
                for y in 0..self.h as i32 {
                    for x in 0..self.w as i32 {
                        if self.get(x, y) {
                            out.set(y, self.w as i32 - 1 - x, Ink::Black);
                        }
                    }
                }
                out
            }
        }
    }

    /// Count of ink pixels (used by tests and the heap/ghosting heuristics).
    pub fn ink_count(&self) -> usize {
        self.bits.iter().map(|b| b.count_ones() as usize).sum()
    }

    #[inline]
    fn fill_span(&mut self, y: i32, x0: i32, x1: i32, on: bool) {
        // x0 inclusive, x1 exclusive, both already clipped.
        let row = y as usize * self.stride;
        let (b0, b1) = ((x0 as usize) >> 3, ((x1 - 1) as usize) >> 3);
        let m0: u8 = 0xFF >> (x0 & 7);
        let m1: u8 = (0xFF00u16 >> (((x1 - 1) & 7) + 1)) as u8;
        if b0 == b1 {
            let m = m0 & m1;
            if on {
                self.bits[row + b0] |= m;
            } else {
                self.bits[row + b0] &= !m;
            }
            return;
        }
        if on {
            self.bits[row + b0] |= m0;
            for b in &mut self.bits[row + b0 + 1..row + b1] {
                *b = 0xFF;
            }
            self.bits[row + b1] |= m1;
        } else {
            self.bits[row + b0] &= !m0;
            for b in &mut self.bits[row + b0 + 1..row + b1] {
                *b = 0;
            }
            self.bits[row + b1] &= !m1;
        }
    }

    #[inline]
    fn xor_span(&mut self, y: i32, x0: i32, x1: i32) {
        let row = y as usize * self.stride;
        let (b0, b1) = ((x0 as usize) >> 3, ((x1 - 1) as usize) >> 3);
        let m0: u8 = 0xFF >> (x0 & 7);
        let m1: u8 = (0xFF00u16 >> (((x1 - 1) & 7) + 1)) as u8;
        if b0 == b1 {
            self.bits[row + b0] ^= m0 & m1;
            return;
        }
        self.bits[row + b0] ^= m0;
        for b in &mut self.bits[row + b0 + 1..row + b1] {
            *b ^= 0xFF;
        }
        self.bits[row + b1] ^= m1;
    }
}

#[cfg(feature = "eg")]
mod eg_impl {
    use super::{Frame, Ink};
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::*;

    impl OriginDimensions for Frame {
        fn size(&self) -> Size {
            Size::new(self.w, self.h)
        }
    }

    impl DrawTarget for Frame {
        type Color = BinaryColor;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for Pixel(p, c) in pixels {
                let ink = if c.is_on() { Ink::Black } else { Ink::White };
                self.set(p.x, p.y, ink);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_fill_crosses_byte_boundaries() {
        let mut f = Frame::new(32, 2);
        f.fill_rect(Rect::new(5, 0, 20, 1), Ink::Black);
        for x in 0..32 {
            assert_eq!(f.get(x, 0), (5..25).contains(&x), "x={x}");
            assert!(!f.get(x, 1));
        }
        f.fill_rect(Rect::new(7, 0, 3, 1), Ink::White);
        assert!(f.get(6, 0) && !f.get(7, 0) && !f.get(9, 0) && f.get(10, 0));
        assert_eq!(f.ink_count(), 17);
    }

    #[test]
    fn invert_is_involutive_and_clipped() {
        let mut f = Frame::new(20, 20);
        f.fill_rect(Rect::new(0, 0, 10, 20), Ink::Black);
        let before = f.clone();
        f.invert_rect(Rect::new(-5, 5, 30, 5));
        assert!(!f.get(0, 5) && f.get(15, 5) && f.get(0, 4) && !f.get(15, 4));
        f.invert_rect(Rect::new(-5, 5, 30, 5));
        assert_eq!(f, before);
    }

    #[test]
    fn dots50_is_exactly_half() {
        let mut f = Frame::new(64, 64);
        f.pattern_rect(f.bounds(), Pattern::Dots50);
        assert_eq!(f.ink_count(), 64 * 64 / 2);
    }

    #[test]
    fn rotation_round_trips() {
        let mut f = Frame::new(8, 4);
        f.set(1, 0, Ink::Black);
        f.set(7, 3, Ink::Black);
        let r = f.rotated(Rotation::Cw90);
        assert_eq!((r.width(), r.height()), (4, 8));
        assert!(r.get(3, 1) && r.get(0, 7));
        let back = r.rotated(Rotation::Ccw90);
        assert_eq!(back, f);
        assert_eq!(f.rotated(Rotation::Flip180).rotated(Rotation::Flip180), f);
    }

    #[test]
    fn blit_modes() {
        let mut f = Frame::new(8, 1);
        let bm = Bitmap { w: 4, h: 1, bits: alloc::vec![0b1010_0000] };
        f.blit(2, 0, bm.as_ref(), BlitMode::Or);
        assert!(f.get(2, 0) && !f.get(3, 0) && f.get(4, 0));
        f.blit(2, 0, bm.as_ref(), BlitMode::Xor);
        assert_eq!(f.ink_count(), 0);
        f.clear(Ink::Black);
        f.blit(2, 0, bm.as_ref(), BlitMode::Clear);
        assert!(!f.get(2, 0) && f.get(3, 0));
    }
}

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
    /// The pattern's ink bits for the eight pixels of any byte-aligned run on row `y`
    /// (every period divides 8, so the byte is the same all along the row).
    #[inline]
    pub fn row_byte(self, y: i32) -> u8 {
        match self {
            Pattern::Solid => 0xFF,
            Pattern::Dots50 => {
                if (y >> 1) & 1 == 0 {
                    0xCC
                } else {
                    0x33
                }
            }
            Pattern::Dots25 => {
                if y & 1 == 0 {
                    0xAA
                } else {
                    0
                }
            }
            Pattern::Sparse => {
                if y & 3 == 0 {
                    0x88
                } else {
                    0
                }
            }
            Pattern::Hatch { pitch } => {
                if y.rem_euclid(pitch.max(1) as i32) == 0 {
                    0xFF
                } else {
                    0
                }
            }
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
        if r.is_empty() {
            return;
        }
        for y in r.y..r.bottom() {
            let v = p.row_byte(y);
            self.span_op(y, r.x, r.right(), |d, m| (d & !m) | (v & m));
        }
    }

    /// Overlay a pattern: only the pattern's ink pixels are painted (a dot screen over content).
    pub fn screen_rect(&mut self, r: Rect, p: Pattern) {
        let r = r.intersect(&self.bounds());
        if r.is_empty() {
            return;
        }
        for y in r.y..r.bottom() {
            let v = p.row_byte(y);
            if v != 0 {
                self.span_op(y, r.x, r.right(), |d, m| d | (v & m));
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
        let stride = bm.stride();
        for dy in dst.y..dst.bottom() {
            let sy = (dy - y) as usize;
            let row = bm.bits.get(sy * stride..).unwrap_or(&[]);
            let row = &row[..stride.min(row.len())];
            self.blit_row(x, dy, row, bm.w, mode);
        }
    }

    /// Blit one packed row of `w` pixels (MSB first) so its first pixel lands at (x, y).
    /// Works a byte at a time with a shifted source window; clips at both edges. Bits of
    /// `src` beyond `w` are ignored, and a short `src` reads as clear.
    pub fn blit_row(&mut self, x: i32, y: i32, src: &[u8], w: u32, mode: BlitMode) {
        if y < 0 || y >= self.h as i32 || w == 0 {
            return;
        }
        let x0 = x.max(0);
        let x1 = (x + w as i32).min(self.w as i32);
        if x1 <= x0 {
            return;
        }
        let row = y as usize * self.stride;
        let (b0, b1) = ((x0 as usize) >> 3, ((x1 - 1) as usize) >> 3);
        let (m0, m1) = edge_masks(x0, x1);
        let n = (w as usize).div_ceil(8).min(src.len());
        let src = &src[..n];
        let sbyte = |i: i32| -> u8 {
            if i < 0 || i as usize >= src.len() {
                0
            } else {
                src[i as usize]
            }
        };
        for b in b0..=b1 {
            let sx0 = (b as i32) * 8 - x;
            let i = sx0.div_euclid(8);
            let k = sx0.rem_euclid(8) as u32;
            let v = if k == 0 { sbyte(i) } else { (sbyte(i) << k) | (sbyte(i + 1) >> (8 - k)) };
            let mut m = 0xFFu8;
            if b == b0 {
                m &= m0;
            }
            if b == b1 {
                m &= m1;
            }
            let d = &mut self.bits[row + b];
            match mode {
                BlitMode::Or => *d |= v & m,
                BlitMode::Clear => *d &= !(v & m),
                BlitMode::Xor => *d ^= v & m,
                BlitMode::Copy => *d = (*d & !m) | (v & m),
            }
        }
    }

    /// Blit a bitmap rotated 90° counter-clockwise (its rows become columns read bottom
    /// to top), with the rotated image's top-left corner at (x, y). The rotated image is
    /// `bm.h` wide and `bm.w` tall. Glyph-sized bitmaps only need this; it skips empty
    /// source bytes and otherwise works per pixel.
    pub fn blit_ccw(&mut self, x: i32, y: i32, bm: BitmapRef<'_>, mode: BlitMode) {
        let dst = Rect::new(x, y, bm.h, bm.w).intersect(&self.bounds());
        if dst.is_empty() {
            return;
        }
        let stride = bm.stride();
        for py in 0..bm.h as i32 {
            let dx = x + py;
            if dx < 0 || dx >= self.w as i32 {
                continue;
            }
            let row = bm.bits.get(py as usize * stride..).unwrap_or(&[]);
            for (b, &v) in row.iter().enumerate().take(stride) {
                if v == 0 && mode != BlitMode::Copy {
                    continue;
                }
                for j in 0..8 {
                    let px = (b * 8 + j) as i32;
                    if px >= bm.w as i32 {
                        break;
                    }
                    let on = v & (0x80 >> j) != 0;
                    let dy = y + bm.w as i32 - 1 - px;
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
    }

    /// Blit a bitmap dilated by one pixel to the right and down ("darker text").
    pub fn blit_bold(&mut self, x: i32, y: i32, bm: BitmapRef<'_>, mode: BlitMode) {
        self.blit(x, y, bm, mode);
        self.blit(x + 1, y, bm, mode);
    }

    /// Copy a rectangle of another frame onto this one at the same coordinates.
    pub fn copy_rect_from(&mut self, src: &Frame, r: Rect) {
        let r = r.intersect(&self.bounds()).intersect(&src.bounds());
        if r.is_empty() {
            return;
        }
        let (x0, x1) = (r.x, r.right());
        let (b0, b1) = ((x0 as usize) >> 3, ((x1 - 1) as usize) >> 3);
        let (m0, m1) = edge_masks(x0, x1);
        for y in r.y..r.bottom() {
            let d = y as usize * self.stride;
            let s = y as usize * src.stride;
            if b0 == b1 {
                let m = m0 & m1;
                self.bits[d + b0] = (self.bits[d + b0] & !m) | (src.bits[s + b0] & m);
                continue;
            }
            self.bits[d + b0] = (self.bits[d + b0] & !m0) | (src.bits[s + b0] & m0);
            self.bits[d + b0 + 1..d + b1].copy_from_slice(&src.bits[s + b0 + 1..s + b1]);
            self.bits[d + b1] = (self.bits[d + b1] & !m1) | (src.bits[s + b1] & m1);
        }
    }

    /// A rotated copy, as the panel expects for the given orientation.
    pub fn rotated(&self, rot: Rotation) -> Frame {
        match rot {
            Rotation::Portrait => self.clone(),
            Rotation::Flip180 => {
                let mut out = Frame::new(self.w, self.h);
                if self.w.is_multiple_of(8) {
                    // Whole bytes: reverse the byte order of each row and the bits of each byte.
                    let st = self.stride;
                    for y in 0..self.h as usize {
                        let src = &self.bits[y * st..(y + 1) * st];
                        let oy = self.h as usize - 1 - y;
                        let dst = &mut out.bits[oy * st..(oy + 1) * st];
                        for (d, s) in dst.iter_mut().zip(src.iter().rev()) {
                            *d = s.reverse_bits();
                        }
                    }
                } else {
                    for y in 0..self.h as i32 {
                        for x in 0..self.w as i32 {
                            if self.get(x, y) {
                                out.set(self.w as i32 - 1 - x, self.h as i32 - 1 - y, Ink::Black);
                            }
                        }
                    }
                }
                out
            }
            Rotation::Cw90 => {
                // Source row y becomes output column h-1-y; set bits only, skipping empty
                // source bytes (a page of text is mostly paper).
                let mut out = Frame::new(self.h, self.w);
                let os = out.stride;
                for y in 0..self.h as usize {
                    let col = self.h as usize - 1 - y;
                    let (cb, cm) = (col >> 3, 0x80u8 >> (col & 7));
                    let row = &self.bits[y * self.stride..(y + 1) * self.stride];
                    for (b, &v) in row.iter().enumerate() {
                        if v == 0 {
                            continue;
                        }
                        for j in 0..8 {
                            if v & (0x80 >> j) != 0 {
                                let x = b * 8 + j;
                                if x < self.w as usize {
                                    out.bits[x * os + cb] |= cm;
                                }
                            }
                        }
                    }
                }
                out
            }
            Rotation::Ccw90 => {
                // Source row y becomes output column y, read bottom to top.
                let mut out = Frame::new(self.h, self.w);
                let os = out.stride;
                for y in 0..self.h as usize {
                    let (cb, cm) = (y >> 3, 0x80u8 >> (y & 7));
                    let row = &self.bits[y * self.stride..(y + 1) * self.stride];
                    for (b, &v) in row.iter().enumerate() {
                        if v == 0 {
                            continue;
                        }
                        for j in 0..8 {
                            if v & (0x80 >> j) != 0 {
                                let x = b * 8 + j;
                                if x < self.w as usize {
                                    let oy = self.w as usize - 1 - x;
                                    out.bits[oy * os + cb] |= cm;
                                }
                            }
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

    /// Apply `f(byte, mask)` to every byte of row `y` touched by the span `x0..x1` (already
    /// clipped), where the mask selects the span's pixels within that byte.
    #[inline]
    fn span_op(&mut self, y: i32, x0: i32, x1: i32, f: impl Fn(u8, u8) -> u8) {
        let row = y as usize * self.stride;
        let (b0, b1) = ((x0 as usize) >> 3, ((x1 - 1) as usize) >> 3);
        let (m0, m1) = edge_masks(x0, x1);
        if b0 == b1 {
            let d = &mut self.bits[row + b0];
            *d = f(*d, m0 & m1);
            return;
        }
        let d = &mut self.bits[row + b0];
        *d = f(*d, m0);
        for d in &mut self.bits[row + b0 + 1..row + b1] {
            *d = f(*d, 0xFF);
        }
        let d = &mut self.bits[row + b1];
        *d = f(*d, m1);
    }

    #[inline]
    fn fill_span(&mut self, y: i32, x0: i32, x1: i32, on: bool) {
        // x0 inclusive, x1 exclusive, both already clipped.
        let row = y as usize * self.stride;
        let (b0, b1) = ((x0 as usize) >> 3, ((x1 - 1) as usize) >> 3);
        let (m0, m1) = edge_masks(x0, x1);
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
        let (m0, m1) = edge_masks(x0, x1);
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

/// Masks selecting the pixels of the span `x0..x1` (exclusive, non-empty, non-negative)
/// inside its first and last bytes.
#[inline]
const fn edge_masks(x0: i32, x1: i32) -> (u8, u8) {
    (0xFF >> (x0 & 7), (0xFF00u16 >> (((x1 - 1) & 7) + 1)) as u8)
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

    /// A small pseudo-random frame with some structure (runs, isolated pixels).
    fn noisy(w: u32, h: u32, seed: u32) -> Frame {
        let mut f = Frame::new(w, h);
        let mut x = seed | 1;
        for b in f.bits.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = if x & 3 == 0 { 0 } else { (x >> 8) as u8 };
        }
        // Clear the padding bits past the width so comparisons are exact.
        for y in 0..h as i32 {
            for x in w as i32..(f.stride as i32 * 8) {
                let i = y as usize * f.stride + (x as usize >> 3);
                f.bits[i] &= !(0x80 >> (x & 7));
            }
        }
        f
    }

    fn blit_reference(f: &mut Frame, x: i32, y: i32, bm: BitmapRef<'_>, mode: BlitMode) {
        for sy in 0..bm.h {
            for sx in 0..bm.w {
                let (dx, dy) = (x + sx as i32, y + sy as i32);
                let on = bm.get(sx, sy);
                match mode {
                    BlitMode::Or => {
                        if on {
                            f.set(dx, dy, Ink::Black)
                        }
                    }
                    BlitMode::Clear => {
                        if on {
                            f.set(dx, dy, Ink::White)
                        }
                    }
                    BlitMode::Xor => {
                        if on {
                            f.flip(dx, dy)
                        }
                    }
                    BlitMode::Copy => f.set(dx, dy, if on { Ink::Black } else { Ink::White }),
                }
            }
        }
    }

    #[test]
    fn bytewise_blit_matches_per_pixel() {
        let src = noisy(37, 11, 7);
        for (i, &(x, y)) in [(0, 0), (3, 2), (-5, -3), (13, 5), (60, 1), (-40, 0), (7, 40)].iter().enumerate() {
            for mode in [BlitMode::Or, BlitMode::Clear, BlitMode::Xor, BlitMode::Copy] {
                let base = noisy(70, 30, 100 + i as u32);
                let mut fast = base.clone();
                let mut slow = base.clone();
                fast.blit(x, y, src.as_bitmap(), mode);
                blit_reference(&mut slow, x, y, src.as_bitmap(), mode);
                assert_eq!(fast, slow, "blit at ({x},{y}) {mode:?}");
            }
        }
    }

    #[test]
    fn bytewise_patterns_and_copies_match_per_pixel() {
        let pats = [Pattern::Dots50, Pattern::Dots25, Pattern::Sparse, Pattern::Hatch { pitch: 6 }, Pattern::Solid];
        for (i, r) in
            [Rect::new(0, 0, 70, 30), Rect::new(3, 1, 20, 9), Rect::new(5, 2, 2, 3), Rect::new(-4, -4, 100, 100)].iter().enumerate()
        {
            for p in pats {
                let base = noisy(70, 30, 9 + i as u32);
                let mut fast = base.clone();
                let mut slow = base.clone();
                fast.screen_rect(*r, p);
                let rr = r.intersect(&slow.bounds());
                for y in rr.y..rr.bottom() {
                    for x in rr.x..rr.right() {
                        if p.ink_at(x, y) {
                            slow.set(x, y, Ink::Black);
                        }
                    }
                }
                assert_eq!(fast, slow, "screen_rect {r:?} {p:?}");
                let mut fast = base.clone();
                let mut slow = base.clone();
                fast.pattern_rect(*r, p);
                for y in rr.y..rr.bottom() {
                    for x in rr.x..rr.right() {
                        slow.set(x, y, if p.ink_at(x, y) { Ink::Black } else { Ink::White });
                    }
                }
                assert_eq!(fast, slow, "pattern_rect {r:?} {p:?}");
            }
            let src = noisy(70, 30, 77);
            let base = noisy(70, 30, 5 + i as u32);
            let mut fast = base.clone();
            let mut slow = base.clone();
            fast.copy_rect_from(&src, *r);
            let rr = r.intersect(&slow.bounds());
            for y in rr.y..rr.bottom() {
                for x in rr.x..rr.right() {
                    slow.set(x, y, if src.get(x, y) { Ink::Black } else { Ink::White });
                }
            }
            assert_eq!(fast, slow, "copy_rect_from {r:?}");
        }
    }

    #[test]
    fn bytewise_rotations_match_per_pixel() {
        for (w, h) in [(64u32, 24u32), (37, 11), (8, 8)] {
            let f = noisy(w, h, w * 31 + h);
            let mut cw = Frame::new(h, w);
            let mut ccw = Frame::new(h, w);
            let mut flip = Frame::new(w, h);
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    if f.get(x, y) {
                        cw.set(h as i32 - 1 - y, x, Ink::Black);
                        ccw.set(y, w as i32 - 1 - x, Ink::Black);
                        flip.set(w as i32 - 1 - x, h as i32 - 1 - y, Ink::Black);
                    }
                }
            }
            assert_eq!(f.rotated(Rotation::Cw90), cw, "cw {w}x{h}");
            assert_eq!(f.rotated(Rotation::Ccw90), ccw, "ccw {w}x{h}");
            assert_eq!(f.rotated(Rotation::Flip180), flip, "flip {w}x{h}");
        }
    }

    #[test]
    fn blit_ccw_matches_rotated_blit() {
        let src = noisy(21, 9, 3);
        let mut a = Frame::new(40, 40);
        a.blit_ccw(5, 7, src.as_bitmap(), BlitMode::Or);
        let mut b = Frame::new(40, 40);
        let rot = src.rotated(Rotation::Ccw90);
        b.blit(5, 7, rot.as_bitmap(), BlitMode::Or);
        assert_eq!(a, b);
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

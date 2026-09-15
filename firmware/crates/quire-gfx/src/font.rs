//! Bitmap font packs ("QFP1"): pre-rasterised strikes baked at build time from the OFL
//! TTFs in `quire-fonts`, so the device never rasterises outlines at runtime.
//!
//! Layout (all little-endian):
//!
//! ```text
//! 0   "QFP1"
//! 4   u16 px size          6   i16 ascent (px)     8   i16 descent (px, negative)
//! 10  i16 line gap (px)    12  u16 glyph count     14  u16 kern pair count
//! 16  u32 bitmap bytes     20  glyph table         ... kern table ... bitmap data
//! glyph (16 B): u32 codepoint, u16 advance (1/4 px), i8 bearing x, i8 top (px above
//!               baseline), u8 w, u8 h, u16 reserved, u32 bitmap offset
//! kern (6 B):   u16 left codepoint, u16 right codepoint, i16 adjust (1/4 px)
//! ```
//!
//! Glyphs are sorted by codepoint and kern pairs by (left, right), so lookups are binary
//! searches over the raw bytes and a [`Font`] is just a reference to flash.

use crate::frame::BitmapRef;

/// Header length in bytes.
const HDR: usize = 20;
const GLYPH: usize = 16;
const KERN: usize = 6;

/// A parsed reference to a font pack. Cheap to copy; owns nothing.
#[derive(Clone, Copy, Debug)]
pub struct Font {
    data: &'static [u8],
}

/// One glyph's metrics and bitmap.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    /// Horizontal advance in quarter pixels.
    pub advance_q: u16,
    /// Left side bearing in pixels.
    pub bearing_x: i8,
    /// Distance from the baseline up to the bitmap's top row, in pixels.
    pub top: i8,
    /// The packed bitmap.
    pub bitmap: BitmapRef<'static>,
}

impl Glyph {
    /// Advance rounded to whole pixels.
    #[inline]
    pub fn advance(&self) -> i32 {
        ((self.advance_q as i32) + 2) >> 2
    }
}

#[inline]
const fn rd_u16(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}
#[inline]
const fn rd_i16(d: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([d[at], d[at + 1]])
}
#[inline]
const fn rd_u32(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}

impl Font {
    /// Wrap a pack. Validates the magic and table sizes; panics on a malformed pack because
    /// packs are compiled into the firmware and a bad one is a build error, not user input.
    pub const fn from_bytes(data: &'static [u8]) -> Font {
        assert!(data.len() >= HDR, "font pack too short");
        assert!(data[0] == b'Q' && data[1] == b'F' && data[2] == b'P' && data[3] == b'1', "not a QFP1 pack");
        let glyphs = rd_u16(data, 12) as usize;
        let kerns = rd_u16(data, 14) as usize;
        let bitmap = rd_u32(data, 16) as usize;
        assert!(data.len() == HDR + glyphs * GLYPH + kerns * KERN + bitmap, "font pack size mismatch");
        Font { data }
    }

    /// Nominal pixel size (the em size the strike was rasterised at).
    #[inline]
    pub const fn size(&self) -> u16 {
        rd_u16(self.data, 4)
    }
    /// Ascent above the baseline in pixels.
    #[inline]
    pub const fn ascent(&self) -> i32 {
        rd_i16(self.data, 6) as i32
    }
    /// Descent below the baseline in pixels (negative).
    #[inline]
    pub const fn descent(&self) -> i32 {
        rd_i16(self.data, 8) as i32
    }
    /// Descent below the baseline as a positive number of pixels (what layout code adds).
    #[inline]
    pub const fn below(&self) -> i32 {
        -self.descent()
    }
    /// Recommended line gap.
    #[inline]
    pub const fn line_gap(&self) -> i32 {
        rd_i16(self.data, 10) as i32
    }
    /// Natural line height (ascent − descent).
    #[inline]
    pub const fn height(&self) -> i32 {
        self.ascent() - self.descent()
    }
    /// Size of the pack in bytes (what it costs in flash).
    #[inline]
    pub const fn byte_len(&self) -> usize {
        self.data.len()
    }

    /// Number of glyphs in the pack.
    #[inline]
    pub const fn glyph_count(&self) -> usize {
        rd_u16(self.data, 12) as usize
    }
    #[inline]
    const fn kern_count(&self) -> usize {
        rd_u16(self.data, 14) as usize
    }
    #[inline]
    const fn kern_base(&self) -> usize {
        HDR + self.glyph_count() * GLYPH
    }
    #[inline]
    const fn bitmap_base(&self) -> usize {
        self.kern_base() + self.kern_count() * KERN
    }

    /// Look up a glyph by codepoint.
    pub fn glyph(&self, c: char) -> Option<Glyph> {
        let cp = c as u32;
        let (mut lo, mut hi) = (0usize, self.glyph_count());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let at = HDR + mid * GLYPH;
            let k = rd_u32(self.data, at);
            if k == cp {
                return Some(self.glyph_at(at));
            } else if k < cp {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        None
    }

    /// The glyph for `c`, or the pack's replacement glyph (`?`, then space).
    pub fn glyph_or_fallback(&self, c: char) -> Glyph {
        self.glyph(c).or_else(|| self.glyph('?')).or_else(|| self.glyph(' ')).unwrap_or_else(|| self.glyph_at(HDR))
    }

    fn glyph_at(&self, at: usize) -> Glyph {
        let d = self.data;
        let w = d[at + 8] as u32;
        let h = d[at + 9] as u32;
        let off = self.bitmap_base() + rd_u32(d, at + 12) as usize;
        let len = (w as usize).div_ceil(8) * h as usize;
        Glyph {
            advance_q: rd_u16(d, at + 4),
            bearing_x: d[at + 6] as i8,
            top: d[at + 7] as i8,
            bitmap: BitmapRef { w, h, bits: &d[off..off + len] },
        }
    }

    /// Kerning adjustment between two characters, in quarter pixels.
    pub fn kern_q(&self, left: char, right: char) -> i32 {
        let (l, r) = (left as u32, right as u32);
        if l > 0xFFFF || r > 0xFFFF {
            return 0;
        }
        let key = (l << 16) | r;
        let base = self.kern_base();
        let (mut lo, mut hi) = (0usize, self.kern_count());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let at = base + mid * KERN;
            let k = ((rd_u16(self.data, at) as u32) << 16) | rd_u16(self.data, at + 2) as u32;
            if k == key {
                return rd_i16(self.data, at + 4) as i32;
            } else if k < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        0
    }

    /// Whether the pack contains a glyph for the character.
    pub fn has(&self, c: char) -> bool {
        self.glyph(c).is_some()
    }
}

/// Build a pack in memory. Used by the build-time generator and by tests; the device
/// only ever reads packs.
#[cfg(feature = "std")]
pub mod build {
    extern crate std;
    use std::vec::Vec;

    /// A glyph to pack.
    pub struct GlyphSpec {
        /// Codepoint.
        pub cp: u32,
        /// Advance in quarter pixels.
        pub advance_q: u16,
        /// Left bearing, px.
        pub bearing_x: i8,
        /// Top above baseline, px.
        pub top: i8,
        /// Bitmap width.
        pub w: u8,
        /// Bitmap height.
        pub h: u8,
        /// Packed rows.
        pub bits: Vec<u8>,
    }

    /// Serialise a pack. `glyphs` need not be sorted; `kerns` are (left, right, quarter px).
    pub fn pack(
        size: u16,
        ascent: i16,
        descent: i16,
        line_gap: i16,
        mut glyphs: Vec<GlyphSpec>,
        mut kerns: Vec<(u16, u16, i16)>,
    ) -> Vec<u8> {
        glyphs.sort_by_key(|g| g.cp);
        glyphs.dedup_by_key(|g| g.cp);
        kerns.sort_by_key(|k| ((k.0 as u32) << 16) | k.1 as u32);
        kerns.dedup_by_key(|k| ((k.0 as u32) << 16) | k.1 as u32);
        let mut bitmap: Vec<u8> = Vec::new();
        let mut table: Vec<u8> = Vec::with_capacity(glyphs.len() * 16);
        for g in &glyphs {
            let off = bitmap.len() as u32;
            let stride = (g.w as usize).div_ceil(8);
            assert_eq!(g.bits.len(), stride * g.h as usize, "glyph {:#x} bitmap size", g.cp);
            bitmap.extend_from_slice(&g.bits);
            table.extend_from_slice(&g.cp.to_le_bytes());
            table.extend_from_slice(&g.advance_q.to_le_bytes());
            table.push(g.bearing_x as u8);
            table.push(g.top as u8);
            table.push(g.w);
            table.push(g.h);
            table.extend_from_slice(&0u16.to_le_bytes());
            table.extend_from_slice(&off.to_le_bytes());
        }
        let mut out = Vec::with_capacity(20 + table.len() + kerns.len() * 6 + bitmap.len());
        out.extend_from_slice(b"QFP1");
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&ascent.to_le_bytes());
        out.extend_from_slice(&descent.to_le_bytes());
        out.extend_from_slice(&line_gap.to_le_bytes());
        out.extend_from_slice(&(glyphs.len() as u16).to_le_bytes());
        out.extend_from_slice(&(kerns.len() as u16).to_le_bytes());
        out.extend_from_slice(&(bitmap.len() as u32).to_le_bytes());
        out.extend_from_slice(&table);
        for (l, r, a) in &kerns {
            out.extend_from_slice(&l.to_le_bytes());
            out.extend_from_slice(&r.to_le_bytes());
            out.extend_from_slice(&a.to_le_bytes());
        }
        out.extend_from_slice(&bitmap);
        out
    }
}

//! The image pipeline: decode (JPEG/PNG/BMP) → scale to a target box → dither to 1 bit,
//! all row-streamed so a full-page cover costs about 16 KB of working memory.

use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_gfx::Bitmap;

use crate::DocError;

/// What the bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    /// Baseline or progressive JPEG (progressive is rejected on the device).
    Jpeg,
    /// PNG.
    Png,
    /// Windows BMP (uncompressed 1/8/24/32-bit).
    Bmp,
    /// Unknown: sniff.
    Unknown,
}

impl ImageKind {
    /// From a media type or file extension.
    pub fn from_hint(hint: &str) -> ImageKind {
        let h = hint.to_ascii_lowercase();
        if h.contains("jpeg") || h.contains("jpg") {
            ImageKind::Jpeg
        } else if h.contains("png") {
            ImageKind::Png
        } else if h.contains("bmp") {
            ImageKind::Bmp
        } else {
            ImageKind::Unknown
        }
    }
    /// From magic bytes.
    pub fn sniff(head: &[u8]) -> ImageKind {
        if head.starts_with(b"\xFF\xD8\xFF") {
            ImageKind::Jpeg
        } else if head.starts_with(b"\x89PNG") {
            ImageKind::Png
        } else if head.starts_with(b"BM") {
            ImageKind::Bmp
        } else {
            ImageKind::Unknown
        }
    }
}

/// How to fit the source into the target box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fit {
    /// Maximum width.
    pub max_w: u32,
    /// Maximum height.
    pub max_h: u32,
    /// Crop to fill the box instead of fitting inside it (covers).
    pub cover: bool,
    /// Use Floyd–Steinberg (true) or ordered dithering (false, faster, for thumbnails).
    pub fs: bool,
}

impl Fit {
    /// Fit inside a box, Floyd–Steinberg.
    pub const fn inside(max_w: u32, max_h: u32) -> Fit {
        Fit { max_w, max_h, cover: false, fs: true }
    }
    /// Fill a box (crop), Floyd–Steinberg.
    pub const fn fill(max_w: u32, max_h: u32) -> Fit {
        Fit { max_w, max_h, cover: true, fs: true }
    }
}

/// Receives decoded grey rows (0 = black, 255 = white), scaled and dithered incrementally.
pub struct RowSink {
    src_w: u32,
    src_h: u32,
    out_w: u32,
    out_h: u32,
    crop_x: u32,
    crop_y: u32,
    crop_w: u32,
    crop_h: u32,
    /// Accumulator for the current output row: sum and count per output column.
    acc: Vec<u32>,
    cnt: Vec<u32>,
    rows_in_acc: u32,
    next_out_row: u32,
    err_cur: Vec<i16>,
    err_next: Vec<i16>,
    fs: bool,
    out: Bitmap,
    src_row: u32,
}

impl RowSink {
    /// Plan the scale for a source of `src_w × src_h`.
    pub fn new(src_w: u32, src_h: u32, fit: Fit) -> Result<RowSink, DocError> {
        if src_w == 0 || src_h == 0 {
            return Err(DocError::Malformed("empty image"));
        }
        if src_w as u64 * src_h as u64 > crate::limits::IMAGE_PIXELS as u64 {
            return Err(DocError::TooLarge("image dimensions"));
        }
        let (max_w, max_h) = (fit.max_w.max(1), fit.max_h.max(1));
        let (crop_x, crop_y, crop_w, crop_h, out_w, out_h);
        if fit.cover {
            // Scale so the box is covered, then crop centre.
            let scale_num_w = max_w as u64 * src_h as u64;
            let scale_num_h = max_h as u64 * src_w as u64;
            if scale_num_w >= scale_num_h {
                // width-bound: fit width, crop height
                out_w = max_w;
                out_h = max_h;
                crop_w = src_w;
                crop_h = ((max_h as u64 * src_w as u64) / max_w as u64).clamp(1, src_h as u64) as u32;
                crop_x = 0;
                crop_y = (src_h - crop_h) / 2;
            } else {
                out_w = max_w;
                out_h = max_h;
                crop_h = src_h;
                crop_w = ((max_w as u64 * src_h as u64) / max_h as u64).clamp(1, src_w as u64) as u32;
                crop_y = 0;
                crop_x = (src_w - crop_w) / 2;
            }
        } else {
            crop_x = 0;
            crop_y = 0;
            crop_w = src_w;
            crop_h = src_h;
            if src_w <= max_w && src_h <= max_h {
                out_w = src_w;
                out_h = src_h;
            } else {
                let by_w = (max_w as u64, (src_h as u64 * max_w as u64) / src_w as u64);
                let by_h = ((src_w as u64 * max_h as u64) / src_h as u64, max_h as u64);
                let (w, h) = if by_w.1 <= max_h as u64 { by_w } else { by_h };
                out_w = (w as u32).max(1);
                out_h = (h as u32).max(1);
            }
        }
        Ok(RowSink {
            src_w,
            src_h,
            out_w,
            out_h,
            crop_x,
            crop_y,
            crop_w,
            crop_h,
            acc: alloc::vec![0; out_w as usize],
            cnt: alloc::vec![0; out_w as usize],
            rows_in_acc: 0,
            next_out_row: 0,
            err_cur: alloc::vec![0; out_w as usize + 2],
            err_next: alloc::vec![0; out_w as usize + 2],
            fs: fit.fs,
            out: Bitmap::new(out_w, out_h),
            src_row: 0,
        })
    }

    /// Output size.
    pub fn size(&self) -> (u32, u32) {
        (self.out_w, self.out_h)
    }
    /// Source size.
    pub fn source_size(&self) -> (u32, u32) {
        (self.src_w, self.src_h)
    }

    /// Push one source row of grey values (length `src_w`).
    pub fn push_row(&mut self, row: &[u8]) {
        let y = self.src_row;
        self.src_row += 1;
        if y < self.crop_y || y >= self.crop_y + self.crop_h || self.next_out_row >= self.out_h {
            return;
        }
        let ry = y - self.crop_y;
        // Which output row does this source row belong to?
        let out_row = (ry as u64 * self.out_h as u64 / self.crop_h as u64) as u32;
        if out_row > self.next_out_row {
            self.flush_row();
        }
        let row = &row[self.crop_x as usize..(self.crop_x + self.crop_w) as usize];
        // Box-accumulate horizontally.
        if self.crop_w == self.out_w {
            for (x, &v) in row.iter().enumerate() {
                self.acc[x] += v as u32;
                self.cnt[x] += 1;
            }
        } else {
            for (sx, &v) in row.iter().enumerate() {
                let ox = (sx as u64 * self.out_w as u64 / self.crop_w as u64) as usize;
                self.acc[ox] += v as u32;
                self.cnt[ox] += 1;
            }
        }
        self.rows_in_acc += 1;
    }

    fn flush_row(&mut self) {
        if self.next_out_row >= self.out_h {
            return;
        }
        let y = self.next_out_row;
        let w = self.out_w as usize;
        if self.fs {
            self.err_next.iter_mut().for_each(|e| *e = 0);
            for x in 0..w {
                let g = self.acc[x].checked_div(self.cnt[x]).map(|v| v as i32).unwrap_or(255);
                let v = g + self.err_cur[x + 1] as i32;
                let (q, e) = if v < 128 { (0, v) } else { (255, v - 255) };
                if q == 0 {
                    self.out.set(x as u32, y, true);
                }
                let e = e.clamp(-255, 255) as i16;
                self.err_cur[x + 2] = self.err_cur[x + 2].saturating_add(e * 7 / 16);
                self.err_next[x] = self.err_next[x].saturating_add(e * 3 / 16);
                self.err_next[x + 1] = self.err_next[x + 1].saturating_add(e * 5 / 16);
                self.err_next[x + 2] = self.err_next[x + 2].saturating_add(e / 16);
            }
            core::mem::swap(&mut self.err_cur, &mut self.err_next);
        } else {
            const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
            for x in 0..w {
                let g = self.acc[x].checked_div(self.cnt[x]).map(|v| v as u8).unwrap_or(255);
                let t = (BAYER4[y as usize & 3][x & 3] as u32 * 16 + 8) as u8;
                if g < t {
                    self.out.set(x as u32, y, true);
                }
            }
        }
        self.acc.iter_mut().for_each(|a| *a = 0);
        self.cnt.iter_mut().for_each(|c| *c = 0);
        self.rows_in_acc = 0;
        self.next_out_row += 1;
    }

    /// Finish and take the bitmap.
    pub fn finish(mut self) -> Bitmap {
        while self.next_out_row < self.out_h {
            self.flush_row();
        }
        self.out
    }
}

/// Decode any supported image into a fitted 1-bit bitmap.
pub fn decode<R: ReadAt>(src: &R, kind: ImageKind, fit: Fit) -> Result<Bitmap, DocError> {
    let mut head = [0u8; 16];
    let n = src.read_at(0, &mut head)?;
    let kind = match kind {
        ImageKind::Unknown => ImageKind::sniff(&head[..n]),
        k => {
            // Trust magic over hints when they disagree.
            let s = ImageKind::sniff(&head[..n]);
            if s == ImageKind::Unknown {
                k
            } else {
                s
            }
        }
    };
    match kind {
        ImageKind::Jpeg => crate::jpeg::decode(src, fit),
        ImageKind::Png => crate::png::decode(src, fit),
        ImageKind::Bmp => decode_bmp(src, fit),
        ImageKind::Unknown => Err(DocError::Unsupported("image format")),
    }
}

/// Uncompressed BMP (1, 8, 24, 32 bpp), bottom-up or top-down.
pub fn decode_bmp<R: ReadAt>(src: &R, fit: Fit) -> Result<Bitmap, DocError> {
    let mut h = [0u8; 54];
    src.read_exact_at(0, &mut h)?;
    if &h[..2] != b"BM" {
        return Err(DocError::Malformed("bmp"));
    }
    let data_off = u32::from_le_bytes([h[10], h[11], h[12], h[13]]) as u64;
    let w = i32::from_le_bytes([h[18], h[19], h[20], h[21]]);
    let hgt = i32::from_le_bytes([h[22], h[23], h[24], h[25]]);
    let bpp = u16::from_le_bytes([h[28], h[29]]) as u32;
    let compression = u32::from_le_bytes([h[30], h[31], h[32], h[33]]);
    if compression != 0 && !(compression == 3 && bpp == 32) {
        return Err(DocError::Unsupported("compressed bmp"));
    }
    let top_down = hgt < 0;
    let (w, hgt) = (w.unsigned_abs(), hgt.unsigned_abs());
    let mut sink = RowSink::new(w, hgt, fit)?;
    let stride = ((w * bpp).div_ceil(32) * 4) as u64;
    let mut palette = Vec::new();
    if bpp <= 8 {
        let hdr_size = u32::from_le_bytes([h[14], h[15], h[16], h[17]]) as u64;
        let ncol = 1usize << bpp;
        let pal = src.read_range(14 + hdr_size, ncol * 4)?;
        palette = pal.chunks(4).map(|c| ((c[2] as u32 * 299 + c[1] as u32 * 587 + c[0] as u32 * 114) / 1000) as u8).collect();
    }
    let mut rowbuf = alloc::vec![0u8; stride as usize];
    let mut grey = alloc::vec![0u8; w as usize];
    for i in 0..hgt {
        let row_index = if top_down { i } else { hgt - 1 - i };
        src.read_exact_at(data_off + row_index as u64 * stride, &mut rowbuf)?;
        for x in 0..w as usize {
            grey[x] = match bpp {
                1 => palette[((rowbuf[x / 8] >> (7 - (x & 7))) & 1) as usize],
                8 => palette[rowbuf[x] as usize],
                24 => ((rowbuf[x * 3 + 2] as u32 * 299 + rowbuf[x * 3 + 1] as u32 * 587 + rowbuf[x * 3] as u32 * 114) / 1000) as u8,
                32 => ((rowbuf[x * 4 + 2] as u32 * 299 + rowbuf[x * 4 + 1] as u32 * 587 + rowbuf[x * 4] as u32 * 114) / 1000) as u8,
                _ => return Err(DocError::Unsupported("bmp depth")),
            };
        }
        sink.push_row(&grey);
    }
    Ok(sink.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_sink_scales_and_dithers() {
        // 100×50 gradient to a 50×25 box: output is 50×25 with a light-to-dark ramp.
        let mut s = RowSink::new(100, 50, Fit::inside(50, 25)).unwrap();
        assert_eq!(s.size(), (50, 25));
        for _ in 0..50 {
            let row: Vec<u8> = (0..100).map(|x| (255 - x * 2) as u8).collect();
            s.push_row(&row);
        }
        let bm = s.finish();
        let left: u32 = (0..25).map(|y| bm.get(2, y) as u32).sum();
        let right: u32 = (0..25).map(|y| bm.get(47, y) as u32).sum();
        assert!(left < right, "left is light ({left}), right is dark ({right})");
    }

    #[test]
    fn cover_crops_to_the_box_ratio() {
        let s = RowSink::new(1000, 1000, Fit::fill(152, 228)).unwrap();
        assert_eq!(s.size(), (152, 228));
        let s2 = RowSink::new(300, 100, Fit::inside(480, 640)).unwrap();
        assert_eq!(s2.size(), (300, 100), "small images are not upscaled");
    }
}

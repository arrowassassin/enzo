//! PNG decoder, row-streamed through zlib inflate: greyscale, RGB, palette, and alpha
//! (composited over white), bit depths 1–16, non-interlaced. Adam7 is refused; the
//! converter handles it.
//!
//! The chunk walk stops at the first `IDAT`; later `IDAT` chunks are discovered lazily
//! as the inflater asks for more input, so the source is read strictly forward — one
//! pass even when it is a deflated ZIP entry served by [`crate::inflate::InflatedRead`].

use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use quire_fs::ReadAt;
use quire_gfx::Bitmap;

use crate::image::{Fit, RowSink};
use crate::inflate::{Framing, Inflater};
use crate::DocError;

/// Largest legal `PLTE` chunk: 256 entries × RGB.
const PLTE_MAX: u64 = 768;
/// Largest `tRNS` chunk we accept (one alpha byte per palette entry).
const TRNS_MAX: u64 = 256;
/// Widest row we will buffer.
const ROW_MAX: usize = 4 * 1024 * 1024;

/// The IDAT chunks of a PNG, presented as one contiguous `ReadAt`, discovered on demand.
struct Idat<'a, R: ReadAt> {
    src: &'a R,
    /// (offset, len, cumulative start) of each IDAT chunk's data found so far.
    chunks: RefCell<Vec<(u64, u64, u64)>>,
    /// Total IDAT bytes found so far.
    total: Cell<u64>,
    /// Offset of the next chunk header to inspect.
    next_hdr: Cell<u64>,
    /// True once IEND (or the end of the file) was reached.
    done: Cell<bool>,
}

impl<R: ReadAt> Idat<'_, R> {
    /// Scan forward until at least one more IDAT chunk is known or the file ends.
    fn discover(&self) -> quire_fs::FsResult<bool> {
        let len = self.src.len();
        while !self.done.get() {
            let pos = self.next_hdr.get();
            if pos + 8 > len {
                self.done.set(true);
                break;
            }
            let mut hdr = [0u8; 8];
            self.src.read_exact_at(pos, &mut hdr)?;
            let clen = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
            let data = pos + 8;
            self.next_hdr.set(data + clen + 4);
            match &hdr[4..8] {
                b"IDAT" => {
                    let start = self.total.get();
                    let clen = clen.min(len.saturating_sub(data));
                    self.chunks.borrow_mut().push((data, clen, start));
                    self.total.set(start + clen);
                    return Ok(true);
                }
                b"IEND" => self.done.set(true),
                _ => {}
            }
        }
        Ok(false)
    }
}

impl<R: ReadAt> ReadAt for Idat<'_, R> {
    /// An upper bound (the container's length); `read_at` returns 0 at the real end.
    fn len(&self) -> u64 {
        self.src.len()
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> quire_fs::FsResult<usize> {
        while offset >= self.total.get() {
            if !self.discover()? {
                return Ok(0);
            }
        }
        let chunks = self.chunks.borrow();
        let idx = chunks.partition_point(|c| c.2 + c.1 <= offset);
        let (off, len, start) = chunks[idx];
        let inner = offset - start;
        let n = buf.len().min((len - inner) as usize);
        self.src.read_at(off + inner, &mut buf[..n])
    }
}

/// Decode a PNG into a fitted 1-bit bitmap.
pub fn decode<R: ReadAt>(src: &R, fit: Fit) -> Result<Bitmap, DocError> {
    decode_multi(src, &[fit])?.pop().ok_or(DocError::Malformed("png"))
}

/// Decode a PNG once, producing one fitted bitmap per entry of `fits` (a cover and its
/// thumbnail from a single pass over the pixels).
pub fn decode_multi<R: ReadAt>(src: &R, fits: &[Fit]) -> Result<Vec<Bitmap>, DocError> {
    let mut sig = [0u8; 8];
    src.read_exact_at(0, &mut sig)?;
    if sig != [0x89, b'P', b'N', b'G', 13, 10, 26, 10] {
        return Err(DocError::Malformed("png: signature"));
    }
    let mut pos = 8u64;
    let (mut w, mut h, mut depth, mut ctype, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut palette: Vec<u8> = Vec::new(); // grey per entry
    let mut trns: Vec<u8> = Vec::new();
    let mut first_idat: Option<u64> = None;
    let len = src.len();
    while pos + 8 <= len {
        let mut hdr = [0u8; 8];
        src.read_exact_at(pos, &mut hdr)?;
        let clen = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        let ctag = &hdr[4..8];
        let data = pos + 8;
        match ctag {
            b"IHDR" => {
                let b = src.read_range(data, 13)?;
                w = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
                h = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
                depth = b[8];
                ctype = b[9];
                interlace = b[12];
            }
            b"PLTE" => {
                if clen > PLTE_MAX {
                    return Err(DocError::Malformed("png: palette too long"));
                }
                let b = src.read_range(data, clen as usize)?;
                palette = b
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| ((c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114) / 1000) as u8)
                    .collect();
            }
            b"tRNS" => {
                if clen > TRNS_MAX {
                    return Err(DocError::Malformed("png: trns too long"));
                }
                trns = src.read_range(data, clen as usize)?;
            }
            b"IDAT" => {
                // Pixel data starts here; the remaining IDAT chunks are found on demand.
                first_idat = Some(pos);
                break;
            }
            b"IEND" => break,
            _ => {}
        }
        pos = data + clen + 4;
    }
    let Some(first_idat) = first_idat else {
        return Err(DocError::Malformed("png: no image data"));
    };
    if w == 0 || h == 0 {
        return Err(DocError::Malformed("png: no image data"));
    }
    if interlace != 0 {
        return Err(DocError::Unsupported("interlaced PNG (the converter can fix this)"));
    }
    let channels = match ctype {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err(DocError::Malformed("png: colour type")),
    };
    let bits_pp = channels as u32 * depth as u32;
    let stride = ((w as u64 * bits_pp as u64).div_ceil(8)) as usize;
    let bpp = (bits_pp as usize).div_ceil(8).max(1); // bytes per complete pixel, for filters
    if stride > ROW_MAX {
        return Err(DocError::TooLarge("png row"));
    }
    let mut sinks = fits.iter().map(|f| RowSink::new(w, h, *f)).collect::<Result<Vec<_>, _>>()?;
    let idat = Idat { src, chunks: RefCell::new(Vec::new()), total: Cell::new(0), next_hdr: Cell::new(first_idat), done: Cell::new(false) };
    let mut inf = Inflater::new(&idat, Framing::Zlib);
    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut grey = vec![0u8; w as usize];
    let mut filled = 0usize; // bytes of the current row (including filter byte) received
    let mut filter = 0u8;
    let mut row = 0u32;
    let row_len = stride + 1;
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let chunk = inf.next_chunk()?;
        if chunk.is_empty() {
            break;
        }
        pending.extend_from_slice(chunk);
        let mut consumed = 0usize;
        while pending.len() - consumed >= row_len - filled {
            let take = row_len - filled;
            let part = &pending[consumed..consumed + take];
            consumed += take;
            if filled == 0 {
                filter = part[0];
                cur[..take - 1].copy_from_slice(&part[1..]);
            } else {
                cur[filled - 1..filled - 1 + take].copy_from_slice(part);
            }
            filled = 0;
            unfilter(filter, &mut cur, &prev, bpp);
            to_grey(&cur, &mut grey, w, depth, ctype, &palette, &trns);
            for s in sinks.iter_mut() {
                s.push_row(&grey);
            }
            core::mem::swap(&mut prev, &mut cur);
            row += 1;
            if row >= h {
                return Ok(sinks.into_iter().map(RowSink::finish).collect());
            }
        }
        // Partial row remains.
        let rest = pending.len() - consumed;
        if rest > 0 {
            let part = &pending[consumed..];
            if filled == 0 {
                filter = part[0];
                cur[..rest - 1].copy_from_slice(&part[1..]);
            } else {
                cur[filled - 1..filled - 1 + rest].copy_from_slice(part);
            }
            filled += rest;
        }
        pending.clear();
    }
    // Truncated file: keep what decoded.
    Ok(sinks.into_iter().map(RowSink::finish).collect())
}

fn unfilter(filter: u8, cur: &mut [u8], prev: &[u8], bpp: usize) {
    match filter {
        0 => {}
        1 => {
            for i in bpp..cur.len() {
                cur[i] = cur[i].wrapping_add(cur[i - bpp]);
            }
        }
        2 => {
            for i in 0..cur.len() {
                cur[i] = cur[i].wrapping_add(prev[i]);
            }
        }
        3 => {
            for i in 0..cur.len() {
                let a = if i >= bpp { cur[i - bpp] as u16 } else { 0 };
                cur[i] = cur[i].wrapping_add(((a + prev[i] as u16) / 2) as u8);
            }
        }
        4 => {
            for i in 0..cur.len() {
                let a = if i >= bpp { cur[i - bpp] as i16 } else { 0 };
                let b = prev[i] as i16;
                let c = if i >= bpp { prev[i - bpp] as i16 } else { 0 };
                let p = a + b - c;
                let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                let pred = if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                };
                cur[i] = cur[i].wrapping_add(pred as u8);
            }
        }
        _ => {}
    }
}

fn to_grey(row: &[u8], out: &mut [u8], w: u32, depth: u8, ctype: u8, palette: &[u8], trns: &[u8]) {
    let sample = |i: usize| -> u32 {
        // i-th sample of `depth` bits
        match depth {
            8 => row[i] as u32,
            16 => row[i * 2] as u32,
            1 | 2 | 4 => {
                let per = 8 / depth as usize;
                let byte = row[i / per];
                let shift = 8 - depth as usize * (i % per + 1);
                ((byte >> shift) & ((1 << depth) - 1)) as u32
            }
            _ => 0,
        }
    };
    let maxv = if depth == 16 { 255u32 } else { (1u32 << depth) - 1 };
    for (x, out_px) in out.iter_mut().enumerate().take(w as usize) {
        let g = match ctype {
            0 => {
                let v = sample(x);
                (v * 255 / maxv) as u8
            }
            2 => {
                let (r, g, b) = (sample(x * 3), sample(x * 3 + 1), sample(x * 3 + 2));
                ((r * 299 + g * 587 + b * 114) / 1000 * 255 / maxv) as u8
            }
            3 => {
                let idx = sample(x) as usize;
                let g = palette.get(idx).copied().unwrap_or(0);
                let a = trns.get(idx).copied().unwrap_or(255) as u32;
                ((g as u32 * a + 255 * (255 - a)) / 255) as u8
            }
            4 => {
                let v = sample(x * 2) * 255 / maxv;
                let a = sample(x * 2 + 1) * 255 / maxv;
                ((v * a + 255 * (255 - a)) / 255) as u8
            }
            6 => {
                let (r, g, b, a) = (sample(x * 4), sample(x * 4 + 1), sample(x * 4 + 2), sample(x * 4 + 3) * 255 / maxv);
                let v = (r * 299 + g * 587 + b * 114) / 1000 * 255 / maxv;
                ((v * a + 255 * (255 - a)) / 255) as u8
            }
            _ => 255,
        };
        *out_px = g;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    /// Minimal PNG encoder for tests (filter 0 / 1 / 4 rows, any colour type at 8 bits).
    pub(crate) fn encode(w: u32, h: u32, ctype: u8, px: &dyn Fn(u32, u32) -> [u8; 4], filter: u8, split_idat: bool) -> Vec<u8> {
        let ch = match ctype {
            0 => 1,
            2 => 3,
            4 => 2,
            6 => 4,
            _ => 1,
        };
        let mut raw = Vec::new();
        let stride = (w as usize) * ch;
        let mut prev = vec![0u8; stride];
        for y in 0..h {
            let mut row = Vec::with_capacity(stride);
            for x in 0..w {
                let p = px(x, y); // [r, g, b, a]
                match ctype {
                    0 => row.push(p[0]),
                    2 => row.extend_from_slice(&p[..3]),
                    4 => row.extend_from_slice(&[p[0], p[3]]),
                    _ => row.extend_from_slice(&p),
                }
            }
            raw.push(filter);
            match filter {
                0 => raw.extend_from_slice(&row),
                1 => {
                    for i in 0..stride {
                        let a = if i >= ch { row[i - ch] } else { 0 };
                        raw.push(row[i].wrapping_sub(a));
                    }
                }
                4 => {
                    for i in 0..stride {
                        let a = if i >= ch { row[i - ch] as i16 } else { 0 };
                        let b = prev[i] as i16;
                        let c = if i >= ch { prev[i - ch] as i16 } else { 0 };
                        let p = a + b - c;
                        let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                        let pred = if pa <= pb && pa <= pc {
                            a
                        } else if pb <= pc {
                            b
                        } else {
                            c
                        };
                        raw.push(row[i].wrapping_sub(pred as u8));
                    }
                }
                _ => raw.extend_from_slice(&row),
            }
            prev = row;
        }
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
        let mut out = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
        let chunk = |out: &mut Vec<u8>, tag: &[u8], data: &[u8]| {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut c = tag.to_vec();
            c.extend_from_slice(data);
            out.extend_from_slice(&c);
            out.extend_from_slice(&crc32(&c).to_be_bytes());
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, ctype, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        if split_idat {
            let mid = z.len() / 2;
            chunk(&mut out, b"IDAT", &z[..mid]);
            chunk(&mut out, b"IDAT", &z[mid..]);
        } else {
            chunk(&mut out, b"IDAT", &z);
        }
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn grey_rgb_alpha_and_filters_round_trip() {
        for ctype in [0u8, 2, 4, 6] {
            for filter in [0u8, 1, 4] {
                for split in [false, true] {
                    let png = encode(40, 30, ctype, &|x, _| [(x * 6) as u8, (x * 6) as u8, (x * 6) as u8, 255], filter, split);
                    let bm = decode(&png, Fit::inside(40, 30)).unwrap_or_else(|e| panic!("ctype {ctype} filter {filter}: {e}"));
                    assert_eq!((bm.w, bm.h), (40, 30));
                    let left: u32 = (0..30).map(|y| bm.get(1, y) as u32).sum();
                    let right: u32 = (0..30).map(|y| bm.get(38, y) as u32).sum();
                    assert!(left > 25 && right < 5, "ctype {ctype} filter {filter} split {split}: left {left} right {right}");
                }
            }
        }
    }

    #[test]
    fn alpha_composites_over_white() {
        let png = encode(8, 8, 6, &|_, _| [0, 0, 0, 0], 0, false); // fully transparent black
        let bm = decode(&png, Fit::inside(8, 8)).unwrap();
        assert_eq!(bm.bits.iter().map(|b| b.count_ones()).sum::<u32>(), 0, "transparent → paper");
    }

    #[test]
    fn real_png_from_childrens_literature() {
        let data = include_bytes!("../fixtures/childrens-literature.epub");
        let z = crate::zip::Zip::open(&data[..]).unwrap();
        let e = z.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(".png")).cloned();
        if let Some(e) = e {
            let bytes = z.read(&e, 4 << 20).unwrap();
            let bm = decode(&bytes, Fit::inside(480, 640)).expect("real png");
            assert!(bm.w > 0 && bm.h > 0);
            // Straight from the entry reader: the same bitmap.
            let r = z.entry_reader(&e).unwrap();
            let bm2 = decode(&r, Fit::inside(480, 640)).expect("streamed png");
            assert_eq!(bm, bm2);
            // Through a forward-only inflated stream: still identical, and never rewound
            // (the chunk walk stops at the first IDAT; later IDATs are found lazily).
            let z2 = miniz_oxide::deflate::compress_to_vec(&bytes, 6);
            let s = crate::inflate::InflatedRead::new(&z2[..], crate::inflate::Framing::Raw, bytes.len() as u64);
            assert_eq!(decode(&s, Fit::inside(480, 640)).expect("forward-only png"), bm);
            assert_eq!(s.restarts(), 0, "png decode reads forward only");
        }
    }

    #[test]
    fn multi_decode_matches_separate_decodes() {
        let png = encode(300, 200, 2, &|x, y| [((x * 7 + y * 3) % 256) as u8, (y % 256) as u8, (x % 256) as u8, 255], 4, true);
        let fits = [Fit::inside(480, 640), Fit::fill(152, 228), Fit { fs: false, ..Fit::fill(152, 228) }];
        let together = decode_multi(&png, &fits).unwrap();
        assert_eq!(together.len(), 3);
        for (bm, fit) in together.iter().zip(fits) {
            assert_eq!(*bm, decode(&png, fit).unwrap());
        }
        assert!(decode_multi(&png, &[]).unwrap().is_empty());
    }

    #[test]
    fn oversized_palette_and_trns_are_errors_not_allocations() {
        let mut png = encode(4, 4, 0, &|_, _| [0, 0, 0, 255], 0, false);
        // Insert a PLTE chunk claiming 16 MB right after IHDR (offset 8 + 25).
        let mut plte = Vec::new();
        plte.extend_from_slice(&(16u32 << 20).to_be_bytes());
        plte.extend_from_slice(b"PLTE");
        let at = 8 + 25;
        let tail = png.split_off(at);
        png.extend_from_slice(&plte);
        png.extend_from_slice(&tail);
        assert!(matches!(decode(&png, Fit::inside(4, 4)), Err(DocError::Malformed(_))));
        png[at + 4..at + 8].copy_from_slice(b"tRNS");
        assert!(matches!(decode(&png, Fit::inside(4, 4)), Err(DocError::Malformed(_))));
    }

    #[test]
    fn junk_is_an_error() {
        assert!(decode(&b"\x89PNG\r\n\x1a\nxxxx"[..].to_vec(), Fit::inside(8, 8)).is_err());
    }
}

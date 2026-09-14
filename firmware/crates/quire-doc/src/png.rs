//! PNG decoder, row-streamed through zlib inflate: greyscale, RGB, palette, and alpha
//! (composited over white), bit depths 1–16, non-interlaced. Adam7 is refused; the
//! converter handles it.

use alloc::vec;
use alloc::vec::Vec;
use quire_fs::{ReadAt, Slice};
use quire_gfx::Bitmap;

use crate::image::{Fit, RowSink};
use crate::inflate::{Framing, Inflater};
use crate::DocError;

/// The IDAT chunks of a PNG, presented as one contiguous `ReadAt`.
struct Idat<'a, R: ReadAt> {
    src: &'a R,
    /// (offset, len) of each chunk's data, and the cumulative start.
    chunks: Vec<(u64, u64, u64)>,
    total: u64,
}

impl<R: ReadAt> ReadAt for Idat<'_, R> {
    fn len(&self) -> u64 {
        self.total
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> quire_fs::FsResult<usize> {
        if offset >= self.total {
            return Ok(0);
        }
        // Find the chunk containing offset.
        let idx = self.chunks.partition_point(|c| c.2 + c.1 <= offset);
        let (off, len, start) = self.chunks[idx];
        let inner = offset - start;
        let n = buf.len().min((len - inner) as usize);
        self.src.read_at(off + inner, &mut buf[..n])
    }
}

/// Decode a PNG into a fitted 1-bit bitmap.
pub fn decode<R: ReadAt>(src: &R, fit: Fit) -> Result<Bitmap, DocError> {
    let mut sig = [0u8; 8];
    src.read_exact_at(0, &mut sig)?;
    if sig != [0x89, b'P', b'N', b'G', 13, 10, 26, 10] {
        return Err(DocError::Malformed("png: signature"));
    }
    let mut pos = 8u64;
    let (mut w, mut h, mut depth, mut ctype, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut palette: Vec<u8> = Vec::new(); // grey per entry
    let mut trns: Vec<u8> = Vec::new();
    let mut chunks: Vec<(u64, u64, u64)> = Vec::new();
    let mut total = 0u64;
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
                let b = src.read_range(data, clen as usize)?;
                palette = b.chunks(3).map(|c| ((c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114) / 1000) as u8).collect();
            }
            b"tRNS" => trns = src.read_range(data, clen as usize)?,
            b"IDAT" => {
                chunks.push((data, clen, total));
                total += clen;
            }
            b"IEND" => break,
            _ => {}
        }
        pos = data + clen + 4;
    }
    if w == 0 || h == 0 || chunks.is_empty() {
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
    if stride > 4 * 1024 * 1024 {
        return Err(DocError::TooLarge("png row"));
    }
    let mut sink = RowSink::new(w, h, fit)?;
    let idat = Idat { src, chunks, total };
    let mut inf = Inflater::new(Slice::new(&idat, 0, total), Framing::Zlib);
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
            sink.push_row(&grey);
            core::mem::swap(&mut prev, &mut cur);
            row += 1;
            if row >= h {
                return Ok(sink.finish());
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
    if row < h {
        // Truncated file: keep what decoded.
    }
    Ok(sink.finish())
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
                let pred = if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c };
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
    for x in 0..w as usize {
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
        out[x] = g;
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
                        let pred = if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c };
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
        }
    }

    #[test]
    fn junk_is_an_error() {
        assert!(decode(&b"\x89PNG\r\n\x1a\nxxxx"[..].to_vec(), Fit::inside(8, 8)).is_err());
    }
}

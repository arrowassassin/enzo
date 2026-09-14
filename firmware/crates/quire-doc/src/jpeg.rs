//! Baseline JPEG decoder, row-streamed, luminance only.
//!
//! Only the Y channel is inverse-transformed (the panel is grey), chroma blocks are
//! entropy-decoded and discarded, and output rows go straight into [`RowSink`] for
//! scaling and dithering. Working memory is one MCU row of luma (width × 16 bytes) plus
//! the Huffman tables, so a 4000 × 3000 photo costs about 64 KB, not 12 MB.
//! Progressive JPEGs are refused with a clear message (the converter handles them).

use alloc::vec;
use alloc::vec::Vec;
use quire_fs::ReadAt;
use quire_gfx::Bitmap;

use crate::image::{Fit, RowSink};
use crate::DocError;

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29,
    22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

#[derive(Clone)]
struct Huffman {
    /// (code length, code) → symbol, stored as lookup by length: for each length 1..=16, the
    /// first code and index into `symbols`.
    mincode: [i32; 17],
    maxcode: [i32; 18],
    valptr: [i32; 17],
    symbols: Vec<u8>,
}

impl Huffman {
    fn build(counts: &[u8; 16], symbols: Vec<u8>) -> Huffman {
        let mut mincode = [0i32; 17];
        let mut maxcode = [-1i32; 18];
        let mut valptr = [0i32; 17];
        let mut code = 0i32;
        let mut k = 0i32;
        for l in 1..=16usize {
            let n = counts[l - 1] as i32;
            valptr[l] = k;
            mincode[l] = code;
            code += n;
            k += n;
            maxcode[l] = if n > 0 { code - 1 } else { -1 };
            code <<= 1;
        }
        maxcode[17] = i32::MAX;
        Huffman { mincode, maxcode, valptr, symbols }
    }
}

struct Component {
    id: u8,
    h: u8,
    v: u8,
    tq: u8,
    td: u8,
    ta: u8,
    dc_pred: i32,
}

/// Bit reader with byte-stuffing and marker detection over chunked input.
struct Bits<'a, R: ReadAt> {
    src: &'a R,
    pos: u64,
    buf: Vec<u8>,
    len: usize,
    idx: usize,
    acc: u32,
    nbits: u32,
    marker_hit: bool,
}

impl<'a, R: ReadAt> Bits<'a, R> {
    fn new(src: &'a R, pos: u64) -> Self {
        Bits { src, pos, buf: vec![0; 4096], len: 0, idx: 0, acc: 0, nbits: 0, marker_hit: false }
    }
    fn next_byte(&mut self) -> Result<u8, DocError> {
        if self.idx >= self.len {
            self.len = self.src.read_at(self.pos, &mut self.buf)?;
            self.idx = 0;
            if self.len == 0 {
                return Err(DocError::Malformed("jpeg: truncated"));
            }
            self.pos += self.len as u64;
        }
        let b = self.buf[self.idx];
        self.idx += 1;
        Ok(b)
    }
    /// Position of the next unread byte in the source.
    fn position(&self) -> u64 {
        self.pos - (self.len - self.idx) as u64
    }
    fn fill(&mut self) -> Result<(), DocError> {
        while self.nbits <= 24 {
            if self.marker_hit {
                self.acc |= 0 << (24 - self.nbits);
                self.nbits += 8;
                continue;
            }
            let mut b = self.next_byte()?;
            if b == 0xFF {
                let b2 = self.next_byte()?;
                if b2 == 0x00 {
                    b = 0xFF;
                } else if (0xD0..=0xD7).contains(&b2) || b2 == 0xFF {
                    // RST marker inside data: the caller handles restarts; feed zeros.
                    self.marker_hit = true;
                    self.idx -= 2;
                    b = 0;
                } else {
                    self.marker_hit = true;
                    self.idx -= 2;
                    b = 0;
                }
            }
            self.acc |= (b as u32) << (24 - self.nbits);
            self.nbits += 8;
        }
        Ok(())
    }
    fn bit(&mut self) -> Result<u32, DocError> {
        if self.nbits == 0 {
            self.fill()?;
        }
        let b = self.acc >> 31;
        self.acc <<= 1;
        self.nbits -= 1;
        Ok(b)
    }
    fn bits(&mut self, n: u32) -> Result<u32, DocError> {
        if n == 0 {
            return Ok(0);
        }
        if self.nbits < n {
            self.fill()?;
        }
        let v = self.acc >> (32 - n);
        self.acc <<= n;
        self.nbits -= n;
        Ok(v)
    }
    fn decode(&mut self, h: &Huffman) -> Result<u8, DocError> {
        let mut code = 0i32;
        for l in 1..=16usize {
            code = (code << 1) | self.bit()? as i32;
            if code <= h.maxcode[l] {
                let idx = h.valptr[l] + code - h.mincode[l];
                return h.symbols.get(idx as usize).copied().ok_or(DocError::Malformed("jpeg: huffman"));
            }
        }
        Err(DocError::Malformed("jpeg: bad huffman code"))
    }
    fn receive_extend(&mut self, s: u32) -> Result<i32, DocError> {
        if s == 0 {
            return Ok(0);
        }
        let v = self.bits(s)? as i32;
        Ok(if v < (1 << (s - 1)) { v - (1 << s) + 1 } else { v })
    }
    /// Skip to just past the next RSTn marker and reset the bit buffer.
    fn restart(&mut self) -> Result<(), DocError> {
        self.acc = 0;
        self.nbits = 0;
        self.marker_hit = false;
        // Find 0xFF 0xDn.
        loop {
            let b = self.next_byte()?;
            if b == 0xFF {
                let b2 = self.next_byte()?;
                if (0xD0..=0xD7).contains(&b2) {
                    return Ok(());
                }
                if b2 == 0xFF {
                    self.idx -= 1;
                }
            }
        }
    }
}

/// Integer 8×8 inverse DCT (separable, 13-bit fixed point), output clamped to 0..=255.
fn idct8x8(coef: &[i32; 64], out: &mut [u8; 64]) {
    // cos table: C[u][x] = c(u) * cos((2x+1) u pi / 16) * 8192 / 2
    const C: [[i32; 8]; 8] = {
        let t = [
            [2896, 2896, 2896, 2896, 2896, 2896, 2896, 2896],
            [4017, 3406, 2276, 799, -799, -2276, -3406, -4017],
            [3784, 1567, -1567, -3784, -3784, -1567, 1567, 3784],
            [3406, -799, -4017, -2276, 2276, 4017, 799, -3406],
            [2896, -2896, -2896, 2896, 2896, -2896, -2896, 2896],
            [2276, -4017, 799, 3406, -3406, -799, 4017, -2276],
            [1567, -3784, 3784, -1567, -1567, 3784, -3784, 1567],
            [799, -2276, 3406, -4017, 4017, -3406, 2276, -799],
        ];
        t
    };
    let mut tmp = [0i32; 64];
    // Rows: tmp[y][x] = sum_u C[u][x] * coef[y][u]
    for y in 0..8 {
        let row = &coef[y * 8..y * 8 + 8];
        if row[1..].iter().all(|&c| c == 0) {
            let v = (row[0] * C[0][0]) >> 3;
            for x in 0..8 {
                tmp[y * 8 + x] = v;
            }
            continue;
        }
        for x in 0..8 {
            let mut s = 0i32;
            for u in 0..8 {
                s += C[u][x] * row[u];
            }
            tmp[y * 8 + x] = s >> 3;
        }
    }
    // Columns.
    for x in 0..8 {
        for y in 0..8 {
            let mut s = 0i32;
            for v in 0..8 {
                s += C[v][y] * tmp[v * 8 + x];
            }
            // Two passes of 1/2 * 8192/... net scale: divide by 2^24 / 8... empirically 2^21
            let val = (s >> 21) + 128;
            out[y * 8 + x] = val.clamp(0, 255) as u8;
        }
    }
}

/// Decode a JPEG into a fitted 1-bit bitmap.
pub fn decode<R: ReadAt>(src: &R, fit: Fit) -> Result<Bitmap, DocError> {
    let mut pos = 0u64;
    let mut hdr = [0u8; 2];
    src.read_exact_at(0, &mut hdr)?;
    if hdr != [0xFF, 0xD8] {
        return Err(DocError::Malformed("jpeg: no SOI"));
    }
    pos += 2;
    let mut qt: [[u16; 64]; 4] = [[1; 64]; 4];
    let mut dc: Vec<Option<Huffman>> = vec![None, None, None, None];
    let mut ac: Vec<Option<Huffman>> = vec![None, None, None, None];
    let mut comps: Vec<Component> = Vec::new();
    let (mut width, mut height) = (0u32, 0u32);
    let mut restart_interval = 0u32;
    let mut progressive = false;
    loop {
        let mut m = [0u8; 2];
        src.read_exact_at(pos, &mut m)?;
        if m[0] != 0xFF {
            return Err(DocError::Malformed("jpeg: marker"));
        }
        let marker = m[1];
        pos += 2;
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0xFF {
            if marker == 0xFF {
                pos -= 1;
            }
            continue;
        }
        if marker == 0xD9 {
            return Err(DocError::Malformed("jpeg: no scan"));
        }
        let mut lb = [0u8; 2];
        src.read_exact_at(pos, &mut lb)?;
        let seg_len = u16::from_be_bytes(lb) as u64;
        if seg_len < 2 {
            return Err(DocError::Malformed("jpeg: segment"));
        }
        let body_off = pos + 2;
        let body_len = (seg_len - 2) as usize;
        match marker {
            0xDB => {
                let b = src.read_range(body_off, body_len)?;
                let mut i = 0;
                while i < b.len() {
                    let pq = b[i] >> 4;
                    let tq = (b[i] & 15) as usize;
                    i += 1;
                    if tq > 3 {
                        return Err(DocError::Malformed("jpeg: dqt"));
                    }
                    for k in 0..64 {
                        let v = if pq == 0 {
                            let v = *b.get(i).ok_or(DocError::Malformed("jpeg: dqt"))? as u16;
                            i += 1;
                            v
                        } else {
                            let v = u16::from_be_bytes([b[i], *b.get(i + 1).ok_or(DocError::Malformed("jpeg: dqt"))?]);
                            i += 2;
                            v
                        };
                        qt[tq][ZIGZAG[k]] = v;
                    }
                }
            }
            0xC4 => {
                let b = src.read_range(body_off, body_len)?;
                let mut i = 0;
                while i + 17 <= b.len() {
                    let tc = b[i] >> 4;
                    let th = (b[i] & 15) as usize;
                    let mut counts = [0u8; 16];
                    counts.copy_from_slice(&b[i + 1..i + 17]);
                    let total: usize = counts.iter().map(|&c| c as usize).sum();
                    i += 17;
                    if i + total > b.len() || th > 3 {
                        return Err(DocError::Malformed("jpeg: dht"));
                    }
                    let symbols = b[i..i + total].to_vec();
                    i += total;
                    let h = Huffman::build(&counts, symbols);
                    if tc == 0 {
                        dc[th] = Some(h);
                    } else {
                        ac[th] = Some(h);
                    }
                }
            }
            0xC0 | 0xC1 => {
                let b = src.read_range(body_off, body_len)?;
                if b.len() < 6 {
                    return Err(DocError::Malformed("jpeg: sof"));
                }
                if b[0] != 8 {
                    return Err(DocError::Unsupported("jpeg: only 8-bit precision"));
                }
                height = u16::from_be_bytes([b[1], b[2]]) as u32;
                width = u16::from_be_bytes([b[3], b[4]]) as u32;
                let nc = b[5] as usize;
                if nc != 1 && nc != 3 {
                    return Err(DocError::Unsupported("jpeg: component count"));
                }
                comps.clear();
                for c in 0..nc {
                    let o = 6 + c * 3;
                    comps.push(Component { id: b[o], h: b[o + 1] >> 4, v: b[o + 1] & 15, tq: b[o + 2] & 3, td: 0, ta: 0, dc_pred: 0 });
                }
            }
            0xC2 | 0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                progressive = true;
            }
            0xDD => {
                let b = src.read_range(body_off, body_len)?;
                restart_interval = u16::from_be_bytes([b[0], b[1]]) as u32;
            }
            0xDA => {
                if progressive {
                    return Err(DocError::Unsupported("progressive JPEG (the converter can fix this)"));
                }
                let b = src.read_range(body_off, body_len)?;
                let ns = b[0] as usize;
                if ns != comps.len() {
                    return Err(DocError::Unsupported("jpeg: non-interleaved scan"));
                }
                for s in 0..ns {
                    let cid = b[1 + s * 2];
                    let t = b[2 + s * 2];
                    if let Some(c) = comps.iter_mut().find(|c| c.id == cid) {
                        c.td = t >> 4;
                        c.ta = t & 15;
                    }
                }
                let data_start = body_off + body_len as u64;
                return decode_scan(src, data_start, width, height, &mut comps, &qt, &dc, &ac, restart_interval, fit);
            }
            _ => {}
        }
        pos = body_off + body_len as u64;
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_scan<R: ReadAt>(
    src: &R,
    start: u64,
    width: u32,
    height: u32,
    comps: &mut [Component],
    qt: &[[u16; 64]; 4],
    dc: &[Option<Huffman>],
    ac: &[Option<Huffman>],
    restart_interval: u32,
    fit: Fit,
) -> Result<Bitmap, DocError> {
    if width == 0 || height == 0 || comps.is_empty() {
        return Err(DocError::Malformed("jpeg: size"));
    }
    let hmax = comps.iter().map(|c| c.h).max().unwrap_or(1).max(1) as u32;
    let vmax = comps.iter().map(|c| c.v).max().unwrap_or(1).max(1) as u32;
    let mcu_w = 8 * hmax;
    let mcu_h = 8 * vmax;
    let mcus_x = width.div_ceil(mcu_w);
    let mcus_y = height.div_ceil(mcu_h);
    let mut sink = RowSink::new(width, height, fit)?;
    // DC-only fast path when the output is at most 1/8 of the source in both axes.
    let (ow, oh) = sink.size();
    let dc_only = ow * 8 <= width && oh * 8 <= height;
    // Luma component (first, or the one with id 1).
    let luma_idx = comps.iter().position(|c| c.id == 1).unwrap_or(0);
    let (lh, lv) = (comps[luma_idx].h.max(1) as u32, comps[luma_idx].v.max(1) as u32);
    // Row buffer for one MCU row of luma at full resolution (or 1 px per block when DC-only).
    let row_px_w = mcus_x * mcu_w;
    let rows_per_mcu = if dc_only { lv } else { mcu_h };
    let buf_w = if dc_only { mcus_x * lh } else { row_px_w };
    let mut rows: Vec<u8> = vec![255; (buf_w * rows_per_mcu) as usize];
    let mut bits = Bits::new(src, start);
    let mut coef = [0i32; 64];
    let mut block = [0u8; 64];
    let mut mcu_count = 0u32;
    let mut grey_row: Vec<u8> = vec![255; width as usize];
    let dc_x_scale = if dc_only { hmax / lh } else { 1 };

    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            if restart_interval > 0 && mcu_count > 0 && mcu_count % restart_interval == 0 {
                bits.restart()?;
                for c in comps.iter_mut() {
                    c.dc_pred = 0;
                }
            }
            mcu_count += 1;
            for (ci, comp) in comps.iter_mut().enumerate() {
                let dct = dc.get(comp.td as usize).and_then(|h| h.as_ref()).ok_or(DocError::Malformed("jpeg: missing dc table"))?;
                let act = ac.get(comp.ta as usize).and_then(|h| h.as_ref()).ok_or(DocError::Malformed("jpeg: missing ac table"))?;
                let q = &qt[comp.tq as usize];
                for by in 0..comp.v.max(1) as u32 {
                    for bx in 0..comp.h.max(1) as u32 {
                        // Decode one block.
                        coef.iter_mut().for_each(|c| *c = 0);
                        let t = bits.decode(dct)?;
                        let diff = bits.receive_extend(t as u32)?;
                        comp.dc_pred += diff;
                        coef[0] = comp.dc_pred * q[0] as i32;
                        let mut k = 1usize;
                        while k < 64 {
                            let rs = bits.decode(act)?;
                            let r = (rs >> 4) as usize;
                            let s = (rs & 15) as u32;
                            if s == 0 {
                                if r == 15 {
                                    k += 16;
                                    continue;
                                }
                                break;
                            }
                            k += r;
                            if k > 63 {
                                break;
                            }
                            let v = bits.receive_extend(s)?;
                            coef[ZIGZAG[k]] = v * q[ZIGZAG[k]] as i32;
                            k += 1;
                        }
                        if ci != luma_idx {
                            continue;
                        }
                        if dc_only {
                            let v = ((coef[0] >> 3) + 128).clamp(0, 255) as u8;
                            let x = mx * lh + bx;
                            let y = by;
                            rows[(y * buf_w + x) as usize] = v;
                        } else {
                            idct8x8(&coef, &mut block);
                            let x0 = mx * mcu_w + bx * 8;
                            let y0 = by * 8;
                            for yy in 0..8usize {
                                let dst = ((y0 + yy as u32) * buf_w + x0) as usize;
                                rows[dst..dst + 8].copy_from_slice(&block[yy * 8..yy * 8 + 8]);
                            }
                        }
                    }
                }
            }
        }
        // Emit this MCU row's luma rows.
        if dc_only {
            // Each stored sample covers (8*hmax/lh) × (8*vmax/lv) source pixels; expand to source rows.
            let px_per_sample_x = 8 * hmax / lh;
            let px_per_sample_y = 8 * vmax / lv;
            for sy in 0..lv {
                let src_row = &rows[(sy * buf_w) as usize..((sy + 1) * buf_w) as usize];
                for x in 0..width as usize {
                    grey_row[x] = src_row[(x as u32 / px_per_sample_x).min(buf_w - 1) as usize];
                }
                for _ in 0..px_per_sample_y {
                    let y_abs = my * mcu_h + sy * px_per_sample_y;
                    if y_abs < height {
                        sink.push_row(&grey_row);
                    }
                }
            }
            let _ = dc_x_scale;
        } else {
            // Luma may be subsampled relative to the MCU (rare: luma is usually the densest).
            let sx = hmax / lh;
            let sy_ = vmax / lv;
            for yy in 0..mcu_h {
                let y_abs = my * mcu_h + yy;
                if y_abs >= height {
                    break;
                }
                let src_y = yy / sy_;
                let src_row = &rows[(src_y * buf_w) as usize..((src_y + 1) * buf_w) as usize];
                if sx == 1 {
                    grey_row.copy_from_slice(&src_row[..width as usize]);
                } else {
                    for x in 0..width as usize {
                        grey_row[x] = src_row[x / sx as usize];
                    }
                }
                sink.push_row(&grey_row);
            }
        }
        rows.iter_mut().for_each(|v| *v = 255);
    }
    let _ = bits.position();
    Ok(sink.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny baseline JPEG encoder for tests: grey only, quality-free (all-ones quant),
    /// standard Huffman tables. Enough to round-trip a gradient through the decoder.
    pub(crate) fn encode_grey(w: u32, h: u32, px: &dyn Fn(u32, u32) -> u8) -> Vec<u8> {
        // Standard luminance Huffman tables (JPEG spec K.3).
        const DC_COUNTS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
        const DC_SYMS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        const AC_COUNTS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
        const AC_SYMS: [u8; 162] = [
            0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
            0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
            0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
            0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
            0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
            0xf9, 0xfa,
        ];
        // Build code tables (length, code) per symbol.
        fn codes(counts: &[u8; 16], syms: &[u8]) -> [(u8, u16); 256] {
            let mut out = [(0u8, 0u16); 256];
            let mut code = 0u16;
            let mut k = 0usize;
            for l in 1..=16u8 {
                for _ in 0..counts[l as usize - 1] {
                    out[syms[k] as usize] = (l, code);
                    code += 1;
                    k += 1;
                }
                code <<= 1;
            }
            out
        }
        let dcc = codes(&DC_COUNTS, &DC_SYMS);
        let acc = codes(&AC_COUNTS, &AC_SYMS);
        struct BW {
            out: Vec<u8>,
            acc: u32,
            n: u32,
        }
        impl BW {
            fn put(&mut self, v: u32, n: u32) {
                for i in (0..n).rev() {
                    self.acc = (self.acc << 1) | ((v >> i) & 1);
                    self.n += 1;
                    if self.n == 8 {
                        self.out.push(self.acc as u8);
                        if self.acc as u8 == 0xFF {
                            self.out.push(0);
                        }
                        self.acc = 0;
                        self.n = 0;
                    }
                }
            }
            fn flush(&mut self) {
                while self.n != 0 {
                    self.put(1, 1);
                }
            }
        }
        let mut bw = BW { out: Vec::new(), acc: 0, n: 0 };
        // Forward DCT (float, test only) with quant = 1 everywhere.
        let mut pred = 0i32;
        let bw_blocks = w.div_ceil(8);
        let bh_blocks = h.div_ceil(8);
        for by in 0..bh_blocks {
            for bx in 0..bw_blocks {
                let mut f = [0f32; 64];
                for v in 0..8 {
                    for u in 0..8 {
                        let mut s = 0f32;
                        for y in 0..8 {
                            for x in 0..8 {
                                let px_v = px((bx * 8 + x).min(w - 1), (by * 8 + y).min(h - 1)) as f32 - 128.0;
                                s += px_v * libm_cos(((2 * x + 1) as f32 * u as f32 * core::f32::consts::PI) / 16.0) * libm_cos(((2 * y + 1) as f32 * v as f32 * core::f32::consts::PI) / 16.0);
                            }
                        }
                        let cu = if u == 0 { core::f32::consts::FRAC_1_SQRT_2 } else { 1.0 };
                        let cv = if v == 0 { core::f32::consts::FRAC_1_SQRT_2 } else { 1.0 };
                        f[v * 8 + u] = 0.25 * cu * cv * s;
                    }
                }
                let q: Vec<i32> = f.iter().map(|x| x.round() as i32).collect();
                // DC
                let diff = q[0] - pred;
                pred = q[0];
                let (s, bitsv) = magnitude(diff);
                let (l, c) = dcc[s as usize];
                bw.put(c as u32, l as u32);
                bw.put(bitsv, s);
                // AC in zigzag order
                let mut run = 0u32;
                for k in 1..64 {
                    let v = q[ZIGZAG[k]];
                    if v == 0 {
                        run += 1;
                        continue;
                    }
                    while run >= 16 {
                        let (l, c) = acc[0xF0];
                        bw.put(c as u32, l as u32);
                        run -= 16;
                    }
                    let (s, bitsv) = magnitude(v);
                    let (l, c) = acc[((run << 4) | s) as usize];
                    bw.put(c as u32, l as u32);
                    bw.put(bitsv, s);
                    run = 0;
                }
                if run > 0 {
                    let (l, c) = acc[0];
                    bw.put(c as u32, l as u32);
                }
            }
        }
        bw.flush();
        fn magnitude(v: i32) -> (u32, u32) {
            if v == 0 {
                return (0, 0);
            }
            let a = v.unsigned_abs();
            let s = 32 - a.leading_zeros();
            let bits = if v < 0 { (v - 1) as u32 & ((1 << s) - 1) } else { v as u32 };
            (s, bits)
        }
        fn libm_cos(x: f32) -> f32 {
            // Taylor-free: use a small table via f64 cos from core? core has no cos in no_std tests
            // without libm; tests run on std, so use std.
            #[allow(clippy::needless_return)]
            return f64::cos(x as f64) as f32;
        }
        let mut out = vec![0xFF, 0xD8];
        // DQT: all ones
        out.extend_from_slice(&[0xFF, 0xDB, 0, 67, 0]);
        out.extend_from_slice(&[1u8; 64]);
        // SOF0
        out.extend_from_slice(&[0xFF, 0xC0, 0, 11, 8]);
        out.extend_from_slice(&(h as u16).to_be_bytes());
        out.extend_from_slice(&(w as u16).to_be_bytes());
        out.extend_from_slice(&[1, 1, 0x11, 0]);
        // DHT DC + AC
        let mut dht = vec![0x00u8];
        dht.extend_from_slice(&DC_COUNTS);
        dht.extend_from_slice(&DC_SYMS);
        dht.push(0x10);
        dht.extend_from_slice(&AC_COUNTS);
        dht.extend_from_slice(&AC_SYMS);
        out.extend_from_slice(&[0xFF, 0xC4]);
        out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&dht);
        // SOS
        out.extend_from_slice(&[0xFF, 0xDA, 0, 8, 1, 1, 0x00, 0, 63, 0]);
        out.extend_from_slice(&bw.out);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    #[test]
    fn round_trips_a_gradient() {
        let jpg = encode_grey(64, 48, &|x, _y| (x * 4) as u8);
        let bm = decode(&jpg, Fit::inside(64, 48)).expect("decode");
        assert_eq!((bm.w, bm.h), (64, 48));
        // Left is dark (ink), right is light (paper).
        let left: u32 = (0..48).map(|y| bm.get(2, y) as u32).sum();
        let right: u32 = (0..48).map(|y| bm.get(61, y) as u32).sum();
        assert!(left > 40 && right < 8, "left ink {left}, right ink {right}");
    }

    #[test]
    fn dc_only_thumbnail_path() {
        let jpg = encode_grey(256, 256, &|x, y| if (x / 64 + y / 64) % 2 == 0 { 0 } else { 255 });
        let bm = decode(&jpg, Fit::inside(32, 32)).expect("decode");
        assert_eq!((bm.w, bm.h), (32, 32));
        assert!(bm.get(4, 4) && !bm.get(12, 4) && bm.get(20, 4), "checkerboard survives the 1/8 path");
    }

    #[test]
    fn real_cover_from_moby_dick_decodes() {
        let data = include_bytes!("../fixtures/moby-dick.epub");
        let z = crate::zip::Zip::open(&data[..]).unwrap();
        let e = z.find("OPS/images/9780316000000.jpg").expect("cover entry").clone();
        let jpg = z.read(&e, 4 << 20).unwrap();
        let bm = decode(&jpg, Fit::fill(152, 228)).expect("decode cover");
        assert_eq!((bm.w, bm.h), (152, 228));
        let ink = bm.bits.iter().map(|b| b.count_ones()).sum::<u32>();
        assert!(ink > 2000 && ink < 30000, "cover has structure: {ink} ink px");
        let full = decode(&jpg, Fit::fill(528, 792)).expect("full cover");
        assert_eq!((full.w, full.h), (528, 792));
    }

    #[test]
    fn progressive_is_refused_not_crashed() {
        let mut jpg = encode_grey(16, 16, &|_, _| 128);
        // Rewrite SOF0 → SOF2.
        if let Some(i) = jpg.windows(2).position(|w| w == [0xFF, 0xC0]) {
            jpg[i + 1] = 0xC2;
        }
        assert!(matches!(decode(&jpg, Fit::inside(16, 16)), Err(DocError::Unsupported(_))));
        assert!(decode(&b"\xFF\xD8\xFF\xE0junk"[..].to_vec(), Fit::inside(16, 16)).is_err());
    }
}

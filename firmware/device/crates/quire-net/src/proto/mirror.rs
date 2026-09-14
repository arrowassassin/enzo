//! The live mirror: the 1-bit panel frame packed at half size for the Window tab.
//! A 2 × 2 block is black when any of its pixels is, so thin strokes survive.

/// Panel width in pixels.
pub const FRAME_W: usize = 528;
/// Panel height in pixels.
pub const FRAME_H: usize = 792;
/// Mirror width.
pub const MIRROR_W: usize = FRAME_W / 2;
/// Mirror height.
pub const MIRROR_H: usize = FRAME_H / 2;
/// Bytes per mirror row.
pub const MIRROR_STRIDE: usize = MIRROR_W.div_ceil(8);
/// Bytes in a packed mirror frame.
pub const MIRROR_BYTES: usize = MIRROR_STRIDE * MIRROR_H;

/// Pack `bits` (a 1-bit frame `w` × `h`, MSB first, 1 = ink, rows padded to bytes) at
/// half size into `out`; returns the bytes written (0 when `out` is too small).
pub fn pack_half(bits: &[u8], w: usize, h: usize, out: &mut [u8]) -> usize {
    let stride = w.div_ceil(8);
    let ow = w / 2;
    let oh = h / 2;
    let ostride = ow.div_ceil(8);
    let need = ostride * oh;
    if out.len() < need || bits.len() < stride * h {
        return 0;
    }
    out[..need].fill(0);
    for oy in 0..oh {
        let r0 = &bits[oy * 2 * stride..oy * 2 * stride + stride];
        let r1 = &bits[(oy * 2 + 1) * stride..(oy * 2 + 1) * stride + stride];
        let orow = &mut out[oy * ostride..oy * ostride + ostride];
        // Two source bytes (16 px) become one output byte (8 px).
        for (i, ob) in orow.iter_mut().enumerate() {
            let s = i * 2;
            if s >= stride {
                break;
            }
            let a = r0[s] | r1[s];
            let b = if s + 1 < stride { r0[s + 1] | r1[s + 1] } else { 0 };
            let v = ((a as u16) << 8) | b as u16;
            let mut o = 0u8;
            for px in 0..8 {
                let pair = (v >> (14 - px * 2)) & 0b11;
                if pair != 0 {
                    o |= 0x80 >> px;
                }
            }
            *ob = o;
        }
    }
    need
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(MIRROR_BYTES, 33 * 396);
    }

    #[test]
    fn packs_or_of_blocks() {
        // 16 × 4 frame: one pixel at (1,1) and a block at (14..16, 2..4).
        let w = 16;
        let h = 4;
        let mut bits = [0u8; 8];
        bits[1 * 2] |= 0x80 >> 1; // (1,1)
        bits[2 * 2 + 1] |= 0b11; // (14,2),(15,2)
        bits[3 * 2 + 1] |= 0b11;
        let mut out = [0u8; 2];
        assert_eq!(pack_half(&bits, w, h, &mut out), 2);
        // Row 0 of the mirror covers source rows 0-1: pixel (0,0) set.
        assert_eq!(out[0], 0x80);
        // Row 1 covers rows 2-3: pixel (7,1) set.
        assert_eq!(out[1], 0x01);
    }

    #[test]
    fn full_frame() {
        let bits = alloc::vec![0xFF; 66 * 792];
        let mut out = alloc::vec![0; MIRROR_BYTES];
        assert_eq!(pack_half(&bits, FRAME_W, FRAME_H, &mut out), MIRROR_BYTES);
        assert!(out.iter().all(|&b| b == 0xFF));
        let mut small = [0u8; 10];
        assert_eq!(pack_half(&bits, FRAME_W, FRAME_H, &mut small), 0);
    }
}

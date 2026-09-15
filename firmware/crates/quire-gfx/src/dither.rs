//! Grey to ink. Floyd–Steinberg for images, ordered Bayer for fast previews, and a
//! 2-bit quantiser for the panel's four-grey mode.

use alloc::vec;
use alloc::vec::Vec;

use crate::frame::Bitmap;

/// Floyd–Steinberg dither an 8-bit greyscale image (0 = black, 255 = white) to 1 bit.
///
/// Uses a two-row error buffer, so memory is `O(width)` regardless of height, which
/// is what lets the device dither a full-screen cover during ingest.
pub fn floyd_steinberg(gray: &[u8], w: u32, h: u32) -> Bitmap {
    let mut out = Bitmap::new(w, h);
    let wu = w as usize;
    let mut err_cur: Vec<i16> = vec![0; wu + 2];
    let mut err_next: Vec<i16> = vec![0; wu + 2];
    for y in 0..h as usize {
        err_next.fill(0);
        for x in 0..wu {
            let v = gray[y * wu + x] as i32 + err_cur[x + 1] as i32;
            let (q, e) = if v < 128 { (0, v) } else { (255, v - 255) };
            if q == 0 {
                out.set(x as u32, y as u32, true);
            }
            let e = e.clamp(-255, 255) as i16;
            // 7/16 right, 3/16 down-left, 5/16 down, 1/16 down-right.
            err_cur[x + 2] = err_cur[x + 2].saturating_add(e * 7 / 16);
            err_next[x] = err_next[x].saturating_add(e * 3 / 16);
            err_next[x + 1] = err_next[x + 1].saturating_add(e * 5 / 16);
            err_next[x + 2] = err_next[x + 2].saturating_add(e / 16);
        }
        core::mem::swap(&mut err_cur, &mut err_next);
    }
    out
}

const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Ordered 4 × 4 Bayer dither: fast, stable between refreshes, good for thumbnails.
pub fn bayer(gray: &[u8], w: u32, h: u32) -> Bitmap {
    let mut out = Bitmap::new(w, h);
    let wu = w as usize;
    for y in 0..h as usize {
        for x in 0..wu {
            let t = (BAYER4[y & 3][x & 3] as u32 * 16 + 8) as u8;
            if gray[y * wu + x] < t {
                out.set(x as u32, y as u32, true);
            }
        }
    }
    out
}

/// Quantise grey to the panel's four levels with error diffusion, returning two
/// bit planes: `(msb, lsb)` where value 0 is black and 3 is white.
pub fn floyd_steinberg_2bit(gray: &[u8], w: u32, h: u32) -> (Bitmap, Bitmap) {
    let mut msb = Bitmap::new(w, h);
    let mut lsb = Bitmap::new(w, h);
    let wu = w as usize;
    let mut err_cur: Vec<i16> = vec![0; wu + 2];
    let mut err_next: Vec<i16> = vec![0; wu + 2];
    for y in 0..h as usize {
        err_next.fill(0);
        for x in 0..wu {
            let v = (gray[y * wu + x] as i32 + err_cur[x + 1] as i32).clamp(0, 255);
            let level = ((v + 42) / 85).min(3); // 0..3
            let q = level * 85;
            let e = (v - q).clamp(-255, 255) as i16;
            if level & 2 != 0 {
                msb.set(x as u32, y as u32, true);
            }
            if level & 1 != 0 {
                lsb.set(x as u32, y as u32, true);
            }
            err_cur[x + 2] = err_cur[x + 2].saturating_add(e * 7 / 16);
            err_next[x] = err_next[x].saturating_add(e * 3 / 16);
            err_next[x + 1] = err_next[x + 1].saturating_add(e * 5 / 16);
            err_next[x + 2] = err_next[x + 2].saturating_add(e / 16);
        }
        core::mem::swap(&mut err_cur, &mut err_next);
    }
    (msb, lsb)
}

/// Nearest-neighbour-free box downscale of an 8-bit grey image to `dw × dh`.
pub fn scale_gray(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dw * dh) as usize];
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return out;
    }
    for y in 0..dh {
        let sy0 = (y * sh / dh) as usize;
        let sy1 = (((y + 1) * sh).div_ceil(dh) as usize).max(sy0 + 1).min(sh as usize);
        for x in 0..dw {
            let sx0 = (x * sw / dw) as usize;
            let sx1 = (((x + 1) * sw).div_ceil(dw) as usize).max(sx0 + 1).min(sw as usize);
            let mut sum = 0u32;
            let mut n = 0u32;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    sum += src[sy * sw as usize + sx] as u32;
                    n += 1;
                }
            }
            out[(y * dw + x) as usize] = (sum / n.max(1)) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mid_grey_dithers_to_about_half() {
        let g = vec![128u8; 64 * 64];
        let b = floyd_steinberg(&g, 64, 64);
        let ink = b.bits.iter().map(|x| x.count_ones()).sum::<u32>();
        assert!((1800..=2300).contains(&ink), "ink={ink}");
        let b2 = bayer(&g, 64, 64);
        let ink2 = b2.bits.iter().map(|x| x.count_ones()).sum::<u32>();
        assert!((1800..=2300).contains(&ink2), "ink={ink2}");
    }

    #[test]
    fn extremes_are_exact() {
        let black = vec![0u8; 16 * 16];
        let white = vec![255u8; 16 * 16];
        assert_eq!(floyd_steinberg(&black, 16, 16).bits.iter().map(|x| x.count_ones()).sum::<u32>(), 256);
        assert_eq!(floyd_steinberg(&white, 16, 16).bits.iter().map(|x| x.count_ones()).sum::<u32>(), 0);
    }

    #[test]
    fn two_bit_levels() {
        let g: Vec<u8> = (0..4).flat_map(|l| core::iter::repeat_n(l * 85, 16)).collect();
        let (msb, lsb) = floyd_steinberg_2bit(&g, 64, 1);
        assert!(!msb.get(0, 0) && !lsb.get(0, 0));
        assert!(msb.get(63, 0) && lsb.get(63, 0));
    }

    #[test]
    fn scale_averages() {
        let src: Vec<u8> = (0..16).map(|i| if i % 2 == 0 { 0 } else { 200 }).collect();
        let out = scale_gray(&src, 4, 4, 2, 2);
        assert!(out.iter().all(|&v| v == 100));
    }
}

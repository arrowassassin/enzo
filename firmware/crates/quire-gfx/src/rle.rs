//! Run-length coding of a frame for the phone mirror: tiny, and decoded in a few
//! lines of JavaScript. Format: `u16 w, u16 h` (little-endian) then runs of
//! `u8 length` alternating paper/ink starting with paper; a run of 255 is followed by
//! another run of the same colour (a zero-length run switches colour without pixels).

use alloc::vec::Vec;

use crate::frame::Frame;

/// Encode a frame.
pub fn encode(frame: &Frame) -> Vec<u8> {
    let (w, h) = (frame.width(), frame.height());
    let mut out = Vec::with_capacity(4 + (w * h / 40) as usize);
    out.extend_from_slice(&(w as u16).to_le_bytes());
    out.extend_from_slice(&(h as u16).to_le_bytes());
    let mut colour = false; // paper first
    let mut run: u32 = 0;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let p = frame.get(x, y);
            if p == colour {
                run += 1;
            } else {
                push_run(&mut out, run);
                colour = p;
                run = 1;
            }
        }
    }
    push_run(&mut out, run);
    out
}

fn push_run(out: &mut Vec<u8>, mut run: u32) {
    while run >= 255 {
        out.push(255);
        out.push(0); // same colour continues
        run -= 255;
    }
    out.push(run as u8);
}

/// Decode back into a frame (used by tests and the simulator).
pub fn decode(data: &[u8]) -> Option<Frame> {
    if data.len() < 4 {
        return None;
    }
    let w = u16::from_le_bytes([data[0], data[1]]) as u32;
    let h = u16::from_le_bytes([data[2], data[3]]) as u32;
    let mut f = Frame::new(w, h);
    let mut colour = false;
    let mut i = 0u32;
    let total = w * h;
    for &b in &data[4..] {
        let n = b as u32;
        if colour {
            for k in i..(i + n).min(total) {
                f.set((k % w) as i32, (k / w) as i32, crate::frame::Ink::Black);
            }
        }
        i += n;
        colour = !colour;
    }
    Some(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Ink, Rect};

    #[test]
    fn round_trip() {
        let mut f = Frame::new(100, 30);
        f.fill_rect(Rect::new(10, 3, 300, 5), Ink::Black); // long runs across rows
        f.fill_rect(Rect::new(0, 20, 100, 1), Ink::Black);
        let enc = encode(&f);
        assert!(enc.len() < 100 * 30 / 8);
        assert_eq!(decode(&enc).unwrap(), f);
    }
}

//! QR codes for the phone-as-keyboard and Drop links (brief §3: version 3–6, modules ≥ 6 px).
//! Encoded with the heap-free generator into fixed buffers, then kept as a packed module
//! bitmap so a screen encodes once and draws as often as it likes.

use alloc::vec::Vec;
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};
use quire_gfx::{Frame, Ink, Rect};

/// Largest symbol we draw (version 10 = 57 modules; a Drop URL with a token fits in 6).
const MAX_VERSION: Version = Version::new(10);
/// Quiet zone in modules on every side.
const QUIET: i32 = 4;

/// An encoded symbol: `n × n` modules, one byte per module.
pub struct Qr {
    n: i32,
    modules: Vec<u8>,
}

impl Qr {
    /// Encode `text` at medium error correction; `None` if it does not fit version 10.
    pub fn encode(text: &str) -> Option<Qr> {
        let mut temp = [0u8; Version::new(10).buffer_len()];
        let mut out = [0u8; Version::new(10).buffer_len()];
        let qr = QrCode::encode_text(text, &mut temp, &mut out, QrCodeEcc::Medium, Version::MIN, MAX_VERSION, None, true).ok()?;
        let n = qr.size();
        let mut modules = Vec::with_capacity((n * n) as usize);
        for y in 0..n {
            for x in 0..n {
                modules.push(qr.get_module(x, y) as u8);
            }
        }
        Some(Qr { n, modules })
    }

    /// Modules per side (without the quiet zone).
    pub fn modules(&self) -> i32 {
        self.n
    }

    /// Pixel size at `module` px per module, quiet zone included.
    pub fn size_px(&self, module: i32) -> i32 {
        (self.n + 2 * QUIET) * module
    }

    /// Draw with the top-left (of the quiet zone) at (x, y); returns the drawn size.
    pub fn draw(&self, f: &mut Frame, x: i32, y: i32, module: i32) -> i32 {
        let size = self.size_px(module);
        f.fill_rect(Rect::new(x, y, size as u32, size as u32), Ink::White);
        for my in 0..self.n {
            for mx in 0..self.n {
                if self.modules[(my * self.n + mx) as usize] != 0 {
                    f.fill_rect(Rect::new(x + (mx + QUIET) * module, y + (my + QUIET) * module, module as u32, module as u32), Ink::Black);
                }
            }
        }
        size
    }
}

/// Draw a QR for `text` with its top-left at (x, y) and modules of `module` px, including
/// a 4-module quiet zone. Returns the drawn size in pixels, or None if the text is too long.
/// Screens that draw every frame should keep a [`Qr`] instead of calling this per draw.
pub fn draw(f: &mut Frame, text: &str, x: i32, y: i32, module: i32) -> Option<i32> {
    Some(Qr::encode(text)?.draw(f, x, y, module))
}

/// Pixel size a QR for `text` would take at `module` px, or None.
pub fn size(text: &str, module: i32) -> Option<i32> {
    Some(Qr::encode(text)?.size_px(module))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_draws() {
        let qr = Qr::encode("http://quire.local/type").expect("fits");
        assert!(qr.modules() >= 21 && qr.modules() <= 57);
        let mut f = Frame::new(400, 400);
        f.clear(Ink::White);
        let s = qr.draw(&mut f, 10, 10, 4);
        assert_eq!(s, qr.size_px(4));
        assert!(f.ink_count() > 100);
        // Finder pattern corner is ink.
        assert!(f.get(10 + 4 * 4, 10 + 4 * 4));
        assert!(Qr::encode(&"x".repeat(2000)).is_none());
    }
}

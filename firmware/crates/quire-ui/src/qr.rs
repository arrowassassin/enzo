//! QR codes for the phone-as-keyboard and Drop links (brief §3: version 3–6, modules ≥ 6 px).

use quire_gfx::{Frame, Ink, Rect};

/// Draw a QR for `text` with its top-left at (x, y) and modules of `module` px, including
/// a 4-module quiet zone. Returns the drawn size in pixels, or None if the text is too long.
pub fn draw(f: &mut Frame, text: &str, x: i32, y: i32, module: i32) -> Option<i32> {
    let qr = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Medium).ok()?;
    let n = qr.size();
    let quiet = 4;
    let size = (n + 2 * quiet) * module;
    f.fill_rect(Rect::new(x, y, size as u32, size as u32), Ink::White);
    for my in 0..n {
        for mx in 0..n {
            if qr.get_module(mx, my) {
                f.fill_rect(Rect::new(x + (mx + quiet) * module, y + (my + quiet) * module, module as u32, module as u32), Ink::Black);
            }
        }
    }
    Some(size)
}

/// Pixel size a QR for `text` would take at `module` px, or None.
pub fn size(text: &str, module: i32) -> Option<i32> {
    let qr = qrcodegen::QrCode::encode_text(text, qrcodegen::QrCodeEcc::Medium).ok()?;
    Some((qr.size() + 8) * module)
}

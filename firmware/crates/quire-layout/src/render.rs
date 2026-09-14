//! Draw a laid-out page onto a frame.

use quire_gfx::{draw_text, BitmapRef, BlitMode, Frame, Ink, Rect, TextStyle};
use quire_qtx::style;

use crate::page::{DrawItem, Page};
use crate::Profile;

/// Supplies image bitmaps by id (the chapter's image cache).
pub trait ImageSource {
    /// The 1-bit bitmap for an image id, already scaled to the requested size if possible.
    fn image(&self, id: u16, w: u32, h: u32) -> Option<BitmapRef<'_>>;
}

/// No images available: draws a framed placeholder.
pub struct NoImages;
impl ImageSource for NoImages {
    fn image(&self, _id: u16, _w: u32, _h: u32) -> Option<BitmapRef<'_>> {
        None
    }
}

/// Render a page.
pub fn render_page(page: &Page, frame: &mut Frame, profile: &Profile, images: &dyn ImageSource) {
    let ts = TextStyle { darker: profile.darker, ..TextStyle::INK };
    for item in &page.items {
        match item {
            DrawItem::Text { x, y, font, text, style: st } => {
                let bold_italic = *st & (style::BOLD | style::ITALIC) == style::BOLD | style::ITALIC;
                let ts = TextStyle { darker: ts.darker || bold_italic, ..ts };
                let end = draw_text(frame, font, *x, *y, text, ts);
                if st & style::UNDERLINE != 0 {
                    frame.hline(*x, *y + 3, (end - *x).max(0) as u32, 1, Ink::Black);
                }
                if st & style::STRIKE != 0 {
                    frame.hline(*x, *y - font.ascent() / 3, (end - *x).max(0) as u32, 2, Ink::Black);
                }
            }
            DrawItem::Rule(r) => frame.fill_rect(*r, Ink::Black),
            DrawItem::Image { id, rect } => match images.image(*id, rect.w, rect.h) {
                Some(bm) => {
                    let ox = rect.x + (rect.w as i32 - bm.w as i32) / 2;
                    let oy = rect.y + (rect.h as i32 - bm.h as i32) / 2;
                    frame.blit(ox, oy, bm, BlitMode::Copy);
                }
                None => {
                    frame.stroke_rect(*rect, 2, Ink::Black);
                    let f = quire_fonts::ui::label();
                    let label = "image";
                    let w = quire_gfx::measure_text(f, label, TextStyle::INK);
                    draw_text(
                        frame,
                        f,
                        rect.x + (rect.w as i32 - w) / 2,
                        rect.y + rect.h as i32 / 2 + f.ascent() / 2,
                        label,
                        TextStyle::INK,
                    );
                }
            },
            DrawItem::DropCap { x, y, font, ch } => {
                let mut s = [0u8; 4];
                draw_text(frame, font, *x, *y, ch.encode_utf8(&mut s), ts);
            }
        }
    }
    let _ = Rect::default();
}

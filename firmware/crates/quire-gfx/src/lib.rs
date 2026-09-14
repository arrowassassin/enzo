//! Quire graphics core.
//!
//! Everything here is `no_std + alloc` and deterministic: the same inputs produce the same
//! bits on the desktop simulator and on the device, which is what the snapshot tests rely on.
//!
//! Conventions: the [`Frame`] is packed 1 bit per pixel, MSB first, one row after another,
//! and a set bit is **ink** (black). Coordinates are `i32` so callers can position things
//! partly off-screen without worrying; every primitive clips.
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod dither;
pub mod font;
pub mod frame;
pub mod rle;
pub mod text;

pub use font::{Font, Glyph};
pub use frame::{Bitmap, BitmapRef, BlitMode, Frame, Ink, Pattern, Rect, Rotation};
pub use text::{draw_text, draw_text_ccw, measure_text, TextStyle};

/// Panel width in portrait orientation, pixels.
pub const PANEL_W: u32 = 528;
/// Panel height in portrait orientation, pixels.
pub const PANEL_H: u32 = 792;

//! `quire-sleep`: procedural 1-bit sleep-screen art for the Xteink X3 panel.
//!
//! Every image is drawn on a [`canvas::Canvas`] — a painter's model with a crisp
//! ink/paper layer over a continuous grey field that is Floyd–Steinberg dithered once,
//! at the end — and leaves a rectangular *clock slot* untouched so the firmware can
//! draw the live time on top. The packs are written as P4 PBM (1 = ink), a zlib
//! variant for the device's inflater, JSON sidecars and PNG contact sheets.

pub mod canvas;
pub mod noise;
pub mod output;
pub mod packs;
pub mod slot;
pub mod text;

/// Panel width in pixels.
pub const W: u32 = quire_gfx::PANEL_W;
/// Panel height in pixels.
pub const H: u32 = quire_gfx::PANEL_H;

/// A finished artwork: the canvas plus the metadata the sidecar records.
pub struct Art {
    /// The drawing.
    pub canvas: canvas::Canvas,
    /// Where the firmware draws the time.
    pub slot: slot::ClockSlot,
    /// A short title shown in the sidecar and preview.
    pub title: String,
    /// Credit line (the generator, plus the quoted author where there is one).
    pub credit: String,
}

/// Seed for image `index` of pack `id`: stable across runs and machines.
pub fn seed_for(id: &str, index: usize) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes().chain(b"/".iter().copied()).chain(index.to_string().bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

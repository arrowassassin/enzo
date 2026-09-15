//! Writing packs: P4 PBM, the zlib-compressed variant, JSON sidecars, the index and
//! PNG contact sheets.

use anyhow::{Context, Result};
use quire_gfx::Bitmap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::canvas::ink_density;
use crate::packs::Pack;
use crate::slot::ClockSlot;
use crate::Art;

/// Format version written to `index.json`.
pub const INDEX_VERSION: u32 = 1;
/// Licence of the generated images.
pub const LICENCE: &str = "CC0-1.0";

/// One image in `pack.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageEntry {
    /// The P4 PBM file, relative to the pack directory.
    pub file: String,
    /// The same PBM, zlib-compressed (deflate with a zlib header and Adler-32).
    pub z: String,
    /// Size of `file` in bytes.
    pub bytes: u64,
    /// Size of `z` in bytes.
    pub bytes_z: u64,
    /// Short title.
    pub title: String,
    /// Where and how the firmware draws the time.
    pub clock: ClockSlot,
    /// Credit line.
    pub credit: String,
    /// Fraction of ink pixels, for the curious (and the tests).
    pub ink: f32,
}

/// `pack.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackJson {
    /// Pack id (directory name).
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// SPDX licence of the images.
    pub licence: String,
    /// The images, in display order.
    pub images: Vec<ImageEntry>,
}

/// One pack in `index.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexPack {
    /// Pack id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Number of images.
    pub images: usize,
    /// Total bytes of the uncompressed PBMs (what the card will hold).
    pub bytes: u64,
    /// Total bytes of the compressed variants (what the device downloads).
    pub bytes_z: u64,
    /// Path of the pack's `pack.json`, relative to the index.
    pub path: String,
}

/// The image format every pack shares.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Format {
    /// `"P4"`.
    pub pbm: String,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Meaning of a set bit.
    pub one: String,
    /// Compression of the `.z` files.
    pub z: String,
}

/// `index.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Index {
    /// Schema version.
    pub version: u32,
    /// Shared image format.
    pub format: Format,
    /// Licence of the images.
    pub licence: String,
    /// The packs.
    pub packs: Vec<IndexPack>,
}

/// A bitmap as a P4 PBM file.
pub fn pbm_bytes(bm: &Bitmap) -> Vec<u8> {
    let mut v = format!("P4\n{} {}\n", bm.w, bm.h).into_bytes();
    v.extend_from_slice(&bm.bits);
    v
}

/// The zlib-framed deflate the device's inflater (`Framing::Zlib`) reads.
pub fn compress(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 10)
}

/// Save a bitmap as a greyscale PNG.
pub fn bitmap_to_png(bm: &Bitmap, path: &Path) -> Result<()> {
    let mut img = image::GrayImage::new(bm.w, bm.h);
    for y in 0..bm.h {
        for x in 0..bm.w {
            img.put_pixel(x, y, image::Luma([if bm.get(x, y) { 0 } else { 255 }]));
        }
    }
    img.save(path).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Render every image of a pack: `(clean bitmap, preview bitmap with the sample time)`.
pub fn render_pack(pack: &Pack, sample_time: &str) -> Vec<(Art, Bitmap, Bitmap)> {
    (0..pack.count)
        .map(|i| {
            let mut art = (pack.render)(i);
            art.slot.clear(&mut art.canvas);
            let clean = art.canvas.finish();
            let mut preview = art.canvas.clone();
            art.slot.draw_sample(&mut preview, sample_time);
            let preview = preview.finish();
            (art, clean, preview)
        })
        .collect()
}

/// Write a pack directory (`<out>/<id>/`) and its preview sheet (`<out>/preview/<id>.png`).
pub fn write_pack(out: &Path, pack: &Pack, sample_time: &str) -> Result<IndexPack> {
    let dir = out.join(pack.id);
    fs::create_dir_all(&dir)?;
    let rendered = render_pack(pack, sample_time);
    let mut images = Vec::new();
    let mut previews = Vec::new();
    for (i, (art, clean, preview)) in rendered.into_iter().enumerate() {
        let file = format!("{:02}.pbm", i + 1);
        let z = format!("{file}.z");
        let pbm = pbm_bytes(&clean);
        let zipped = compress(&pbm);
        fs::write(dir.join(&file), &pbm)?;
        fs::write(dir.join(&z), &zipped)?;
        images.push(ImageEntry {
            file,
            z,
            bytes: pbm.len() as u64,
            bytes_z: zipped.len() as u64,
            title: art.title.clone(),
            clock: art.slot,
            credit: art.credit.clone(),
            ink: (ink_density(&clean) * 1000.0).round() / 1000.0,
        });
        previews.push(preview);
    }
    let json = PackJson {
        id: pack.id.to_string(),
        name: pack.name.to_string(),
        description: pack.description.to_string(),
        licence: LICENCE.to_string(),
        images,
    };
    fs::write(dir.join("pack.json"), serde_json::to_string_pretty(&json)? + "\n")?;
    let preview_dir = out.join("preview");
    fs::create_dir_all(&preview_dir)?;
    contact_sheet(&previews, &preview_dir.join(format!("{}.png", pack.id)))?;
    Ok(IndexPack {
        id: json.id,
        name: json.name,
        description: json.description,
        images: json.images.len(),
        bytes: json.images.iter().map(|i| i.bytes).sum(),
        bytes_z: json.images.iter().map(|i| i.bytes_z).sum(),
        path: format!("{}/pack.json", pack.id),
    })
}

/// Write `index.json`.
pub fn write_index(out: &Path, packs: Vec<IndexPack>) -> Result<Index> {
    let index = Index {
        version: INDEX_VERSION,
        format: Format { pbm: "P4".into(), width: crate::W, height: crate::H, one: "ink".into(), z: "zlib".into() },
        licence: LICENCE.into(),
        packs,
    };
    fs::write(out.join("index.json"), serde_json::to_string_pretty(&index)? + "\n")?;
    Ok(index)
}

/// Contact sheet: every image in a row at one third size, with a gutter.
pub fn contact_sheet(bitmaps: &[Bitmap], path: &Path) -> Result<()> {
    const SCALE: u32 = 4;
    const GUTTER: u32 = 8;
    let (tw, th) = (crate::W / SCALE, crate::H / SCALE);
    let n = bitmaps.len() as u32;
    let sheet_w = n * tw + (n + 1) * GUTTER;
    let sheet_h = th + 2 * GUTTER;
    let mut img = image::GrayImage::from_pixel(sheet_w, sheet_h, image::Luma([210]));
    for (i, bm) in bitmaps.iter().enumerate() {
        let ox = GUTTER + i as u32 * (tw + GUTTER);
        for y in 0..th {
            for x in 0..tw {
                // Box-filter the block so dithers read as tone in the preview.
                let mut ink = 0u32;
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        if bm.get(x * SCALE + dx, y * SCALE + dy) {
                            ink += 1;
                        }
                    }
                }
                let v = 255 - (ink * 255 / (SCALE * SCALE)) as u8;
                img.put_pixel(ox + x, GUTTER + y, image::Luma([v]));
            }
        }
    }
    let file = fs::File::create(path).with_context(|| format!("write {}", path.display()))?;
    let enc = image::codecs::png::PngEncoder::new_with_quality(
        std::io::BufWriter::new(file),
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    );
    image::ImageEncoder::write_image(enc, img.as_raw(), sheet_w, sheet_h, image::ExtendedColorType::L8)?;
    Ok(())
}

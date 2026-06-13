//! Glyph atlas: rasterises glyphs with cosmic-text/swash and packs them
//! into a single RGBA8 texture uploaded to the GPU.

use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache};
use std::collections::HashMap;

pub const ATLAS_W: u32 = 2048;
pub const ATLAS_H: u32 = 2048;

#[derive(Clone, Copy, Debug)]
pub struct GlyphRect {
    /// UV pixel coordinates inside the atlas texture.
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// Signed offset from the cell origin (reserved for sub-cell placement).
    pub _offset_x: i32,
    pub _offset_y: i32,
}

pub struct GlyphAtlas {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    /// Raw RGBA8 pixel buffer, ATLAS_W × ATLAS_H.
    pub pixels: Vec<u8>,
    pub cell_w: u32,
    pub cell_h: u32,
    /// Map from Unicode scalar → packed atlas rect.
    cache: HashMap<char, GlyphRect>,
    /// Next free position in the atlas.
    cursor_x: u32,
    cursor_y: u32,
    row_h: u32,
    /// Metrics buffer used for single-char measurement.
    measure_buf: Buffer,
}

impl GlyphAtlas {
    pub fn new(font_size: f32) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let pixels = vec![0u8; (ATLAS_W * ATLAS_H * 4) as usize];

        // Measure a reference character to get cell dimensions.
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buf = Buffer::new(&mut font_system, metrics);
        buf.set_size(&mut font_system, Some(1000.0), Some(1000.0));
        buf.set_text(&mut font_system, "M", Attrs::new(), Shaping::Advanced);
        buf.shape_until_scroll(&mut font_system, false);

        let cell_w = font_size as u32;
        let cell_h = (font_size * 1.2) as u32;

        Self {
            font_system,
            swash_cache,
            pixels,
            cell_w,
            cell_h,
            cache: HashMap::new(),
            cursor_x: 0,
            cursor_y: 0,
            row_h: 0,
            measure_buf: buf,
        }
    }

    /// Returns the atlas rect for `ch`, rasterising it on first call.
    pub fn get_or_insert(&mut self, ch: char) -> GlyphRect {
        if let Some(&r) = self.cache.get(&ch) {
            return r;
        }
        // Rasterise via cosmic-text + swash.
        let metrics = Metrics::new(self.cell_h as f32 / 1.2, self.cell_h as f32);
        self.measure_buf.set_metrics(&mut self.font_system, metrics);
        let s: String = std::iter::once(ch).collect();
        self.measure_buf
            .set_text(&mut self.font_system, &s, Attrs::new(), Shaping::Advanced);
        self.measure_buf
            .shape_until_scroll(&mut self.font_system, false);

        // Walk glyph runs and stamp each physical glyph into the atlas.
        let cell_w = self.cell_w;
        let cell_h = self.cell_h;

        // Allocate a cell-sized region even if the glyph is smaller.
        let rect = self.allocate(cell_w, cell_h);

        // Collect physical glyphs from layout without holding a borrow on self.
        struct BlitJob {
            dst_x: i32,
            dst_y: i32,
            w: usize,
            h: usize,
            data: Vec<u8>,
            content: cosmic_text::SwashContent,
        }
        let physicals: Vec<_> = self
            .measure_buf
            .layout_runs()
            .flat_map(|run| {
                run.glyphs
                    .iter()
                    .map(|g| g.physical((0.0, 0.0), 1.0))
                    .collect::<Vec<_>>()
            })
            .collect();

        let mut jobs: Vec<BlitJob> = Vec::new();
        for physical in physicals {
            if let Some(img) = self
                .swash_cache
                .get_image(&mut self.font_system, physical.cache_key)
            {
                jobs.push(BlitJob {
                    dst_x: rect.x as i32 + physical.x + img.placement.left,
                    dst_y: rect.y as i32 + physical.y - img.placement.top,
                    w: img.placement.width as usize,
                    h: img.placement.height as usize,
                    data: img.data.to_vec(),
                    content: img.content,
                });
            }
        }
        for job in jobs {
            self.blit_swash(job.dst_x, job.dst_y, job.w, job.h, &job.data, &job.content);
        }

        self.cache.insert(ch, rect);
        rect
    }

    fn allocate(&mut self, w: u32, h: u32) -> GlyphRect {
        if self.cursor_x + w > ATLAS_W {
            self.cursor_x = 0;
            self.cursor_y += self.row_h;
            self.row_h = 0;
        }
        let x = self.cursor_x;
        let y = self.cursor_y;
        self.cursor_x += w;
        self.row_h = self.row_h.max(h);
        GlyphRect {
            x,
            y,
            w,
            h,
            _offset_x: 0,
            _offset_y: 0,
        }
    }

    fn blit_swash(
        &mut self,
        dst_x: i32,
        dst_y: i32,
        w: usize,
        h: usize,
        data: &[u8],
        content: &cosmic_text::SwashContent,
    ) {
        use cosmic_text::SwashContent;
        for row in 0..h {
            for col in 0..w {
                let px = col + dst_x as usize;
                let py = row + dst_y as usize;
                if px >= ATLAS_W as usize || py >= ATLAS_H as usize {
                    continue;
                }
                let dst = (py * ATLAS_W as usize + px) * 4;
                match content {
                    SwashContent::Mask => {
                        let alpha = data[row * w + col];
                        // White glyph, alpha-masked.
                        self.pixels[dst] = 255;
                        self.pixels[dst + 1] = 255;
                        self.pixels[dst + 2] = 255;
                        self.pixels[dst + 3] = alpha;
                    }
                    SwashContent::Color => {
                        let src = (row * w + col) * 4;
                        self.pixels[dst..dst + 4].copy_from_slice(&data[src..src + 4]);
                    }
                    SwashContent::SubpixelMask => {
                        // Treat subpixel as greyscale for simplicity in this spike.
                        let src = (row * w + col) * 3;
                        let avg =
                            (data[src] as u16 + data[src + 1] as u16 + data[src + 2] as u16) / 3;
                        self.pixels[dst] = 255;
                        self.pixels[dst + 1] = 255;
                        self.pixels[dst + 2] = 255;
                        self.pixels[dst + 3] = avg as u8;
                    }
                }
            }
        }
    }
}

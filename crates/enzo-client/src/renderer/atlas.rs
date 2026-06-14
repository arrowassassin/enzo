//! Glyph atlas: rasterises glyphs with cosmic-text/swash and packs them
//! into a single RGBA8 texture uploaded to the GPU.

use std::collections::HashMap;

use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache, SwashContent};

/// Atlas texture width in pixels.
pub const ATLAS_W: u32 = 2048;
/// Atlas texture height in pixels.
pub const ATLAS_H: u32 = 2048;

/// UV pixel coordinates of one glyph inside the atlas texture.
#[derive(Clone, Copy, Debug)]
pub struct GlyphRect {
    /// Left edge (pixels).
    pub x: u32,
    /// Top edge (pixels).
    pub y: u32,
    /// Width (pixels).
    pub w: u32,
    /// Height (pixels).
    pub h: u32,
}

/// Deferred blit job — collected before touching `self.pixels` to avoid
/// borrowing conflicts between `swash_cache`/`font_system` and `pixels`.
struct BlitJob {
    dst_x: i32,
    dst_y: i32,
    w: usize,
    h: usize,
    data: Vec<u8>,
    content: SwashContent,
}

/// Rasterises characters on demand and packs them into one RGBA8 texture.
pub struct GlyphAtlas {
    font_system: FontSystem,
    swash_cache: SwashCache,
    /// Raw RGBA8 pixels, `ATLAS_W × ATLAS_H`.
    pub pixels: Vec<u8>,
    /// Monospace cell width (pixels).
    pub cell_w: u32,
    /// Monospace cell height (pixels).
    pub cell_h: u32,
    cache: HashMap<char, GlyphRect>,
    cursor_x: u32,
    cursor_y: u32,
    row_h: u32,
    measure_buf: Buffer,
}

impl GlyphAtlas {
    /// Create a new atlas for the given font size (pixels).
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "font_size is always a small positive value"
    )]
    pub fn new(font_size: f32) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let pixels = vec![0u8; (ATLAS_W * ATLAS_H * 4) as usize];

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

    /// Return the atlas rect for `ch`, rasterising it on first call.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap,
        reason = "cell dimensions are small (≤ ATLAS_W) and fit safely in f32 / i32"
    )]
    pub fn get_or_insert(&mut self, ch: char) -> GlyphRect {
        if let Some(&r) = self.cache.get(&ch) {
            return r;
        }

        let metrics = Metrics::new(self.cell_h as f32 / 1.2, self.cell_h as f32);
        self.measure_buf.set_metrics(&mut self.font_system, metrics);
        let s: String = std::iter::once(ch).collect();
        self.measure_buf
            .set_text(&mut self.font_system, &s, Attrs::new(), Shaping::Advanced);
        self.measure_buf
            .shape_until_scroll(&mut self.font_system, false);

        let cell_w = self.cell_w;
        let cell_h = self.cell_h;
        let rect = self.allocate(cell_w, cell_h);

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
                    data: img.data.clone(),
                    content: img.content,
                });
            }
        }
        for job in jobs {
            self.blit(job.dst_x, job.dst_y, job.w, job.h, &job.data, job.content);
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
        GlyphRect { x, y, w, h }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "bounds-checked before use; atlas dims fit in i32/usize"
    )]
    fn blit(
        &mut self,
        dst_x: i32,
        dst_y: i32,
        w: usize,
        h: usize,
        data: &[u8],
        content: SwashContent,
    ) {
        for row in 0..h {
            for col in 0..w {
                let px = col as i32 + dst_x;
                let py = row as i32 + dst_y;
                if px < 0 || py < 0 || px >= ATLAS_W as i32 || py >= ATLAS_H as i32 {
                    continue;
                }
                let dst = (py as usize * ATLAS_W as usize + px as usize) * 4;
                match content {
                    SwashContent::Mask => {
                        let alpha = data[row * w + col];
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
                        let src = (row * w + col) * 3;
                        let avg = (u16::from(data[src])
                            + u16::from(data[src + 1])
                            + u16::from(data[src + 2]))
                            / 3;
                        // avg is 0..=255 (average of three u8 values).
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

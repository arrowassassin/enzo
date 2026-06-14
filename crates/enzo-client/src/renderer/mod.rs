//! wgpu + cosmic-text terminal renderer.
//!
//! Uploads a glyph atlas as a GPU texture and draws one quad per visible
//! cell using the cell's fg colour.

mod atlas;

use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::window::Window;

use atlas::{ATLAS_H, ATLAS_W, GlyphAtlas};

use crate::terminal::{Color, Terminal};

const FONT_SIZE: f32 = 14.0;
/// Maximum quads (one per visible cell) pre-allocated.
const MAX_CELLS: usize = 220 * 50;

/// One vertex of a glyph quad.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    fg: [f32; 4],
}

/// wgpu-backed terminal renderer.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    atlas_texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    atlas: GlyphAtlas,
    verts: Vec<Vertex>,
    indices: Vec<u16>,
    width: u32,
    height: u32,
}

impl Renderer {
    /// Initialise wgpu and create all GPU objects.
    #[allow(clippy::too_many_lines)]
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window).expect("create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("request adapter");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats[0];
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoNoVsync,
            desired_maximum_frame_latency: 1,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_W,
                height: ATLAS_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
            label: None,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x4
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let vb_size = (MAX_CELLS * 4 * std::mem::size_of::<Vertex>()) as u64;
        let ib_size = (MAX_CELLS * 6 * std::mem::size_of::<u16>()) as u64;
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vertex_buf"),
            size: vb_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("index_buf"),
            size: ib_size,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            vertex_buf,
            index_buf,
            atlas_texture,
            bind_group,
            atlas: GlyphAtlas::new(FONT_SIZE),
            verts: Vec::with_capacity(MAX_CELLS * 4),
            indices: Vec::with_capacity(MAX_CELLS * 6),
            width: size.width,
            height: size.height,
        }
    }

    /// Handle a window resize.
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.width = size.width;
        self.height = size.height;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Render one frame from the terminal state.
    pub fn render(&mut self, terminal: &Terminal) {
        self.build_quads(terminal);
        self.upload_atlas();

        self.queue
            .write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&self.verts));
        self.queue
            .write_buffer(&self.index_buf, 0, bytemuck::cast_slice(&self.indices));

        let Ok(frame) = self.surface.get_current_texture() else {
            return;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.04,
                            g: 0.04,
                            b: 0.04,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
            pass.set_index_buffer(self.index_buf.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..u32::try_from(self.indices.len()).unwrap_or(0), 0, 0..1);
        }
        self.queue.submit([enc.finish()]);
        frame.present();
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "terminal coords are small (≤ 220×50) and atlas coords ≤ 2048; f32 is sufficient"
    )]
    fn build_quads(&mut self, terminal: &Terminal) {
        self.verts.clear();
        self.indices.clear();

        let cw = f32::from(u16::try_from(self.atlas.cell_w).unwrap_or(16));
        let ch = f32::from(u16::try_from(self.atlas.cell_h).unwrap_or(20));
        let sw = self.width as f32;
        let sh = self.height as f32;
        let (cursor_col, cursor_row) = terminal.cursor();
        let cols = terminal.cols();
        let cells = terminal.cells();

        for row in 0..terminal.rows() {
            for col in 0..cols {
                let cell = cells[row as usize * cols as usize + col as usize];
                let is_cursor = col == cursor_col && row == cursor_row;
                let ch_val = if is_cursor && cell.ch == ' ' {
                    '_'
                } else {
                    cell.ch
                };

                if ch_val == ' ' && !is_cursor {
                    continue;
                }

                let rect = self.atlas.get_or_insert(ch_val);
                let base = u16::try_from(self.verts.len()).unwrap_or(0);

                let x0 = f32::from(col) * cw / sw * 2.0 - 1.0;
                let x1 = (f32::from(col) + 1.0) * cw / sw * 2.0 - 1.0;
                let y0 = 1.0 - f32::from(row) * ch / sh * 2.0;
                let y1 = 1.0 - (f32::from(row) + 1.0) * ch / sh * 2.0;

                let atlas_w = ATLAS_W as f32;
                let atlas_h = ATLAS_H as f32;
                let u0 = rect.x as f32 / atlas_w;
                let u1 = (rect.x + rect.w) as f32 / atlas_w;
                let v0 = rect.y as f32 / atlas_h;
                let v1 = (rect.y + rect.h) as f32 / atlas_h;

                let fg = resolve_fg(cell.style.fg);

                self.verts.extend_from_slice(&[
                    Vertex {
                        pos: [x0, y0],
                        uv: [u0, v0],
                        fg,
                    },
                    Vertex {
                        pos: [x1, y0],
                        uv: [u1, v0],
                        fg,
                    },
                    Vertex {
                        pos: [x1, y1],
                        uv: [u1, v1],
                        fg,
                    },
                    Vertex {
                        pos: [x0, y1],
                        uv: [u0, v1],
                        fg,
                    },
                ]);
                self.indices.extend_from_slice(&[
                    base,
                    base + 1,
                    base + 2,
                    base,
                    base + 2,
                    base + 3,
                ]);
            }
        }
    }

    fn upload_atlas(&mut self) {
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.atlas.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(ATLAS_W * 4),
                rows_per_image: Some(ATLAS_H),
            },
            wgpu::Extent3d {
                width: ATLAS_W,
                height: ATLAS_H,
                depth_or_array_layers: 1,
            },
        );
    }
}

// ── Colour mapping ───────────────────────────────────────────────────────────

/// Convert a terminal fg colour to linear RGBA.
///
/// Default maps to Matrix-green (the enzo 8-bit flagship colour).
#[must_use]
pub fn resolve_fg(color: Color) -> [f32; 4] {
    match color {
        Color::Default => [0.0, 1.0, 0.0, 1.0],
        Color::Rgb(r, g, b) => [
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            1.0,
        ],
        Color::Indexed(idx) => indexed_color(idx),
    }
}

fn indexed_color(idx: u8) -> [f32; 4] {
    match idx {
        // Standard ANSI colours.
        0 => [0.00, 0.00, 0.00, 1.0],
        1 => [0.80, 0.00, 0.00, 1.0],
        2 => [0.00, 0.80, 0.00, 1.0],
        3 => [0.80, 0.80, 0.00, 1.0],
        4 => [0.00, 0.00, 0.80, 1.0],
        5 => [0.80, 0.00, 0.80, 1.0],
        6 => [0.00, 0.80, 0.80, 1.0],
        7 => [0.80, 0.80, 0.80, 1.0],
        // Bright ANSI colours.
        8 => [0.40, 0.40, 0.40, 1.0],
        9 => [1.00, 0.20, 0.20, 1.0],
        10 => [0.20, 1.00, 0.20, 1.0],
        11 => [1.00, 1.00, 0.20, 1.0],
        12 => [0.20, 0.20, 1.00, 1.0],
        13 => [1.00, 0.20, 1.00, 1.0],
        14 => [0.20, 1.00, 1.00, 1.0],
        15 => [1.00, 1.00, 1.00, 1.0],
        // 6×6×6 colour cube.
        16..=231 => {
            let i = idx - 16;
            let r = f32::from(i / 36) / 5.0;
            let g = f32::from((i / 6) % 6) / 5.0;
            let b = f32::from(i % 6) / 5.0;
            [r, g, b, 1.0]
        }
        // Greyscale ramp.
        232..=255 => {
            let l = f32::from(idx - 232) / 23.0;
            [l, l, l, 1.0]
        }
    }
}

// ── WGSL shader ─────────────────────────────────────────────────────────────

const SHADER_SRC: &str = r"
struct VertIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv:  vec2<f32>,
    @location(2) fg:  vec4<f32>,
};

struct VertOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fg: vec4<f32>,
};

@vertex
fn vs_main(v: VertIn) -> VertOut {
    var o: VertOut;
    o.clip = vec4<f32>(v.pos, 0.0, 1.0);
    o.uv   = v.uv;
    o.fg   = v.fg;
    return o;
}

@group(0) @binding(0) var t_atlas: texture_2d<f32>;
@group(0) @binding(1) var s_atlas: sampler;

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let alpha = textureSample(t_atlas, s_atlas, in.uv).a;
    return vec4<f32>(in.fg.rgb, in.fg.a * alpha);
}
";

// ── Tests for pure logic ─────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "test values are exact integer fractions")]
mod tests {
    use super::*;
    use crate::terminal::Color;

    #[test]
    fn resolve_fg_default_is_green() {
        let [r, g, b, a] = resolve_fg(Color::Default);
        assert_eq!([r, g, b, a], [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn resolve_fg_rgb() {
        let c = resolve_fg(Color::Rgb(255, 0, 128));
        assert!((c[0] - 1.0).abs() < 1e-6);
        assert!((c[1]).abs() < 1e-6);
        assert!((c[2] - 128.0 / 255.0).abs() < 1e-3);
    }

    #[test]
    fn resolve_fg_indexed_ansi() {
        assert_eq!(resolve_fg(Color::Indexed(0)), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(resolve_fg(Color::Indexed(15)), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn indexed_color_cube() {
        // Index 16 = (0,0,0) in the 6x6x6 cube → black.
        let c = indexed_color(16);
        assert_eq!(c, [0.0, 0.0, 0.0, 1.0]);
        // Index 231 = (5,5,5) → white.
        let c = indexed_color(231);
        assert_eq!(c, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn indexed_color_greyscale() {
        let c = indexed_color(232);
        assert_eq!(c[3], 1.0);
        let c = indexed_color(255);
        assert!((c[0] - 1.0).abs() < 1e-6);
    }
}

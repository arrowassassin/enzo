//! wgpu + cosmic-text GPU renderer.
//!
//! One glyph-atlas texture drives all rendering: solid backgrounds come from the
//! pre-filled white cell at atlas (0,0) (see [`atlas::GlyphAtlas::solid_rect`]);
//! glyphs sample the per-character alpha mask packed beside it.
//!
//! Layout (columns): `DOCK_COLS` | `PANEL_COLS` | content
//! Layout (rows):    header row  | surface rows  | footer row

mod atlas;

use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::window::Window;

use atlas::{ATLAS_H, ATLAS_W, GlyphAtlas};

use crate::surface::{BrowserPanel, BrowserState, DbState, IdeState, Surface};
use crate::terminal::{Color, Terminal};
use crate::ui::UiState;

// ── Layout constants ──────────────────────────────────────────────────────────

/// Logical font size in physical pixels *before* `HiDPI` scaling.
const FONT_SIZE: f32 = 14.0;
/// Sidebar dock width (cells).
const DOCK_COLS: u16 = 4;
/// Side-panel width (cells).
const PANEL_COLS: u16 = 20;
/// Maximum quads pre-allocated (cells + backgrounds + chrome).
const MAX_QUADS: usize = 8192;

// ── Design-system colours (sRGB linear) ──────────────────────────────────────

// Backgrounds
const BG_BASE: wgpu::Color = wgpu::Color {
    r: 0.055,
    g: 0.047,
    b: 0.078,
    a: 1.0,
}; // #0e0c14
const BG_DOCK: [f32; 4] = [0.071, 0.059, 0.102, 1.0]; // #120f1a
const BG_PANEL: [f32; 4] = [0.102, 0.086, 0.149, 1.0]; // #1a1626
const BG_CHROME: [f32; 4] = [0.133, 0.114, 0.188, 1.0]; // #221d30
const BG_CONTENT: [f32; 4] = [0.086, 0.075, 0.122, 1.0]; // #16131f
const BG_ACCENT: [f32; 4] = [0.325, 0.290, 0.718, 1.0]; // #534ab7  active tab / selection

// Text
const FG_PRIMARY: [f32; 4] = [0.910, 0.894, 0.961, 1.0]; // #e8e4f5
const FG_SECONDARY: [f32; 4] = [0.624, 0.592, 0.769, 1.0]; // #9f97c4
const FG_DIMMED: [f32; 4] = [0.373, 0.369, 0.431, 1.0]; // #5f5e6e
const FG_TEAL: [f32; 4] = [0.365, 0.792, 0.647, 1.0]; // #5dcaa5
const FG_PURPLE: [f32; 4] = [0.498, 0.467, 0.867, 1.0]; // #7f77dd
const FG_ORANGE: [f32; 4] = [0.937, 0.624, 0.153, 1.0]; // #ef9f27
const FG_RED: [f32; 4] = [0.886, 0.294, 0.290, 1.0]; // #e24b4a
const FG_GREEN: [f32; 4] = [0.388, 0.600, 0.133, 1.0]; // #639922
const FG_WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

// Code-specific
const FG_KW: [f32; 4] = [0.686, 0.663, 0.925, 1.0]; // #afa9ec  keyword
const FG_FUNC: [f32; 4] = [0.522, 0.718, 0.922, 1.0]; // #85b7eb  function

// ── Vertex layout ─────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    fg: [f32; 4],
}

// ── Layout helper ─────────────────────────────────────────────────────────────

/// Pre-computed dimensions used by every draw call — avoids `too_many_arguments`.
#[derive(Clone, Copy)]
struct Layout {
    cw: f32, // cell width  (physical px)
    ch: f32, // cell height (physical px)
    sw: f32, // screen width  (physical px)
    sh: f32, // screen height (physical px)
    total_cols: u16,
    total_rows: u16,
    content_rows: u16, // total_rows - 2 (header + footer)
    content_col: u16,  // first column of the content area
    content_cols: u16, // width of content area
}

impl Layout {
    fn new(cw: f32, ch: f32, sw: f32, sh: f32) -> Self {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "sw/cw and sh/ch are both small positive"
        )]
        let (total_cols, total_rows) = ((sw / cw) as u16, (sh / ch) as u16);
        let content_col = DOCK_COLS + PANEL_COLS;
        let content_cols = total_cols.saturating_sub(content_col);
        let content_rows = total_rows.saturating_sub(2);
        Self {
            cw,
            ch,
            sw,
            sh,
            total_cols,
            total_rows,
            content_rows,
            content_col,
            content_cols,
        }
    }
}

// ── Public render input ───────────────────────────────────────────────────────

/// All state the renderer needs for one frame.
pub struct RenderInput<'a> {
    /// Active terminal for the current tab.
    pub terminal: &'a Terminal,
    /// Tab bar and connection state.
    pub ui: &'a UiState,
    /// Which top-level surface to draw.
    pub surface: Surface,
    /// IDE surface state (file explorer + open file).
    pub ide: &'a IdeState,
    /// Database surface state (SQL editor + results).
    pub db: &'a DbState,
    /// Browser surface state (URL + devtools panels).
    pub browser: &'a BrowserState,
}

// ── Renderer ──────────────────────────────────────────────────────────────────

/// wgpu-backed GPU renderer for the full application.
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
    #[allow(
        clippy::too_many_lines,
        clippy::cast_possible_truncation,
        reason = "wgpu setup is inherently verbose; scale_factor is always in (0,4] — safe f32"
    )]
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;

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
            label:  Some("glyph_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module:      &shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode:    wgpu::VertexStepMode::Vertex,
                    attributes:   &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module:      &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend:      Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive:    wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample:  wgpu::MultisampleState::default(),
            multiview:    None,
            cache:        None,
        });

        let vb_size = (MAX_QUADS * 4 * std::mem::size_of::<Vertex>()) as u64;
        let ib_size = (MAX_QUADS * 6 * std::mem::size_of::<u16>()) as u64;
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
            atlas: GlyphAtlas::new(FONT_SIZE * scale),
            verts: Vec::with_capacity(MAX_QUADS * 4),
            indices: Vec::with_capacity(MAX_QUADS * 6),
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

    /// Render one frame for the given surface.
    pub fn render(&mut self, input: &RenderInput<'_>) {
        self.build_frame(input);
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
                        load: wgpu::LoadOp::Clear(BG_BASE),
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

    // ── Frame builder ─────────────────────────────────────────────────────────

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "cell/atlas dims are small; safe cast"
    )]
    fn build_frame(&mut self, input: &RenderInput<'_>) {
        self.verts.clear();
        self.indices.clear();

        let cw = self.atlas.cell_w as f32;
        let ch = self.atlas.cell_h as f32;
        let sw = self.width as f32;
        let sh = self.height as f32;
        let lay = Layout::new(cw, ch, sw, sh);

        // ── 1. Background fills (drawn first so text renders on top) ──────────
        // Header row
        self.push_bg(0, 0, lay.total_cols, 1, BG_CHROME, &lay);
        // Dock column
        self.push_bg(1, 0, DOCK_COLS, lay.content_rows, BG_DOCK, &lay);
        // Side panel
        self.push_bg(1, DOCK_COLS, PANEL_COLS, lay.content_rows, BG_PANEL, &lay);
        // Content area
        self.push_bg(
            1,
            lay.content_col,
            lay.content_cols,
            lay.content_rows,
            BG_CONTENT,
            &lay,
        );
        // Footer row
        let footer_row = lay.total_rows.saturating_sub(1);
        self.push_bg(footer_row, 0, lay.total_cols, 1, BG_CHROME, &lay);

        // Active tab highlight in header
        self.build_header_bg(input.ui, &lay);

        // ── 2. Header (tab bar) ───────────────────────────────────────────────
        self.build_header(input.ui, &lay);

        // ── 3. Dock icons ─────────────────────────────────────────────────────
        self.build_dock(input.surface, &lay);

        // ── 4. Side panel ─────────────────────────────────────────────────────
        match input.surface {
            Surface::Terminal => self.build_panel_terminal(input.ui, &lay),
            Surface::Ide => self.build_panel_ide(input.ide, &lay),
            Surface::Database => self.build_panel_db(input.db, &lay),
            Surface::Browser => self.build_panel_browser(input.browser, &lay),
        }

        // ── 5. Content ────────────────────────────────────────────────────────
        match input.surface {
            Surface::Terminal => self.build_content_terminal(input.terminal, &lay),
            Surface::Ide => self.build_content_ide(input.ide, &lay),
            Surface::Database => self.build_content_db(input.db, &lay),
            Surface::Browser => self.build_content_browser(input.browser, &lay),
        }

        // ── 6. Footer (status bar) ────────────────────────────────────────────
        self.build_footer(input, &lay);
    }

    // ── Header ───────────────────────────────────────────────────────────────

    fn build_header_bg(&mut self, ui: &UiState, lay: &Layout) {
        // Highlight background of the active tab.
        let mut col: u16 = 2; // skip logo area
        col += 8; // " > enzo  "  (9 chars, but logo section is ~9 chars wide)
        for (i, tab) in ui.tabs().iter().enumerate() {
            let seg_len = u16::try_from(2 + tab.title.chars().take(8).count() + 3).unwrap_or(13);
            if i == ui.active_index() {
                self.push_bg(0, col, seg_len, 1, BG_ACCENT, lay);
            }
            col = col.saturating_add(seg_len);
        }
    }

    fn build_header(&mut self, ui: &UiState, lay: &Layout) {
        let mut col = self.push_str(0, 0, " > enzo  ", FG_TEAL, lay);
        for (i, tab) in ui.tabs().iter().enumerate() {
            let is_active = i == ui.active_index();
            let fg = if is_active { FG_WHITE } else { FG_SECONDARY };
            let title: String = tab.title.chars().take(8).collect();
            let n = u16::try_from(i + 1).unwrap_or(99);
            let mark = if is_active { "*" } else { " " };
            let seg = format!(" {n}:{title}{mark} ");
            col = self.push_str(0, col, &seg, fg, lay);
        }
        self.push_str(0, col, " [+] ", FG_DIMMED, lay);

        // Right-aligned ATP status
        let (status, fg) = if ui.connected {
            (" ATP * ", FG_TEAL)
        } else {
            (" ATP o ", FG_DIMMED)
        };
        let status_len = u16::try_from(status.len()).unwrap_or(7);
        let status_col = lay.total_cols.saturating_sub(status_len);
        self.push_str(0, status_col, status, fg, lay);
    }

    // ── Dock ─────────────────────────────────────────────────────────────────

    fn build_dock(&mut self, active: Surface, lay: &Layout) {
        let icons: &[(Surface, &str)] = &[
            (Surface::Terminal, " $ "),
            (Surface::Ide, "<> "),
            (Surface::Browser, " @ "),
            (Surface::Database, "db "),
        ];
        for (i, &(surf, icon)) in icons.iter().enumerate() {
            let row = 2 + u16::try_from(i * 2).unwrap_or(0);
            let fg = if surf == active { FG_TEAL } else { FG_DIMMED };
            self.push_str(row, 0, icon, fg, lay);
        }
        // AI and settings near bottom
        let ai_row = lay.total_rows.saturating_sub(4);
        let set_row = lay.total_rows.saturating_sub(2);
        self.push_str(ai_row, 0, " * ", FG_PURPLE, lay);
        self.push_str(set_row, 0, " ~ ", FG_DIMMED, lay);
    }

    // ── Terminal panel ────────────────────────────────────────────────────────

    fn build_panel_terminal(&mut self, ui: &UiState, lay: &Layout) {
        self.push_str(1, DOCK_COLS, " SESSIONS ", FG_PURPLE, lay);
        for (i, tab) in ui
            .tabs()
            .iter()
            .take(lay.content_rows as usize - 2)
            .enumerate()
        {
            let row = 2 + u16::try_from(i).unwrap_or(0);
            let is_active = i == ui.active_index();
            let (fg, prefix) = if is_active {
                (FG_PRIMARY, "> ")
            } else {
                (FG_SECONDARY, "  ")
            };
            let label: String = format!("{prefix}{}", tab.title)
                .chars()
                .take(PANEL_COLS as usize - 1)
                .collect();
            if is_active {
                self.push_bg(row, DOCK_COLS, PANEL_COLS, 1, BG_ACCENT, lay);
            }
            self.push_str(row, DOCK_COLS, &label, fg, lay);
        }
    }

    // ── IDE panel ────────────────────────────────────────────────────────────

    fn build_panel_ide(&mut self, ide: &IdeState, lay: &Layout) {
        self.push_str(1, DOCK_COLS, " EXPLORER ", FG_PURPLE, lay);
        let max_rows = (lay.content_rows as usize).saturating_sub(2);
        for (i, entry) in ide.entries.iter().take(max_rows).enumerate() {
            let row = 2 + u16::try_from(i).unwrap_or(0);
            let is_sel = i == ide.selected;
            if is_sel {
                self.push_bg(row, DOCK_COLS, PANEL_COLS, 1, BG_ACCENT, lay);
            }
            let indent = entry.depth * 2;
            let prefix = if entry.is_dir { "v " } else { "  " };
            let name_start = indent + 2;
            let max_name = (PANEL_COLS as usize).saturating_sub(name_start + 1);
            let name: String = entry.name.chars().take(max_name).collect();
            let spaces: String = " ".repeat(indent);
            let label = format!("{spaces}{prefix}{name}");
            let fg = if is_sel {
                FG_WHITE
            } else if entry.is_dir {
                FG_TEAL
            } else {
                FG_SECONDARY
            };
            self.push_str(row, DOCK_COLS, &label, fg, lay);
        }
    }

    // ── Database panel ────────────────────────────────────────────────────────

    fn build_panel_db(&mut self, db: &DbState, lay: &Layout) {
        self.push_str(1, DOCK_COLS, " CONNECTIONS ", FG_PURPLE, lay);
        self.push_bg(2, DOCK_COLS, PANEL_COLS, 1, BG_ACCENT, lay);
        let conn: String = db
            .active_conn
            .chars()
            .take(PANEL_COLS as usize - 2)
            .collect();
        self.push_str(2, DOCK_COLS, &format!(" {conn}"), FG_WHITE, lay);
    }

    // ── Browser panel ────────────────────────────────────────────────────────

    fn build_panel_browser(&mut self, browser: &BrowserState, lay: &Layout) {
        self.push_str(1, DOCK_COLS, " DEVTOOLS ", FG_PURPLE, lay);
        let panels = [
            (BrowserPanel::Page, "Page"),
            (BrowserPanel::Network, "Network"),
            (BrowserPanel::Console, "Console"),
        ];
        for (i, (panel, label)) in panels.iter().enumerate() {
            let row = 2 + u16::try_from(i).unwrap_or(0);
            let is_active = browser.panel == *panel;
            let fg = if is_active { FG_WHITE } else { FG_SECONDARY };
            if is_active {
                self.push_bg(row, DOCK_COLS, PANEL_COLS, 1, BG_ACCENT, lay);
            }
            let text = format!(" {label}");
            self.push_str(row, DOCK_COLS, &text, fg, lay);
        }
    }

    // ── Terminal content ──────────────────────────────────────────────────────

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "terminal coords are small; content_col fits in u16"
    )]
    fn build_content_terminal(&mut self, terminal: &Terminal, lay: &Layout) {
        let (cursor_col, cursor_row) = terminal.cursor();
        let cols = terminal.cols();
        let cells = terminal.cells();
        let max_col = lay.content_cols.min(cols);

        for row in 0..lay.content_rows.min(terminal.rows()) {
            for col in 0..max_col {
                let cell = cells[row as usize * cols as usize + col as usize];
                let is_cursor = col == cursor_col && row == cursor_row;
                let ch = if is_cursor && cell.ch == ' ' {
                    '_'
                } else {
                    cell.ch
                };
                if ch == ' ' && !is_cursor {
                    continue;
                }
                let visual_row = row + 1;
                let visual_col = col + lay.content_col;
                let fg = resolve_fg(cell.style.fg);
                self.push_char(visual_row, visual_col, ch, fg, lay);
            }
        }
    }

    // ── IDE content ───────────────────────────────────────────────────────────

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "scroll and line count are bounded by file size << usize::MAX; safe"
    )]
    fn build_content_ide(&mut self, ide: &IdeState, lay: &Layout) {
        if ide.open_path.is_none() {
            let hint = "Enter: open file   arrows: navigate";
            self.push_str(
                lay.content_rows / 2,
                lay.content_col + 2,
                hint,
                FG_DIMMED,
                lay,
            );
            return;
        }

        let gutter_w: u16 = 5; // "  42 "
        let code_col = lay.content_col + gutter_w;
        let max_code_cols = lay.content_cols.saturating_sub(gutter_w) as usize;

        for (i, line) in ide.lines.iter().enumerate().skip(ide.scroll) {
            let display_row = i - ide.scroll;
            if display_row >= lay.content_rows as usize {
                break;
            }
            let row = 1 + u16::try_from(display_row).unwrap_or(0);
            let lineno = i + 1;

            // Cursor line highlight
            if i == ide.cursor_line {
                self.push_bg(row, lay.content_col, lay.content_cols, 1, BG_PANEL, lay);
            }

            // Line number
            let gutter = format!("{lineno:>4} ");
            self.push_str(row, lay.content_col, &gutter, FG_DIMMED, lay);

            // Code with very basic keyword highlighting
            let visible: String = line.chars().take(max_code_cols).collect();
            self.push_highlighted(row, code_col, &visible, lay);
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "line length and col offset are bounded by content area width << u16::MAX"
    )]
    fn push_highlighted(&mut self, row: u16, start_col: u16, line: &str, lay: &Layout) {
        let keywords = [
            "fn", "let", "mut", "pub", "use", "impl", "struct", "enum", "match", "if", "else",
            "for", "while", "return", "self", "Self", "true", "false", "mod", "type", "async",
            "await", "const",
        ];
        // Simple char-by-char scan: colour keyword spans, dim comments, rest primary
        let chars: Vec<char> = line.chars().collect();
        let n = chars.len();
        let mut col = start_col;
        let mut i = 0usize;

        while i < n {
            // Comment
            if i + 1 < n && chars[i] == '/' && chars[i + 1] == '/' {
                let rest: String = chars[i..].iter().collect();
                self.push_str(row, col, &rest, FG_DIMMED, lay);
                break;
            }
            // String literal
            if chars[i] == '"' {
                let mut j = i + 1;
                while j < n && !(chars[j] == '"' && chars[j - 1] != '\\') {
                    j += 1;
                }
                let s: String = chars[i..=j.min(n - 1)].iter().collect();
                self.push_str(row, col, &s, FG_ORANGE, lay);
                col = col.saturating_add(u16::try_from(s.len()).unwrap_or(0));
                i = j + 1;
                continue;
            }
            // Try keyword match
            let mut matched = false;
            for kw in keywords {
                let kl = kw.len();
                if i + kl <= n {
                    let slice: String = chars[i..i + kl].iter().collect();
                    let after_ok =
                        i + kl >= n || !chars[i + kl].is_alphanumeric() && chars[i + kl] != '_';
                    let before_ok =
                        i == 0 || !chars[i - 1].is_alphanumeric() && chars[i - 1] != '_';
                    if slice == kw && before_ok && after_ok {
                        self.push_str(row, col, kw, FG_KW, lay);
                        col = col.saturating_add(u16::try_from(kl).unwrap_or(0));
                        i += kl;
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                // Single char: pick colour
                let ch = chars[i];
                let fg = if ch.is_ascii_uppercase() {
                    FG_FUNC
                } else {
                    FG_PRIMARY
                };
                if ch != ' ' {
                    self.push_char(row, col, ch, fg, lay);
                }
                col = col.saturating_add(1);
                i += 1;
            }
        }
    }

    // ── Database content ──────────────────────────────────────────────────────

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "content area is small; safe cast"
    )]
    fn build_content_db(&mut self, db: &DbState, lay: &Layout) {
        let cc = lay.content_col;
        let ccols = lay.content_cols as usize;

        // SQL editor area (top ~4 rows)
        self.push_str(1, cc, " SQL ", FG_PURPLE, lay);
        let query_display: String = db.query.chars().take(ccols.saturating_sub(2)).collect();
        self.push_str(2, cc + 1, &query_display, FG_PRIMARY, lay);

        // Cursor bar
        let cursor_col =
            u16::try_from(db.query[..db.cursor.min(db.query.len())].chars().count()).unwrap_or(0);
        self.push_char(2, cc + 1 + cursor_col, '_', FG_TEAL, lay);

        // Divider row
        self.push_bg(4, cc, lay.content_cols, 1, BG_PANEL, lay);
        let divider = format!(" {} rows", db.rows.len());
        self.push_str(4, cc, &divider, FG_SECONDARY, lay);
        if let Some(ms) = db.query_ms {
            let t = format!(" {ms}ms ");
            let t_col = lay.content_col + lay.content_cols - u16::try_from(t.len()).unwrap_or(0);
            self.push_str(4, t_col, &t, FG_TEAL, lay);
        }

        if let Some(err) = &db.error {
            let msg: String = err.chars().take(ccols.saturating_sub(2)).collect();
            self.push_str(5, cc + 1, &msg, FG_RED, lay);
            return;
        }

        // Column headers
        if !db.columns.is_empty() {
            self.push_bg(5, cc, lay.content_cols, 1, BG_DOCK, lay);
            let col_w = (ccols / db.columns.len()).max(1);
            for (ci, col_name) in db.columns.iter().enumerate() {
                let text: String = col_name.chars().take(col_w.saturating_sub(1)).collect();
                let c = cc + u16::try_from(ci * col_w).unwrap_or(0);
                self.push_str(5, c, &text, FG_TEAL, lay);
            }
        }

        // Rows
        let col_w = if db.columns.is_empty() {
            1
        } else {
            (ccols / db.columns.len()).max(1)
        };
        for (ri, row) in db.rows.iter().enumerate().skip(db.result_scroll) {
            let display_row = ri - db.result_scroll;
            let r = 6 + u16::try_from(display_row).unwrap_or(0);
            if r >= lay.total_rows.saturating_sub(1) {
                break;
            }
            for (ci, cell_val) in row.iter().enumerate() {
                let text: String = cell_val.chars().take(col_w.saturating_sub(1)).collect();
                let c = cc + u16::try_from(ci * col_w).unwrap_or(0);
                self.push_str(r, c, &text, FG_SECONDARY, lay);
            }
        }
    }

    // ── Browser content ───────────────────────────────────────────────────────

    fn build_content_browser(&mut self, browser: &BrowserState, lay: &Layout) {
        let cc = lay.content_col;
        let ccols = lay.content_cols as usize;

        // URL bar
        self.push_bg(1, cc, lay.content_cols, 1, BG_DOCK, lay);
        let url: String = browser.url.chars().take(ccols.saturating_sub(4)).collect();
        self.push_str(1, cc + 2, &url, FG_PRIMARY, lay);

        // Sub-panel tabs
        let tab_labels = [" Page ", " Network ", " Console "];
        let mut tab_col = cc + 2;
        for (i, label) in tab_labels.iter().enumerate() {
            let panel = match i {
                0 => BrowserPanel::Page,
                1 => BrowserPanel::Network,
                _ => BrowserPanel::Console,
            };
            let is_active = browser.panel == panel;
            let fg = if is_active { FG_WHITE } else { FG_SECONDARY };
            if is_active {
                let len = u16::try_from(label.len()).unwrap_or(0);
                self.push_bg(2, tab_col, len, 1, BG_ACCENT, lay);
            }
            tab_col = self.push_str(2, tab_col, label, fg, lay);
        }

        match browser.panel {
            BrowserPanel::Page => {
                let msg = "[Browser surface — CEF integration pending]";
                self.push_str(lay.content_rows / 2, cc + 2, msg, FG_DIMMED, lay);
            }
            BrowserPanel::Network => {
                // Header
                self.push_bg(3, cc, lay.content_cols, 1, BG_DOCK, lay);
                self.push_str(3, cc, " METHOD", FG_TEAL, lay);
                self.push_str(3, cc + 8, " STATUS", FG_TEAL, lay);
                self.push_str(3, cc + 15, " ms", FG_TEAL, lay);
                self.push_str(3, cc + 20, " PATH", FG_TEAL, lay);

                for (i, req) in browser.requests.iter().enumerate() {
                    let r = 4 + u16::try_from(i).unwrap_or(0);
                    if r >= lay.total_rows.saturating_sub(1) {
                        break;
                    }
                    let status_fg = if req.status >= 400 { FG_RED } else { FG_GREEN };
                    self.push_str(r, cc, &format!(" {:<6}", req.method), FG_SECONDARY, lay);
                    self.push_str(r, cc + 8, &format!(" {}", req.status), status_fg, lay);
                    self.push_str(r, cc + 15, &format!(" {}ms", req.ms), FG_DIMMED, lay);
                    let path: String = req.path.chars().take(ccols.saturating_sub(21)).collect();
                    self.push_str(r, cc + 20, &format!(" {path}"), FG_PRIMARY, lay);
                }
            }
            BrowserPanel::Console => {
                for (i, line) in browser.console_lines.iter().enumerate() {
                    let r = 4 + u16::try_from(i).unwrap_or(0);
                    if r >= lay.total_rows.saturating_sub(1) {
                        break;
                    }
                    let fg = if line.starts_with("[ERR]") {
                        FG_RED
                    } else if line.starts_with("[WARN]") {
                        FG_ORANGE
                    } else {
                        FG_SECONDARY
                    };
                    let text: String = line.chars().take(lay.content_cols as usize - 2).collect();
                    self.push_str(r, cc + 1, &text, fg, lay);
                }
            }
        }
    }

    // ── Footer ────────────────────────────────────────────────────────────────

    fn build_footer(&mut self, input: &RenderInput<'_>, lay: &Layout) {
        let row = lay.total_rows.saturating_sub(1);

        let left = match input.surface {
            Surface::Terminal => {
                let (c, r) = input.terminal.cursor();
                format!(
                    " PTY zsh  {}x{}",
                    input.terminal.cols(),
                    input.terminal.rows()
                ) + &format!("  {c},{r}")
            }
            Surface::Ide => {
                let fname = input.ide.open_path.as_deref().unwrap_or("no file");
                format!(" {fname}  Ln {}", input.ide.cursor_line + 1)
            }
            Surface::Database => {
                format!(" {}  SQL editor", input.db.active_conn)
            }
            Surface::Browser => {
                format!(" {}", input.browser.url)
            }
        };
        let left_short: String = left.chars().take(lay.total_cols as usize / 2).collect();
        self.push_str(row, 0, &left_short, FG_SECONDARY, lay);

        // Right side: connection + surface indicator
        let surf_label = match input.surface {
            Surface::Terminal => "TERM",
            Surface::Ide => "IDE ",
            Surface::Database => "DB  ",
            Surface::Browser => "WEB ",
        };
        let (conn_label, conn_fg) = if input.ui.connected {
            (" ATP * ", FG_TEAL)
        } else {
            (" ATP o ", FG_DIMMED)
        };
        let right = format!(" {surf_label}{conn_label}");
        let right_len = u16::try_from(right.len()).unwrap_or(0);
        let right_col = lay.total_cols.saturating_sub(right_len);
        self.push_str(row, right_col, &format!(" {surf_label}"), FG_PURPLE, lay);
        let conn_col = right_col + 1 + u16::try_from(surf_label.len()).unwrap_or(4);
        self.push_str(row, conn_col, conn_label, conn_fg, lay);
    }

    // ── Primitive quad emitters ───────────────────────────────────────────────

    /// Emit a solid-coloured background rectangle (cell-space coordinates).
    #[allow(
        clippy::cast_precision_loss,
        clippy::too_many_arguments,
        reason = "all values are small cell coords; Layout replaces most args"
    )]
    fn push_bg(&mut self, row: u16, col: u16, w: u16, h: u16, color: [f32; 4], lay: &Layout) {
        let sr = self.atlas.solid_rect;
        let base = u16::try_from(self.verts.len()).unwrap_or(u16::MAX);

        let x0 = f32::from(col) * lay.cw / lay.sw * 2.0 - 1.0;
        let x1 = f32::from(col + w) * lay.cw / lay.sw * 2.0 - 1.0;
        let y0 = 1.0 - f32::from(row) * lay.ch / lay.sh * 2.0;
        let y1 = 1.0 - f32::from(row + h) * lay.ch / lay.sh * 2.0;

        // Centre UV of the solid-white atlas cell — alpha = 1.0 everywhere.
        let u = (sr.x as f32 + sr.w as f32 * 0.5) / ATLAS_W as f32;
        let v = (sr.y as f32 + sr.h as f32 * 0.5) / ATLAS_H as f32;

        self.verts.extend_from_slice(&[
            Vertex {
                pos: [x0, y0],
                uv: [u, v],
                fg: color,
            },
            Vertex {
                pos: [x1, y0],
                uv: [u, v],
                fg: color,
            },
            Vertex {
                pos: [x1, y1],
                uv: [u, v],
                fg: color,
            },
            Vertex {
                pos: [x0, y1],
                uv: [u, v],
                fg: color,
            },
        ]);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Emit one glyph quad at the given visual cell position.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "cell coords ≤ 220; atlas ≤ 2048; safe"
    )]
    fn push_char(&mut self, row: u16, col: u16, ch: char, fg: [f32; 4], lay: &Layout) {
        let rect = self.atlas.get_or_insert(ch);
        let base = u16::try_from(self.verts.len()).unwrap_or(u16::MAX);

        let x0 = f32::from(col) * lay.cw / lay.sw * 2.0 - 1.0;
        let x1 = f32::from(col + 1) * lay.cw / lay.sw * 2.0 - 1.0;
        let y0 = 1.0 - f32::from(row) * lay.ch / lay.sh * 2.0;
        let y1 = 1.0 - f32::from(row + 1) * lay.ch / lay.sh * 2.0;

        let aw = ATLAS_W as f32;
        let ah = ATLAS_H as f32;
        let u0 = rect.x as f32 / aw;
        let u1 = (rect.x + rect.w) as f32 / aw;
        let v0 = rect.y as f32 / ah;
        let v1 = (rect.y + rect.h) as f32 / ah;

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
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Render each non-space character in `text` starting at `(row, col)`.
    /// Returns the column *after* the last rendered character.
    fn push_str(&mut self, row: u16, col: u16, text: &str, fg: [f32; 4], lay: &Layout) -> u16 {
        let mut c = col;
        for ch in text.chars() {
            if ch != ' ' {
                self.push_char(row, c, ch, fg, lay);
            }
            c = c.saturating_add(1);
        }
        c
    }

    // ── Atlas upload ──────────────────────────────────────────────────────────

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

// ── Colour mapping ────────────────────────────────────────────────────────────

/// Convert a terminal foreground colour to linear RGBA.
#[must_use]
pub fn resolve_fg(color: Color) -> [f32; 4] {
    match color {
        Color::Default => FG_TEAL,
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
        0 => [0.00, 0.00, 0.00, 1.0],
        1 => [0.80, 0.00, 0.00, 1.0],
        2 => [0.00, 0.80, 0.00, 1.0],
        3 => [0.80, 0.80, 0.00, 1.0],
        4 => [0.00, 0.00, 0.80, 1.0],
        5 => [0.80, 0.00, 0.80, 1.0],
        6 => [0.00, 0.80, 0.80, 1.0],
        7 => [0.80, 0.80, 0.80, 1.0],
        8 => [0.40, 0.40, 0.40, 1.0],
        9 => [1.00, 0.20, 0.20, 1.0],
        10 => [0.20, 1.00, 0.20, 1.0],
        11 => [1.00, 1.00, 0.20, 1.0],
        12 => [0.20, 0.20, 1.00, 1.0],
        13 => [1.00, 0.20, 1.00, 1.0],
        14 => [0.20, 1.00, 1.00, 1.0],
        15 => [1.00, 1.00, 1.00, 1.0],
        16..=231 => {
            let i = idx - 16;
            let r = f32::from(i / 36) / 5.0;
            let g = f32::from((i / 6) % 6) / 5.0;
            let b = f32::from(i % 6) / 5.0;
            [r, g, b, 1.0]
        }
        232..=255 => {
            let l = f32::from(idx - 232) / 23.0;
            [l, l, l, 1.0]
        }
    }
}

// ── WGSL shader ───────────────────────────────────────────────────────────────

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "test values are exact integer fractions")]
mod tests {
    use super::*;
    use crate::terminal::Color;

    #[test]
    fn resolve_fg_default_is_teal() {
        let c = resolve_fg(Color::Default);
        assert_eq!(c, FG_TEAL);
    }

    #[test]
    fn resolve_fg_rgb() {
        let c = resolve_fg(Color::Rgb(255, 0, 0));
        assert!((c[0] - 1.0).abs() < 1e-6);
        assert!(c[1].abs() < 1e-6);
        assert!(c[2].abs() < 1e-6);
    }

    #[test]
    fn resolve_fg_indexed_ansi() {
        assert_eq!(resolve_fg(Color::Indexed(0)), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(resolve_fg(Color::Indexed(15)), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn layout_content_cols() {
        let lay = Layout::new(14.0, 17.0, 1400.0, 850.0);
        assert_eq!(lay.total_cols, 100);
        assert_eq!(lay.content_col, DOCK_COLS + PANEL_COLS);
        assert!(lay.content_cols > 0);
    }

    #[test]
    fn indexed_color_cube_bounds() {
        assert_eq!(indexed_color(16), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(indexed_color(231), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn indexed_color_greyscale() {
        let c = indexed_color(232);
        assert_eq!(c[3], 1.0);
        let c = indexed_color(255);
        assert!((c[0] - 1.0).abs() < 1e-6);
    }
}

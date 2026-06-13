//! Compositor latency spike — measures keystroke→glyph latency with wgpu + cosmic-text.
//! This binary is deliberately throwaway; none of this code ships.
//! See docs/SPIKE-compositor.md for the decision matrix and pass/fail thresholds.
//!
//! Run:  cargo run -p enzo-compositor-spike --release
//!
//! Controls:
//!   Type anything  — echoed at cursor, keystroke timestamp recorded
//!   Enter          — newline
//!   Escape         — print latency report and exit

#![allow(clippy::pedantic, missing_docs)]

mod atlas;
mod grid;
mod metrics;
mod renderer;

use std::sync::Arc;
use std::time::Instant;

use log::info;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::grid::TermGrid;
use crate::metrics::LatencyTracker;
use crate::renderer::Renderer;

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run event loop");
}

#[derive(Default)]
struct App {
    state: Option<AppState>,
}

struct AppState {
    window: Arc<Window>,
    renderer: Renderer,
    grid: TermGrid,
    metrics: LatencyTracker,
    /// Timestamp set on KeyEvent::pressed, cleared after the next rendered frame.
    key_pressed_at: Option<Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Enzo compositor spike — type, press Esc to report")
            .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 720u32));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)));
        let grid = TermGrid::new(200, 50);
        self.state = Some(AppState {
            window,
            renderer,
            grid,
            metrics: LatencyTracker::new(),
            key_pressed_at: None,
        });
        info!("window created");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else { return };

        match event {
            WindowEvent::CloseRequested => {
                state.metrics.report();
                event_loop.exit();
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                state.metrics.report();
                event_loop.exit();
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Enter),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                state.key_pressed_at = Some(Instant::now());
                state.grid.newline();
                state.window.request_redraw();
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Character(ref s),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                state.key_pressed_at = Some(Instant::now());
                for ch in s.chars() {
                    state.grid.put_char(ch);
                }
                state.window.request_redraw();
            }

            WindowEvent::Resized(size) => {
                state.renderer.resize(size);
            }

            WindowEvent::RedrawRequested => {
                state.renderer.render(&state.grid);
                if let Some(t) = state.key_pressed_at.take() {
                    state.metrics.record_keystroke_latency(t.elapsed());
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }
}

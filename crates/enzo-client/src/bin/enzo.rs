//! Enzo terminal client — GPU-accelerated terminal that talks to enzo-daemon
//! over the ATP (Agent Terminal Protocol) Unix socket.
//!
//! Usage:
//!   cargo run -p enzo-client --release
//!
//! Environment:
//!   `ENZO_ATP_SOCK`  Override the daemon socket path (default: /tmp/enzo-atp.sock)
//!   `RUST_LOG`       Log level (e.g. `debug`)

use std::sync::Arc;

use log::error;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use enzo_client::atp::{AtpClient, DaemonMessage};
use enzo_client::renderer::Renderer;
use enzo_client::terminal::{DEFAULT_COLS, DEFAULT_ROWS, Terminal};

const DEFAULT_SOCK: &str = "/tmp/enzo-atp.sock";

// ── User event (tokio → winit) ───────────────────────────────────────────────

#[derive(Debug)]
enum ClientEvent {
    Output(Vec<u8>),
    DaemonClosed,
}

// ── winit app ────────────────────────────────────────────────────────────────

struct App {
    input_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    state: Option<AppState>,
}

struct AppState {
    window: Arc<Window>,
    renderer: Renderer,
    terminal: Terminal,
}

impl App {
    fn new(input_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>) -> Self {
        Self {
            input_tx,
            state: None,
        }
    }
}

impl ApplicationHandler<ClientEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("enzo")
            .with_inner_size(LogicalSize::new(1600u32, 900u32));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)));
        let terminal = Terminal::new(DEFAULT_COLS, DEFAULT_ROWS);
        self.state = Some(AppState {
            window,
            renderer,
            terminal,
        });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: ClientEvent) {
        let Some(state) = &mut self.state else { return };
        match event {
            ClientEvent::Output(data) => {
                state.terminal.process(&data);
                state.window.request_redraw();
            }
            ClientEvent::DaemonClosed => {
                log::warn!("daemon connection closed");
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else { return };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                state.renderer.resize(size);
                state.window.request_redraw();
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        text,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                let bytes = key_to_bytes(&logical_key, text.as_deref());
                if !bytes.is_empty() {
                    let _ = self.input_tx.send(bytes);
                }
            }

            WindowEvent::RedrawRequested => {
                state.renderer.render(&state.terminal);
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

// ── Key → bytes ──────────────────────────────────────────────────────────────

fn key_to_bytes(key: &Key, text: Option<&str>) -> Vec<u8> {
    // Use the text field when available — it carries Ctrl-modified chars
    // (e.g. Ctrl+C → "\x03") and regular printable chars already.
    if let Some(t) = text
        && !t.is_empty()
    {
        return t.as_bytes().to_vec();
    }
    // Fall back to NamedKey escape sequences.
    match key {
        Key::Named(NamedKey::Enter) => b"\r".to_vec(),
        Key::Named(NamedKey::Tab) => b"\t".to_vec(),
        Key::Named(NamedKey::Backspace) => b"\x7f".to_vec(),
        Key::Named(NamedKey::Escape) => b"\x1b".to_vec(),
        Key::Named(NamedKey::ArrowUp) => b"\x1b[A".to_vec(),
        Key::Named(NamedKey::ArrowDown) => b"\x1b[B".to_vec(),
        Key::Named(NamedKey::ArrowRight) => b"\x1b[C".to_vec(),
        Key::Named(NamedKey::ArrowLeft) => b"\x1b[D".to_vec(),
        Key::Named(NamedKey::Home) => b"\x1b[H".to_vec(),
        Key::Named(NamedKey::End) => b"\x1b[F".to_vec(),
        Key::Named(NamedKey::PageUp) => b"\x1b[5~".to_vec(),
        Key::Named(NamedKey::PageDown) => b"\x1b[6~".to_vec(),
        Key::Named(NamedKey::Insert) => b"\x1b[2~".to_vec(),
        Key::Named(NamedKey::Delete) => b"\x1b[3~".to_vec(),
        _ => vec![],
    }
}

// ── ATP background task ───────────────────────────────────────────────────────

async fn run_atp(
    sock_path: String,
    proxy: EventLoopProxy<ClientEvent>,
    mut input_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let proxy2 = proxy.clone();
    let client = match AtpClient::connect(&sock_path, move |msg| match msg {
        DaemonMessage::Output { data, .. } => {
            let _ = proxy2.send_event(ClientEvent::Output(data));
        }
        DaemonMessage::Closed => {
            let _ = proxy2.send_event(ClientEvent::DaemonClosed);
        }
    })
    .await
    {
        Ok(c) => c,
        Err(e) => {
            error!("ATP connect failed: {e:#}");
            let _ = proxy.send_event(ClientEvent::DaemonClosed);
            return;
        }
    };

    let session_id = format!("enzo-{}", std::process::id());
    if let Err(e) = client
        .spawn_session(&session_id, DEFAULT_COLS, DEFAULT_ROWS)
        .await
    {
        error!("spawn_session: {e:#}");
        return;
    }

    while let Some(bytes) = input_rx.recv().await {
        if let Err(e) = client.send_input(&session_id, &bytes).await {
            log::warn!("send_input: {e}");
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    env_logger::init();

    let sock_path = std::env::var("ENZO_ATP_SOCK").unwrap_or_else(|_| DEFAULT_SOCK.to_owned());

    let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let event_loop = EventLoop::<ClientEvent>::with_user_event()
        .build()
        .expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let proxy = event_loop.create_proxy();

    std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(run_atp(sock_path, proxy, input_rx));
    });

    let mut app = App::new(input_tx);
    event_loop.run_app(&mut app).expect("run event loop");
}

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
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use enzo_client::atp::{AtpClient, DaemonMessage};
use enzo_client::renderer::Renderer;
use enzo_client::terminal::{DEFAULT_COLS, DEFAULT_ROWS, Terminal};
use enzo_client::ui::UiState;

const DEFAULT_SOCK: &str = "/tmp/enzo-atp.sock";

// ── Commands: winit → tokio ──────────────────────────────────────────────────

enum AppCommand {
    Input {
        session_id: String,
        data: Vec<u8>,
    },
    NewSession {
        session_id: String,
        cols: u16,
        rows: u16,
    },
    CloseSession {
        session_id: String,
    },
}

// ── Events: tokio → winit ────────────────────────────────────────────────────

#[derive(Debug)]
enum ClientEvent {
    Connected,
    Output { session_id: String, data: Vec<u8> },
    DaemonClosed,
}

// ── winit app ────────────────────────────────────────────────────────────────

struct App {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<AppCommand>,
    state: Option<AppState>,
    mods: ModifiersState,
    next_session: u32,
}

struct AppState {
    window: Arc<Window>,
    renderer: Renderer,
    /// Live sessions: (`session_id`, terminal). Ordered by tab insertion.
    terminals: Vec<(String, Terminal)>,
    ui: UiState,
}

impl App {
    fn new(cmd_tx: tokio::sync::mpsc::UnboundedSender<AppCommand>) -> Self {
        Self {
            cmd_tx,
            state: None,
            mods: ModifiersState::empty(),
            next_session: 0,
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

        let session_id = {
            let id = format!("enzo-{}", self.next_session);
            self.next_session += 1;
            id
        };
        let mut ui = UiState::new();
        ui.add_tab(session_id.clone(), "bash".into());

        let _ = self.cmd_tx.send(AppCommand::NewSession {
            session_id: session_id.clone(),
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
        });

        self.state = Some(AppState {
            window,
            renderer,
            terminals: vec![(session_id, Terminal::new(DEFAULT_COLS, DEFAULT_ROWS))],
            ui,
        });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: ClientEvent) {
        let Some(state) = &mut self.state else { return };
        match event {
            ClientEvent::Connected => {
                state.ui.connected = true;
                state.window.request_redraw();
            }
            ClientEvent::Output { session_id, data } => {
                if let Some((_, term)) = state
                    .terminals
                    .iter_mut()
                    .find(|(sid, _)| *sid == session_id)
                {
                    term.process(&data);
                    if state.ui.active_session_id() == Some(session_id.as_str()) {
                        state.window.request_redraw();
                    }
                }
            }
            ClientEvent::DaemonClosed => {
                log::warn!("daemon connection closed");
                state.ui.connected = false;
                state.window.request_redraw();
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "single match over all WindowEvent variants"
    )]
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::ModifiersChanged(new_mods) => {
                self.mods = new_mods.state();
            }

            WindowEvent::Resized(size) => {
                if let Some(state) = &mut self.state {
                    state.renderer.resize(size);
                    state.window.request_redraw();
                }
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
                // Tab management shortcuts take priority over terminal input.
                if self.mods.control_key() {
                    match &logical_key {
                        Key::Character(s) if s.as_str() == "t" => {
                            let n = self.next_session;
                            self.next_session += 1;
                            let session_id = format!("enzo-{n}");
                            if let Some(st) = &mut self.state {
                                st.ui.add_tab(session_id.clone(), "bash".into());
                                st.terminals.push((
                                    session_id.clone(),
                                    Terminal::new(DEFAULT_COLS, DEFAULT_ROWS),
                                ));
                                st.window.request_redraw();
                            }
                            let _ = self.cmd_tx.send(AppCommand::NewSession {
                                session_id,
                                cols: DEFAULT_COLS,
                                rows: DEFAULT_ROWS,
                            });
                            return;
                        }
                        Key::Character(s) if s.as_str() == "w" => {
                            let closed_sid = if let Some(st) = &mut self.state {
                                let sid = st.ui.close_active();
                                if let Some(ref s) = sid {
                                    st.terminals.retain(|(id, _)| id != s);
                                }
                                st.window.request_redraw();
                                sid
                            } else {
                                None
                            };
                            if let Some(session_id) = closed_sid {
                                let _ = self.cmd_tx.send(AppCommand::CloseSession { session_id });
                            }
                            return;
                        }
                        Key::Named(NamedKey::Tab) => {
                            if let Some(st) = &mut self.state {
                                if self.mods.shift_key() {
                                    st.ui.prev_tab();
                                } else {
                                    st.ui.next_tab();
                                }
                                st.window.request_redraw();
                            }
                            return;
                        }
                        _ => {}
                    }
                }

                // Route normal key input to the active session.
                let bytes = key_to_bytes(&logical_key, text.as_deref());
                if !bytes.is_empty() {
                    let active_id = self
                        .state
                        .as_ref()
                        .and_then(|s| s.ui.active_session_id())
                        .map(str::to_owned);
                    if let Some(session_id) = active_id {
                        let _ = self.cmd_tx.send(AppCommand::Input {
                            session_id,
                            data: bytes,
                        });
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                if let Some(state) = &mut self.state {
                    // Find the active terminal index so we can split the borrow:
                    // state.terminals (immut) vs state.renderer (mut) are disjoint fields.
                    let active_idx = state
                        .ui
                        .active_session_id()
                        .map(str::to_owned)
                        .and_then(|id| state.terminals.iter().position(|(sid, _)| *sid == id));
                    if let Some(idx) = active_idx {
                        state.renderer.render(&state.terminals[idx].1, &state.ui);
                    }
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

// ── Key → bytes ──────────────────────────────────────────────────────────────

fn key_to_bytes(key: &Key, text: Option<&str>) -> Vec<u8> {
    // The text field carries Ctrl-modified chars (e.g. Ctrl+C → "\x03") and
    // regular printable chars; prefer it when present.
    if let Some(t) = text
        && !t.is_empty()
    {
        return t.as_bytes().to_vec();
    }
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
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<AppCommand>,
) {
    let proxy2 = proxy.clone();
    let client = match AtpClient::connect(&sock_path, move |msg| match msg {
        DaemonMessage::Output { session_id, data } => {
            let _ = proxy2.send_event(ClientEvent::Output { session_id, data });
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

    let _ = proxy.send_event(ClientEvent::Connected);

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            AppCommand::NewSession {
                session_id,
                cols,
                rows,
            } => {
                if let Err(e) = client.spawn_session(&session_id, cols, rows).await {
                    error!("spawn_session {session_id}: {e:#}");
                }
            }
            AppCommand::CloseSession { session_id } => {
                if let Err(e) = client.close_session(&session_id).await {
                    log::warn!("close_session {session_id}: {e}");
                }
            }
            AppCommand::Input { session_id, data } => {
                if let Err(e) = client.send_input(&session_id, &data).await {
                    log::warn!("send_input: {e}");
                }
            }
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    env_logger::init();

    let sock_path = std::env::var("ENZO_ATP_SOCK").unwrap_or_else(|_| DEFAULT_SOCK.to_owned());

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<AppCommand>();
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
            .block_on(run_atp(sock_path, proxy, cmd_rx));
    });

    let mut app = App::new(cmd_tx);
    event_loop.run_app(&mut app).expect("run event loop");
}

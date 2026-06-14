//! Enzo terminal client — GPU-accelerated developer workspace that talks to
//! enzo-daemon over the ATP (Agent Terminal Protocol) Unix socket.
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
use enzo_client::renderer::{RenderInput, Renderer};
use enzo_client::surface::{BrowserState, DbState, IdeState, Surface};
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

// ── App ───────────────────────────────────────────────────────────────────────

struct App {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<AppCommand>,
    state: Option<AppState>,
    mods: ModifiersState,
    next_session: u32,
}

struct AppState {
    window: Arc<Window>,
    renderer: Renderer,
    /// Live sessions: (`session_id`, terminal), ordered by tab insertion.
    terminals: Vec<(String, Terminal)>,
    ui: UiState,
    active_surface: Surface,
    ide: IdeState,
    db: DbState,
    browser: BrowserState,
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

        let session_id = format!("enzo-{}", self.next_session);
        self.next_session += 1;

        let mut ui = UiState::new();
        ui.add_tab(session_id.clone(), "bash".into());

        let _ = self.cmd_tx.send(AppCommand::NewSession {
            session_id: session_id.clone(),
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
        });

        let project_root = std::env::current_dir()
            .map_or_else(|_| ".".to_string(), |p| p.to_string_lossy().into_owned());

        self.state = Some(AppState {
            window,
            renderer,
            terminals: vec![(session_id, Terminal::new(DEFAULT_COLS, DEFAULT_ROWS))],
            ui,
            active_surface: Surface::Terminal,
            ide: IdeState::new(project_root),
            db: DbState::demo(),
            browser: BrowserState::demo(),
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
                self.handle_key(&logical_key, text.as_deref());
            }

            WindowEvent::RedrawRequested => {
                if let Some(state) = &mut self.state {
                    let active_idx = state
                        .ui
                        .active_session_id()
                        .map(str::to_owned)
                        .and_then(|id| state.terminals.iter().position(|(sid, _)| *sid == id));
                    if let Some(idx) = active_idx {
                        let input = RenderInput {
                            terminal: &state.terminals[idx].1,
                            ui: &state.ui,
                            surface: state.active_surface,
                            ide: &state.ide,
                            db: &state.db,
                            browser: &state.browser,
                        };
                        state.renderer.render(&input);
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

// ── Key handling ──────────────────────────────────────────────────────────────

impl App {
    #[allow(
        clippy::too_many_lines,
        reason = "single match over keyboard shortcuts"
    )]
    fn handle_key(&mut self, logical_key: &Key, text: Option<&str>) {
        // ── Global shortcuts (work on all surfaces) ───────────────────────────
        if self.mods.super_key() || self.mods.control_key() {
            match logical_key {
                // Surface switching: Cmd/Ctrl+1..4
                Key::Character(s) if s.as_str() == "1" => {
                    if let Some(st) = &mut self.state {
                        st.active_surface = Surface::Terminal;
                        st.window.request_redraw();
                    }
                    return;
                }
                Key::Character(s) if s.as_str() == "2" => {
                    if let Some(st) = &mut self.state {
                        st.active_surface = Surface::Ide;
                        st.window.request_redraw();
                    }
                    return;
                }
                Key::Character(s) if s.as_str() == "3" => {
                    if let Some(st) = &mut self.state {
                        st.active_surface = Surface::Database;
                        st.window.request_redraw();
                    }
                    return;
                }
                Key::Character(s) if s.as_str() == "4" => {
                    if let Some(st) = &mut self.state {
                        st.active_surface = Surface::Browser;
                        st.window.request_redraw();
                    }
                    return;
                }
                // New tab: Ctrl+T
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
                        st.active_surface = Surface::Terminal;
                        st.window.request_redraw();
                    }
                    let _ = self.cmd_tx.send(AppCommand::NewSession {
                        session_id,
                        cols: DEFAULT_COLS,
                        rows: DEFAULT_ROWS,
                    });
                    return;
                }
                // Close tab: Ctrl+W
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
                // Cycle tabs: Ctrl+Tab / Ctrl+Shift+Tab
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

        // ── Surface-specific input ────────────────────────────────────────────
        let surface = self.state.as_ref().map(|s| s.active_surface);
        match surface {
            Some(Surface::Terminal) => self.handle_terminal_key(logical_key, text),
            Some(Surface::Ide) => self.handle_ide_key(logical_key),
            Some(Surface::Database) => self.handle_db_key(logical_key, text),
            Some(Surface::Browser) => self.handle_browser_key(logical_key),
            None => {}
        }
    }

    fn handle_terminal_key(&mut self, logical_key: &Key, text: Option<&str>) {
        let bytes = key_to_bytes(logical_key, text);
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

    fn handle_ide_key(&mut self, key: &Key) {
        let Some(state) = &mut self.state else { return };
        match key {
            Key::Named(NamedKey::ArrowDown) => state.ide.move_selection(1),
            Key::Named(NamedKey::ArrowUp) => state.ide.move_selection(-1),
            Key::Named(NamedKey::Enter) => state.ide.open_selected(),
            Key::Named(NamedKey::PageDown) => state.ide.scroll_content(10),
            Key::Named(NamedKey::PageUp) => state.ide.scroll_content(-10),
            Key::Named(NamedKey::ArrowRight) => state.ide.scroll_content(1),
            Key::Named(NamedKey::ArrowLeft) => state.ide.scroll_content(-1),
            _ => {}
        }
        state.window.request_redraw();
    }

    fn handle_db_key(&mut self, key: &Key, text: Option<&str>) {
        let Some(state) = &mut self.state else { return };
        match key {
            Key::Named(NamedKey::Backspace) => state.db.backspace(),
            Key::Named(NamedKey::ArrowDown) => state.db.result_scroll += 1,
            Key::Named(NamedKey::ArrowUp) => {
                state.db.result_scroll = state.db.result_scroll.saturating_sub(1);
            }
            _ => {
                if let Some(t) = text {
                    for ch in t.chars() {
                        if !ch.is_control() {
                            state.db.insert(ch);
                        }
                    }
                }
            }
        }
        state.window.request_redraw();
    }

    fn handle_browser_key(&mut self, key: &Key) {
        use enzo_client::surface::BrowserPanel;
        let Some(state) = &mut self.state else { return };
        match key {
            Key::Character(s) if s.as_str() == "n" => {
                state.browser.panel = BrowserPanel::Network;
            }
            Key::Character(s) if s.as_str() == "c" => {
                state.browser.panel = BrowserPanel::Console;
            }
            Key::Character(s) if s.as_str() == "p" => {
                state.browser.panel = BrowserPanel::Page;
            }
            _ => {}
        }
        state.window.request_redraw();
    }
}

// ── Key → bytes ──────────────────────────────────────────────────────────────

fn key_to_bytes(key: &Key, text: Option<&str>) -> Vec<u8> {
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

// ── ATP background task ──────────────────────────────────────────────────────

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

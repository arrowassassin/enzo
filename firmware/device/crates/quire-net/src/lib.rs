//! Quire's network layer for the Xteink X3: the Wi-Fi manager (station and hotspot),
//! the embassy-net stack, mDNS, the hotspot's DHCP server and captive DNS, and the Drop
//! page HTTP server with its API and WebSocket (firmware-design/06-sideloading.md).
//!
//! The main loop talks to it through two channels — [`NetCommand`]s in, [`NetToMain`]
//! events out — and a shared state ([`NetShared`]) the UI reads through
//! [`quire_ui::net::NetState`]. Everything pure lives in [`proto`] and is unit-tested on
//! the host with `--no-default-features`; the `hal` feature adds the radio and the
//! executor tasks.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

pub mod fs;
pub mod proto;
mod shared;

#[cfg(feature = "hal")]
mod hal;

use alloc::string::String;
use alloc::vec::Vec;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use quire_ui::net::FetchRequest;
use quire_ui::Event;

pub use fs::{CardFs, DynFs};
#[cfg(feature = "hal")]
pub use hal::{net_task, NetTaskArgs};
pub use proto::api::StatusInfo;
pub use shared::*;

/// What the main loop asks the network layer to do (mirrors the `SysRequest` arms).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetCommand {
    /// Join the best saved network (or just bring the radio up when none is in range).
    WifiOn,
    /// Radio off; every network task ends and its memory is freed.
    WifiOff,
    /// Raise the hotspot.
    Hotspot,
    /// Join a network; an empty password means "use the saved one".
    Join {
        /// SSID.
        ssid: String,
        /// Password, or empty for a saved network.
        password: String,
    },
    /// Forget a saved network.
    Forget(String),
    /// Scan; the result arrives as `Event::WifiScan`.
    Scan,
    /// A client-side fetch (Bookshop, OPDS, weather…); see [`hal::fetch`].
    Fetch(FetchRequest),
    /// Start an OTA update from a URL or card path.
    Ota(String),
    /// Start or stop the Calibre wireless server.
    Calibre(bool),
    /// Sync now.
    SyncNow,
    /// The reading page is (not) the top screen: drives the 10-minute idle timeout.
    Reading(bool),
    /// A device text field is (not) focused: the page switches to its typing view.
    Typing(bool),
}

/// What the network layer tells the main loop.
#[derive(Clone, Debug, PartialEq)]
pub enum NetToMain {
    /// An event for `Ui::handle` (`Wifi`, `WifiScan`, `Net`, `PhoneText`, `PhoneKey`,
    /// `BooksChanged`).
    Ui(Event),
    /// The saved network list changed.
    SavedNetworks(Vec<String>),
    /// `/.quire/settings.bin` was rewritten by the Drop page; reload it.
    SettingsChanged,
    /// The Drop page asked for the current frame: write `/.quire/screen.pbm` and call
    /// [`screen_ready`].
    ScreenRequest,
}

/// Load `/.quire/wifi.bin`.
pub fn load_config(fs: &dyn CardFs) -> proto::wifi_bin::NetConfig {
    use quire_fs::Fs;
    DynFs(fs).read_to_vec(proto::wifi_bin::WIFI_FILE).map(|b| proto::wifi_bin::NetConfig::decode(&b)).unwrap_or_default()
}

/// Save `/.quire/wifi.bin` and refresh the shared PIN flag.
pub fn save_config(fs: &dyn CardFs, cfg: &proto::wifi_bin::NetConfig) {
    use quire_fs::Fs;
    let dfs = DynFs(fs);
    if !Fs::exists(&dfs, quire_ui::ROOT) {
        let _ = Fs::mkdir_all(&dfs, quire_ui::ROOT);
    }
    if let Err(e) = dfs.write_atomic(proto::wifi_bin::WIFI_FILE, &cfg.encode()) {
        log::warn!("wifi.bin: {e:?}");
    }
    with(|i| i.pin_set = !cfg.pin.is_empty());
}

/// Read the card once at boot: publishes the PIN flag and returns the saved network
/// names for the UI.
pub fn init_from_card(fs: &dyn CardFs) -> Vec<String> {
    let cfg = load_config(fs);
    with(|i| i.pin_set = !cfg.pin.is_empty());
    cfg.names()
}

/// Where the main loop writes the current frame for `GET /api/screen.pbm`.
pub const SCREEN_FILE: &str = "/.quire/screen.pbm";

/// The event that tells the UI the transfer list changed, as a main-loop message.
pub fn downloads_event_msg() -> NetToMain {
    NetToMain::Ui(shared::downloads_event())
}

/// Commands from the main loop.
static COMMANDS: Channel<CriticalSectionRawMutex, NetCommand, 8> = Channel::new();
/// Events to the main loop.
static EVENTS: Channel<CriticalSectionRawMutex, NetToMain, 16> = Channel::new();

/// Queue a command; false when the queue is full (the command is dropped).
pub fn send_command(cmd: NetCommand) -> bool {
    COMMANDS.try_send(cmd).is_ok()
}

/// Take one event, without waiting.
pub fn poll_event() -> Option<NetToMain> {
    EVENTS.try_receive().ok()
}

/// Wait for the next command (net task side).
pub async fn next_command() -> NetCommand {
    COMMANDS.receive().await
}

/// Take a command without waiting (net task side).
pub fn try_command() -> Option<NetCommand> {
    COMMANDS.try_receive().ok()
}

/// Post an event to the main loop, waiting for room.
pub async fn post(ev: NetToMain) {
    EVENTS.send(ev).await
}

/// Post an event without waiting; false when dropped because the queue is full.
pub fn try_post(ev: NetToMain) -> bool {
    EVENTS.try_send(ev).is_ok()
}

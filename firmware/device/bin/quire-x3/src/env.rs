//! The platform the UI runs on: card, clock, battery, requests, randomness.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use quire_fs::Fs;
use quire_net::NetShared;
use quire_ui::net::NetState;
use quire_ui::{Battery, DeviceInfo, Env, SysRequest, WifiState};

use quire_board::assets::FlashRegion;
use quire_board::sdfs::{SdFs, LOCAL_NOW};
use quire_ui::dict::builtin::DictSource;

/// Firmware version from the crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The device environment.
pub struct DeviceEnv {
    /// The mounted card.
    pub fs: SdFs,
    /// Local time at boot (seconds since 1970) minus the uptime at that moment.
    pub clock_base: u32,
    /// Latest battery reading.
    pub battery: Battery,
    /// Wi-Fi state, as last reported by the network task.
    pub wifi: WifiState,
    /// Saved network names.
    pub saved_networks: Vec<String>,
    /// Requests the UI made since the main loop last drained them.
    pub requests: Vec<SysRequest>,
    /// Panel controller name for About.
    pub panel: &'static str,
    /// Serial text (MAC).
    pub serial: String,
    /// Build stamp.
    pub build: &'static str,
    rng: esp_hal::rng::Rng,
    net: NetShared,
    /// The assets partition holding the dictionary, when it was found.
    pub assets: Option<FlashRegion>,
}

impl DeviceEnv {
    /// New, with the clock set so that `now()` returns `local_now`.
    pub fn new(fs: SdFs, local_now: u32, panel: &'static str, serial: String, build: &'static str) -> Self {
        let up = quire_board::power::UPTIME_MS.load(Ordering::Relaxed) / 1000;
        LOCAL_NOW.store(local_now, Ordering::Relaxed);
        DeviceEnv {
            fs,
            clock_base: local_now.wrapping_sub(up),
            battery: Battery::default(),
            wifi: WifiState::Off,
            saved_networks: Vec::new(),
            requests: Vec::new(),
            panel,
            serial,
            build,
            rng: esp_hal::rng::Rng::new(),
            net: NetShared::new(),
            assets: None,
        }
    }

    /// Re-base the clock (after the user sets the time or the RTC is read again).
    pub fn set_clock(&mut self, local_now: u32) {
        let up = quire_board::power::UPTIME_MS.load(Ordering::Relaxed) / 1000;
        self.clock_base = local_now.wrapping_sub(up);
        LOCAL_NOW.store(local_now, Ordering::Relaxed);
    }

    /// Advance the shared clock; the main loop calls this every second.
    pub fn tick_clock(&self) {
        LOCAL_NOW.store(self.now(), Ordering::Relaxed);
    }

    /// Take the pending requests.
    pub fn take_requests(&mut self) -> Vec<SysRequest> {
        core::mem::take(&mut self.requests)
    }
}

impl Env for DeviceEnv {
    type Fs = SdFs;
    fn fs(&self) -> &SdFs {
        &self.fs
    }
    fn now(&self) -> u32 {
        self.clock_base.wrapping_add(quire_board::power::UPTIME_MS.load(Ordering::Relaxed) / 1000)
    }
    fn millis(&self) -> u32 {
        quire_board::power::UPTIME_MS.load(Ordering::Relaxed)
    }
    fn battery(&self) -> Battery {
        self.battery
    }
    fn wifi(&self) -> WifiState {
        self.wifi.clone()
    }
    fn saved_networks(&self) -> Vec<String> {
        self.saved_networks.clone()
    }
    fn device(&self) -> DeviceInfo {
        let stats = esp_alloc::HEAP.stats();
        DeviceInfo {
            version: String::from(VERSION),
            build: String::from(self.build),
            panel: String::from(self.panel),
            free_heap: (stats.size - stats.current_usage) as u32,
            largest_block: esp_alloc::HEAP.free() as u32,
            flash_bytes: 16 * 1024 * 1024,
            card_total: Some(self.fs.total_bytes()),
            card_free: self.fs.free_bytes(),
            serial: self.serial.clone(),
        }
    }
    fn request(&mut self, req: SysRequest) {
        self.requests.push(req);
    }
    fn random(&mut self) -> u32 {
        self.rng.random()
    }
    fn dictionary(&self) -> Option<&dyn DictSource> {
        self.assets.as_ref().map(|a| a as &dyn DictSource)
    }
    fn net(&mut self) -> &mut dyn NetState {
        self.net.refresh();
        &mut self.net
    }
}

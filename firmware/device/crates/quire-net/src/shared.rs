//! State shared between the network tasks and the main loop, behind a critical-section
//! mutex: the upload list the Drop screen shows, the Wi-Fi state, the status the API
//! reports, and the half-size mirror frame for the Window tab.

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use critical_section::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::PubSubChannel;
use embassy_sync::signal::Signal;
use quire_ui::net::{Download, DownloadState, NetEvent, NetState, WeatherReport};
use quire_ui::{Event, WifiState};

use crate::proto::api::StatusInfo;
use crate::proto::mirror;

/// Most upload rows kept for the Drop screen.
const MAX_DOWNLOADS: usize = 12;

/// The shared state.
pub struct Inner {
    /// Uploads and downloads, oldest first.
    pub downloads: Vec<Download>,
    /// Bumped on every change to `downloads`.
    pub downloads_gen: u32,
    /// Wi-Fi state as last published.
    pub wifi: WifiState,
    /// Device status for `/api/status`.
    pub status: StatusInfo,
    /// Card size in bytes (0 when unknown).
    pub card_total: u64,
    /// Whether a Drop page PIN is set.
    pub pin_set: bool,
    /// The packed half-size frame, present while a Window tab is subscribed.
    pub mirror: Option<Vec<u8>>,
    /// Bumped when `mirror` changes.
    pub mirror_gen: u32,
    /// WebSocket clients that want mirror frames.
    pub mirror_clients: u8,
    /// Uptime (ms) of the last HTTP or WebSocket activity.
    pub activity_ms: u32,
    /// Whether the radio should skip power saving right now (a transfer is running).
    pub busy_until_ms: u32,
    /// Calibre status line.
    pub calibre_status: String,
    /// Last weather report (filled by the fetchers).
    pub weather: Option<WeatherReport>,
    /// Sleep image packs (filled by the fetchers).
    pub sleep_packs: Vec<(String, String, u16, u64, bool)>,
    /// An OTA is running.
    pub ota_busy: bool,
}

impl Inner {
    const fn new() -> Self {
        Inner {
            downloads: Vec::new(),
            downloads_gen: 0,
            wifi: WifiState::Off,
            status: StatusInfo {
                battery_percent: 0,
                charging: false,
                version: String::new(),
                build: String::new(),
                book_title: None,
                book_percent: 0,
                hostname: String::new(),
                heap_free: 0,
                heap_largest: 0,
                uptime: 0,
                local_now: 0,
                reading: false,
            },
            card_total: 0,
            pin_set: false,
            mirror: None,
            mirror_gen: 0,
            mirror_clients: 0,
            activity_ms: 0,
            busy_until_ms: 0,
            calibre_status: String::new(),
            weather: None,
            sleep_packs: Vec::new(),
            ota_busy: false,
        }
    }
}

static NET: Mutex<RefCell<Inner>> = Mutex::new(RefCell::new(Inner::new()));

/// Run `f` with the shared state locked. Keep it short: it runs with interrupts off.
pub fn with<R>(f: impl FnOnce(&mut Inner) -> R) -> R {
    critical_section::with(|cs| f(&mut NET.borrow_ref_mut(cs)))
}

/// Text frames queued for every WebSocket client (progress echoes, "N books added",
/// "the device is listening"). Slow clients drop the oldest.
pub static WS_OUT: PubSubChannel<CriticalSectionRawMutex, String, 6, 2, 1> = PubSubChannel::new();

/// Broadcast a text frame to the WebSocket clients.
pub fn ws_broadcast(text: String) {
    WS_OUT.immediate_publisher().publish_immediate(text);
}

/// Raised by the main loop once `/.quire/screen.pbm` holds the current frame.
pub static SCREEN_READY: Signal<CriticalSectionRawMutex, bool> = Signal::new();

/// Tell the server the screen file is ready (or that it could not be written).
pub fn screen_ready(ok: bool) {
    SCREEN_READY.signal(ok);
}

/// Publish the status the API reports; the main loop calls this once a second.
pub fn publish_status(status: StatusInfo, card_total: u64) {
    with(|i| {
        i.status = status;
        i.card_total = card_total;
    });
}

/// The Wi-Fi state as last published.
pub fn wifi_state() -> WifiState {
    with(|i| i.wifi.clone())
}

/// Whether any Window tab wants mirror frames.
pub fn mirror_wanted() -> bool {
    with(|i| i.mirror_clients > 0)
}

/// Pack the frame at half size for the Window tab. The main loop calls this after a
/// refresh, at most twice a second, while [`mirror_wanted`] is true.
pub fn publish_mirror(bits: &[u8], w: u32, h: u32) {
    let mut packed = alloc::vec![0u8; mirror::MIRROR_BYTES];
    if mirror::pack_half(bits, w as usize, h as usize, &mut packed) == 0 {
        return;
    }
    with(|i| {
        i.mirror = Some(packed);
        i.mirror_gen = i.mirror_gen.wrapping_add(1);
    });
}

/// Note network activity (resets the idle timeout, disables power saving briefly).
pub fn touch(now_ms: u32) {
    with(|i| {
        i.activity_ms = now_ms;
        i.busy_until_ms = now_ms.wrapping_add(20_000);
    });
}

fn changed(i: &mut Inner) {
    i.downloads_gen = i.downloads_gen.wrapping_add(1);
}

/// An upload (or download) began.
pub fn transfer_started(title: &str, url: &str, total: Option<u64>, done: u64) {
    with(|i| {
        i.downloads.retain(|d| d.title != title);
        if i.downloads.len() >= MAX_DOWNLOADS {
            i.downloads.remove(0);
        }
        i.downloads.push(Download {
            title: String::from(title),
            author: String::new(),
            url: String::from(url),
            done,
            total,
            state: DownloadState::Working,
            book: None,
        });
        changed(i);
    });
}

/// Progress on a transfer.
pub fn transfer_progress(title: &str, done: u64) {
    with(|i| {
        if let Some(d) = i.downloads.iter_mut().find(|d| d.title == title) {
            d.done = done;
            changed(i);
        }
    });
}

/// A transfer ended.
pub fn transfer_finished(title: &str, result: Result<(), String>) {
    with(|i| {
        if let Some(d) = i.downloads.iter_mut().find(|d| d.title == title) {
            match result {
                Ok(()) => {
                    d.state = DownloadState::Done;
                    if let Some(t) = d.total {
                        d.done = t;
                    }
                }
                Err(e) => d.state = DownloadState::Failed(e),
            }
            changed(i);
        }
    });
}

/// The `Event` that tells the UI the transfer list changed.
pub fn downloads_event() -> Event {
    Event::Net(NetEvent::Downloads)
}

/// The UI's handle on the shared state: a cached copy of the transfer list, refreshed
/// (cheaply, by generation) every time the UI asks for it.
pub struct NetShared {
    downloads: Vec<Download>,
    seen_gen: u32,
}

impl Default for NetShared {
    fn default() -> Self {
        Self::new()
    }
}

impl NetShared {
    /// A fresh handle.
    pub fn new() -> Self {
        NetShared { downloads: Vec::new(), seen_gen: u32::MAX }
    }
    /// Copy the transfer list if it changed since the last look.
    pub fn refresh(&mut self) {
        let fresh = with(|i| if i.downloads_gen != self.seen_gen { Some((i.downloads.clone(), i.downloads_gen)) } else { None });
        if let Some((d, g)) = fresh {
            self.downloads = d;
            self.seen_gen = g;
        }
    }
}

impl NetState for NetShared {
    fn downloads(&self) -> &[Download] {
        &self.downloads
    }
    fn weather(&self) -> Option<WeatherReport> {
        with(|i| i.weather.clone())
    }
    fn calibre_status(&self) -> String {
        with(|i| {
            if i.calibre_status.is_empty() {
                match i.wifi {
                    WifiState::Off => String::from("Wi-Fi off"),
                    _ => String::from("Calibre server off"),
                }
            } else {
                i.calibre_status.clone()
            }
        })
    }
    fn ota_busy(&self) -> bool {
        with(|i| i.ota_busy)
    }
    fn sleep_packs(&self) -> Vec<(String, String, u16, u64, bool)> {
        with(|i| i.sleep_packs.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfers_and_handle() {
        let mut h = NetShared::new();
        h.refresh();
        let base = h.downloads().len();
        transfer_started("x.epub", "/Books/x.epub", Some(10), 0);
        transfer_progress("x.epub", 4);
        h.refresh();
        let d = h.downloads().iter().find(|d| d.title == "x.epub").unwrap();
        assert_eq!(d.done, 4);
        assert_eq!(d.state, DownloadState::Working);
        transfer_finished("x.epub", Ok(()));
        h.refresh();
        let d = h.downloads().iter().find(|d| d.title == "x.epub").unwrap();
        assert_eq!((d.done, d.state.clone()), (10, DownloadState::Done));
        assert_eq!(h.downloads().len(), base + 1);
        for i in 0..20 {
            transfer_started(&alloc::format!("f{i}"), "", None, 0);
        }
        h.refresh();
        assert!(h.downloads().len() <= MAX_DOWNLOADS);
    }

    #[test]
    fn mirror() {
        assert!(!mirror_wanted());
        let bits = alloc::vec![0u8; 66 * 792];
        publish_mirror(&bits, 528, 792);
        assert!(with(|i| i.mirror.as_ref().map(|m| m.len())) == Some(mirror::MIRROR_BYTES));
    }
}

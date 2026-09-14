//! The radio-side of the crate: the net task, its Wi-Fi session, and the servers that
//! run while the radio is up. Everything here needs the `hal` feature.

pub mod captive;
pub mod dhcp;
pub mod fetch;
pub mod http;
pub mod mdns;
pub mod page;
pub mod task;
pub mod wifi;
pub mod ws;

pub use task::{net_task, NetTaskArgs};

/// Milliseconds since boot.
pub fn now_ms() -> u32 {
    embassy_time::Instant::now().as_millis() as u32
}

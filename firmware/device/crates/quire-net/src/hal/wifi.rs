//! Wi-Fi controller helpers: configuration for both modes, scanning, joining, and the
//! bits-of-signal reading the UI shows.

use alloc::string::String;
use alloc::vec::Vec;

use embassy_net::Stack;
use embassy_time::{with_timeout, Duration};
use esp_radio::wifi::ap::AccessPointConfig;
use esp_radio::wifi::scan::ScanConfig;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{AuthenticationMethod, Config, ControllerConfig, PowerSaveMode, WifiController};
use quire_ui::WifiState;

use crate::proto::hotspot;
use crate::proto::wifi_bin::{bars, NetConfig, Seen};

/// How long a join may take (association plus DHCP).
const JOIN_TIMEOUT: Duration = Duration::from_secs(15);
/// How long DHCP may take after association.
const DHCP_TIMEOUT: Duration = Duration::from_secs(12);

/// The controller configuration: fewer dynamic buffers than the default so the driver's
/// heap stays near 40 KB (each static RX buffer is 1.6 KB, allocated at start).
pub fn controller_config(initial: Config) -> ControllerConfig {
    ControllerConfig::default()
        .with_initial_config(initial)
        .with_static_rx_buf_num(8)
        .with_dynamic_rx_buf_num(12)
        .with_dynamic_tx_buf_num(12)
        .with_rx_ba_win(6)
}

/// A station configuration with nothing to join yet.
pub fn idle_station() -> Config {
    Config::Station(StationConfig::default())
}

/// The hotspot configuration.
pub fn hotspot_config(ssid: &str, password: &str) -> Config {
    Config::AccessPoint(
        AccessPointConfig::default()
            .with_ssid(ssid)
            .with_password(String::from(password))
            .with_auth_method(AuthenticationMethod::Wpa2Personal)
            .with_channel(6)
            .with_max_connections(4),
    )
}

/// Scan for networks (up to 20, hidden ones excluded).
pub async fn scan(controller: &mut WifiController<'_>) -> Vec<Seen> {
    let cfg = ScanConfig::default().with_max(20);
    match with_timeout(Duration::from_secs(8), controller.scan_async(&cfg)).await {
        Ok(Ok(list)) => list
            .into_iter()
            .map(|ap| Seen {
                ssid: String::from(ap.ssid.as_str()),
                rssi: ap.signal_strength,
                secured: !matches!(ap.auth_method, None | Some(AuthenticationMethod::None)),
            })
            .collect(),
        Ok(Err(e)) => {
            log::warn!("scan: {e:?}");
            Vec::new()
        }
        Err(_) => {
            log::warn!("scan: timeout");
            Vec::new()
        }
    }
}

/// Signal bars for the current association.
pub fn signal(controller: &WifiController<'_>) -> u8 {
    controller.rssi().map(bars).unwrap_or(0)
}

/// Join `ssid` and wait for an address. Returns the connected state, or the failure text.
pub async fn join(
    controller: &mut WifiController<'_>,
    stack: Stack<'_>,
    ssid: &str,
    password: &str,
    host: &str,
) -> Result<WifiState, String> {
    let mut cfg = StationConfig::default().with_ssid(ssid).with_password(String::from(password));
    if password.is_empty() {
        cfg = cfg.with_auth_method(AuthenticationMethod::None);
    }
    controller.set_config(&Config::Station(cfg)).map_err(|e| alloc::format!("{e:?}"))?;
    match with_timeout(JOIN_TIMEOUT, controller.connect_async()).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return Err(alloc::format!("{e:?}")),
        Err(_) => return Err(String::from("timeout")),
    }
    if with_timeout(DHCP_TIMEOUT, stack.wait_config_up()).await.is_err() {
        let _ = controller.disconnect_async().await;
        return Err(String::from("no address (DHCP)"));
    }
    let ip = stack.config_v4().map(|c| alloc::format!("{}", c.address.address())).unwrap_or_default();
    Ok(WifiState::Connected { ssid: String::from(ssid), ip, host: String::from(host), signal: signal(controller) })
}

/// Pick what to join: an explicit target (a saved network when the password is empty),
/// else the strongest saved network in a scan.
pub async fn choose_target(
    controller: &mut WifiController<'_>,
    saved: &NetConfig,
    target: Option<(String, String)>,
) -> Option<(String, String)> {
    match target {
        Some((ssid, pw)) if !pw.is_empty() => Some((ssid, pw)),
        Some((ssid, _)) => saved.get(&ssid).map(|n| (n.ssid.clone(), n.password.clone())),
        None => {
            if saved.networks.is_empty() {
                return None;
            }
            let seen = scan(controller).await;
            crate::proto::wifi_bin::choose(saved, &seen).map(|n| (n.ssid.clone(), n.password.clone()))
        }
    }
}

/// Apply power saving; errors are only logged.
pub fn power_save(controller: &mut WifiController<'_>, on: bool) {
    let mode = if on { PowerSaveMode::Minimum } else { PowerSaveMode::None };
    if let Err(e) = controller.set_power_saving(mode) {
        log::debug!("power save: {e:?}");
    }
}

/// The hotspot's address as text.
pub fn ap_ip_text() -> String {
    let [a, b, c, d] = hotspot::AP_IP;
    alloc::format!("{a}.{b}.{c}.{d}")
}

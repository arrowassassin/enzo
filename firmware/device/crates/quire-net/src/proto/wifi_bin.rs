//! `/.quire/wifi.bin`: saved networks (up to 8, most recently used first), the Drop page
//! PIN and the hotspot password, as postcard.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Where the file lives.
pub const WIFI_FILE: &str = "/.quire/wifi.bin";
/// Most networks kept.
pub const MAX_NETWORKS: usize = 8;

/// A saved network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedNetwork {
    /// Name.
    pub ssid: String,
    /// Passphrase (empty for an open network).
    pub password: String,
    /// Whether the last join worked.
    pub last_ok: bool,
}

/// The file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetConfig {
    /// Schema version.
    pub version: u16,
    /// Networks, most recently used first.
    pub networks: Vec<SavedNetwork>,
    /// Drop page PIN (empty: none).
    pub pin: String,
    /// Hotspot WPA2 password (generated once, so phones can remember it).
    pub hotspot_password: String,
}

impl Default for NetConfig {
    fn default() -> Self {
        NetConfig { version: 1, networks: Vec::new(), pin: String::new(), hotspot_password: String::new() }
    }
}

impl NetConfig {
    /// Decode, or defaults when the bytes are missing or from another version.
    pub fn decode(bytes: &[u8]) -> NetConfig {
        match postcard::from_bytes::<NetConfig>(bytes) {
            Ok(c) if c.version == 1 => c,
            _ => NetConfig::default(),
        }
    }
    /// Encode.
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }
    /// Names in order.
    pub fn names(&self) -> Vec<String> {
        self.networks.iter().map(|n| n.ssid.clone()).collect()
    }
    /// A saved network by name.
    pub fn get(&self, ssid: &str) -> Option<&SavedNetwork> {
        self.networks.iter().find(|n| n.ssid == ssid)
    }
    /// Add or update a network and move it to the front; the oldest drops past the cap.
    pub fn remember(&mut self, ssid: &str, password: &str, ok: bool) {
        self.networks.retain(|n| n.ssid != ssid);
        self.networks.insert(0, SavedNetwork { ssid: String::from(ssid), password: String::from(password), last_ok: ok });
        self.networks.truncate(MAX_NETWORKS);
    }
    /// Forget a network; true when it was there.
    pub fn forget(&mut self, ssid: &str) -> bool {
        let n = self.networks.len();
        self.networks.retain(|x| x.ssid != ssid);
        self.networks.len() != n
    }
    /// Mark the outcome of a join.
    pub fn mark(&mut self, ssid: &str, ok: bool) {
        if let Some(n) = self.networks.iter_mut().find(|n| n.ssid == ssid) {
            n.last_ok = ok;
        }
    }
}

/// A network seen in a scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// Name.
    pub ssid: String,
    /// RSSI in dBm.
    pub rssi: i8,
    /// Whether it needs a password.
    pub secured: bool,
}

/// The saved network to auto-join: the strongest visible one; on a tie, the most
/// recently used.
pub fn choose<'a>(saved: &'a NetConfig, seen: &[Seen]) -> Option<&'a SavedNetwork> {
    let mut best: Option<(&SavedNetwork, i8, usize)> = None;
    for (i, n) in saved.networks.iter().enumerate() {
        let Some(s) = seen.iter().filter(|s| s.ssid == n.ssid).max_by_key(|s| s.rssi) else { continue };
        match best {
            Some((_, r, j)) if r > s.rssi || (r == s.rssi && j < i) => {}
            _ => best = Some((n, s.rssi, i)),
        }
    }
    best.map(|b| b.0)
}

/// Signal bars 0–4 from dBm.
pub fn bars(rssi: i32) -> u8 {
    match rssi {
        r if r >= -55 => 4,
        r if r >= -65 => 3,
        r if r >= -75 => 2,
        r if r >= -85 => 1,
        _ => 0,
    }
}

/// Merge a scan into the UI's list: strongest first, deduplicated by name, saved flagged.
pub fn scan_list(saved: &NetConfig, seen: &[Seen]) -> Vec<quire_ui::WifiNetwork> {
    let mut sorted: Vec<&Seen> = seen.iter().filter(|s| !s.ssid.is_empty()).collect();
    sorted.sort_by_key(|a| core::cmp::Reverse(a.rssi));
    let mut out: Vec<quire_ui::WifiNetwork> = Vec::new();
    for s in sorted {
        if out.iter().any(|o| o.ssid == s.ssid) {
            continue;
        }
        out.push(quire_ui::WifiNetwork {
            ssid: s.ssid.clone(),
            signal: bars(s.rssi as i32),
            secured: s.secured,
            saved: saved.get(&s.ssid).is_some(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_cap() {
        let mut c = NetConfig::default();
        for i in 0..10 {
            c.remember(&alloc::format!("net{i}"), "pw", true);
        }
        assert_eq!(c.networks.len(), MAX_NETWORKS);
        assert_eq!(c.networks[0].ssid, "net9");
        assert!(c.get("net0").is_none());
        c.remember("net5", "new", false);
        assert_eq!(c.networks[0].ssid, "net5");
        assert_eq!(c.networks[0].password, "new");
        assert_eq!(c.networks.iter().filter(|n| n.ssid == "net5").count(), 1);
        c.pin = "1234".into();
        let bytes = c.encode();
        assert_eq!(NetConfig::decode(&bytes), c);
        assert_eq!(NetConfig::decode(b"garbage"), NetConfig::default());
        assert!(c.forget("net5"));
        assert!(!c.forget("net5"));
    }

    #[test]
    fn chooses_strongest_saved() {
        let mut c = NetConfig::default();
        c.remember("home", "a", true);
        c.remember("work", "b", true);
        let seen = [
            Seen { ssid: "cafe".into(), rssi: -30, secured: true },
            Seen { ssid: "home".into(), rssi: -70, secured: true },
            Seen { ssid: "work".into(), rssi: -60, secured: true },
            Seen { ssid: "home".into(), rssi: -50, secured: true },
        ];
        assert_eq!(choose(&c, &seen).map(|n| n.ssid.as_str()), Some("home"));
        assert_eq!(choose(&c, &seen[..1]), None);
        let list = scan_list(&c, &seen);
        assert_eq!(list.iter().map(|n| n.ssid.as_str()).collect::<Vec<_>>(), ["cafe", "home", "work"]);
        assert!(list[1].saved && !list[0].saved);
        assert_eq!(bars(-40), 4);
        assert_eq!(bars(-90), 0);
    }
}

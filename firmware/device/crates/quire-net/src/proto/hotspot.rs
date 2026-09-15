//! Hotspot identity: the SoftAP name from the MAC and a generated WPA2 password.

use alloc::string::String;
use core::fmt::Write;

/// `Quire-XXXX` from the last two MAC bytes.
pub fn ssid_from_mac(mac: &[u8; 6]) -> String {
    let mut s = String::with_capacity(10);
    let _ = write!(s, "Quire-{:02X}{:02X}", mac[4], mac[5]);
    s
}

/// Characters that read unambiguously on an e-ink screen (no 0/O, 1/l/I).
const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";

/// An 8-character password from 64 random bits.
pub fn password(mut seed: u64) -> String {
    let mut s = String::with_capacity(8);
    for _ in 0..8 {
        // A small LCG step so one `u32` of entropy still spreads over all positions.
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let i = ((seed >> 33) % ALPHABET.len() as u64) as usize;
        s.push(ALPHABET[i] as char);
    }
    s
}

/// The hotspot's IPv4 address and prefix.
pub const AP_IP: [u8; 4] = [192, 168, 4, 1];
/// Prefix length of the hotspot network.
pub const AP_PREFIX: u8 = 24;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(ssid_from_mac(&[0x10, 0x20, 0x30, 0x40, 0xAB, 0x0C]), "Quire-AB0C");
        let p = password(12345);
        assert_eq!(p.len(), 8);
        assert!(p.bytes().all(|b| ALPHABET.contains(&b)));
        assert_ne!(password(1), password(2));
    }
}

//! SNTP reply parsing (RFC 4330), pure and host-tested.

/// Seconds between 1900 (NTP) and 1970 (Unix).
const NTP_UNIX_DELTA: u32 = 2_208_988_800;

/// The transmit timestamp of a server reply as Unix seconds; `None` for a bad packet.
pub fn parse(resp: &[u8]) -> Option<u32> {
    if resp.len() < 48 {
        return None;
    }
    let mode = resp[0] & 7;
    let leap = resp[0] >> 6;
    let stratum = resp[1];
    if mode != 4 || leap == 3 || stratum == 0 || stratum > 15 {
        return None;
    }
    let secs = u32::from_be_bytes([resp[40], resp[41], resp[42], resp[43]]);
    // Era 0 only (until 2036), and nothing before the reader could exist.
    let unix = secs.checked_sub(NTP_UNIX_DELTA)?;
    (unix > 1_700_000_000).then_some(unix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_reply_and_rejects_bad_ones() {
        let mut r = [0u8; 48];
        r[0] = 0x24; // LI 0, v4, server
        r[1] = 2;
        let ntp = 1_789_000_000u32 + NTP_UNIX_DELTA;
        r[40..44].copy_from_slice(&ntp.to_be_bytes());
        assert_eq!(parse(&r), Some(1_789_000_000));
        let mut kod = r;
        kod[1] = 0; // kiss-o'-death
        assert_eq!(parse(&kod), None);
        let mut alarm = r;
        alarm[0] = 0xE4; // LI 3: clock unsynchronised
        assert_eq!(parse(&alarm), None);
        let mut client = r;
        client[0] = 0x23;
        assert_eq!(parse(&client), None);
        assert_eq!(parse(&r[..47]), None);
    }
}

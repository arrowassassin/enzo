//! Small pure helpers shared by the drivers.

/// BCD byte to binary (DS3231 registers).
pub const fn bcd_to_bin(b: u8) -> u8 {
    (b >> 4) * 10 + (b & 0x0F)
}

/// Binary to BCD (0–99).
pub const fn bin_to_bcd(v: u8) -> u8 {
    ((v / 10) << 4) | (v % 10)
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian), matching
/// `quire_library::time::from_civil`.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era as i64 * 146_097 + doe - 719_468
}

/// Civil date for days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

/// State of charge (0–100) from the BQ27220 SOC register, clamped.
pub fn soc_percent(raw: u16) -> u8 {
    raw.min(100) as u8
}

/// Battery health from a full-charge capacity and the design capacity (642 mAh).
pub fn health_percent(full_charge_mah: u16) -> u8 {
    ((full_charge_mah as u32 * 100) / 642).min(100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcd_round_trip() {
        for v in 0..100u8 {
            assert_eq!(bcd_to_bin(bin_to_bcd(v)), v);
        }
    }

    #[test]
    fn civil_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 9, 14), 20_710);
        assert_eq!(civil_from_days(20_710), (2026, 9, 14));
        for d in [0i64, 59, 60, 365, 10_000, 20_000, 30_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
    }

    #[test]
    fn gauge_maths() {
        assert_eq!(soc_percent(150), 100);
        assert_eq!(health_percent(321), 50);
    }
}

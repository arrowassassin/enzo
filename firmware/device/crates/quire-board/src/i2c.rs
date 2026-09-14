//! The I²C peripherals on GPIO0/GPIO20 at 400 kHz: BQ27220 fuel gauge (0x55), DS3231
//! clock (0x68) and QMI8658 IMU (0x6B/0x6A). Every read tolerates a missing device.

use esp_hal::i2c::master::I2c;
use esp_hal::Blocking;
use quire_ui::Battery;

use crate::pins::{ADDR_GAUGE, ADDR_IMU, ADDR_RTC};
use crate::util::{bcd_to_bin, bin_to_bcd, civil_from_days, days_from_civil, health_percent, soc_percent};

/// The bus.
pub type Bus = I2c<'static, Blocking>;

fn read_u16(i2c: &mut Bus, addr: u8, reg: u8) -> Option<u16> {
    let mut b = [0u8; 2];
    i2c.write_read(addr, &[reg], &mut b).ok()?;
    Some(u16::from_le_bytes(b))
}

/// BQ27220 standard commands (TI SLUSDS3, §4.1).
pub mod gauge {
    /// Voltage, mV.
    pub const VOLTAGE: u8 = 0x08;
    /// Current, mA signed (positive = charging = USB present on the X3).
    pub const CURRENT: u8 = 0x0C;
    /// Full-charge capacity, mAh.
    pub const FULL_CHARGE: u8 = 0x12;
    /// Time to empty, minutes (0xFFFF when not discharging).
    pub const TIME_TO_EMPTY: u8 = 0x16;
    /// Cycle count.
    pub const CYCLES: u8 = 0x2A;
    /// State of charge, percent.
    pub const SOC: u8 = 0x2C;
    /// State of health, percent.
    pub const SOH: u8 = 0x2E;
}

/// Read the battery state; `None` when the gauge does not answer.
pub fn read_battery(i2c: &mut Bus) -> Option<Battery> {
    let soc = read_u16(i2c, ADDR_GAUGE, gauge::SOC)?;
    let mv = read_u16(i2c, ADDR_GAUGE, gauge::VOLTAGE).unwrap_or(0);
    let current = read_u16(i2c, ADDR_GAUGE, gauge::CURRENT).map(|c| c as i16).unwrap_or(0);
    let tte = read_u16(i2c, ADDR_GAUGE, gauge::TIME_TO_EMPTY).unwrap_or(0xFFFF);
    let cycles = read_u16(i2c, ADDR_GAUGE, gauge::CYCLES);
    let soh = read_u16(i2c, ADDR_GAUGE, gauge::SOH);
    let full = read_u16(i2c, ADDR_GAUGE, gauge::FULL_CHARGE);
    let health = soh.map(|h| h.min(100) as u8).or_else(|| full.map(health_percent));
    Some(Battery {
        percent: soc_percent(soc),
        charging: current > 0,
        days_left: (tte != 0xFFFF && tte != 0).then(|| (tte as u32 / 1440).min(u16::MAX as u32) as u16),
        cycles,
        health,
        millivolts: mv,
    })
}

/// DS3231 registers.
pub mod rtc {
    /// Seconds (BCD).
    pub const SECONDS: u8 = 0x00;
    /// Status: bit 7 OSF (oscillator stopped since last set → time invalid).
    pub const STATUS: u8 = 0x0F;
    /// Temperature MSB (signed °C).
    pub const TEMP_MSB: u8 = 0x11;
}

/// Read the clock as local seconds since 1970; `None` when absent or never set.
pub fn read_clock(i2c: &mut Bus) -> Option<u32> {
    let mut st = [0u8; 1];
    i2c.write_read(ADDR_RTC, &[rtc::STATUS], &mut st).ok()?;
    if st[0] & 0x80 != 0 {
        return None; // oscillator stop flag: the time was lost
    }
    let mut r = [0u8; 7];
    i2c.write_read(ADDR_RTC, &[rtc::SECONDS], &mut r).ok()?;
    let sec = bcd_to_bin(r[0] & 0x7F) as u32;
    let min = bcd_to_bin(r[1] & 0x7F) as u32;
    let hour = if r[2] & 0x40 != 0 {
        // 12-hour mode: bit 5 = PM.
        let h = bcd_to_bin(r[2] & 0x1F) as u32 % 12;
        if r[2] & 0x20 != 0 {
            h + 12
        } else {
            h
        }
    } else {
        bcd_to_bin(r[2] & 0x3F) as u32
    };
    let day = bcd_to_bin(r[4] & 0x3F) as u32;
    let month = bcd_to_bin(r[5] & 0x1F) as u32;
    let year = 2000 + bcd_to_bin(r[6]) as i32 + if r[5] & 0x80 != 0 { 100 } else { 0 };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if !(0..=200_000).contains(&days) {
        return None;
    }
    Some((days as u32) * 86_400 + hour * 3600 + min * 60 + sec)
}

/// Set the clock from local seconds since 1970 and clear the stop flag.
pub fn set_clock(i2c: &mut Bus, t: u32) -> bool {
    let days = (t / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    let secs = t % 86_400;
    // Day of week 1–7 with Monday = 1 (1970-01-01 was a Thursday = 4).
    let dow = ((days + 3) % 7 + 1) as u8;
    let century = y >= 2100;
    let regs = [
        rtc::SECONDS,
        bin_to_bcd((secs % 60) as u8),
        bin_to_bcd(((secs / 60) % 60) as u8),
        bin_to_bcd((secs / 3600) as u8),
        dow,
        bin_to_bcd(d as u8),
        bin_to_bcd(m as u8) | if century { 0x80 } else { 0 },
        bin_to_bcd((y % 100) as u8),
    ];
    if i2c.write(ADDR_RTC, &regs).is_err() {
        return false;
    }
    // Clear OSF, keep the rest of the status register.
    let mut st = [0u8; 1];
    if i2c.write_read(ADDR_RTC, &[rtc::STATUS], &mut st).is_ok() {
        let _ = i2c.write(ADDR_RTC, &[rtc::STATUS, st[0] & !0x80]);
    }
    true
}

/// Board temperature from the clock chip, tenths of °C.
pub fn read_temperature_deci(i2c: &mut Bus) -> Option<i16> {
    let mut b = [0u8; 2];
    i2c.write_read(ADDR_RTC, &[rtc::TEMP_MSB], &mut b).ok()?;
    let whole = b[0] as i8 as i16;
    let quarter = (b[1] >> 6) as i16;
    Some(whole * 10 + quarter * 25 / 10)
}

/// QMI8658 registers (QST datasheet rev 1.1).
pub mod imu {
    /// WHO_AM_I, reads 0x05.
    pub const WHO_AM_I: u8 = 0x00;
    /// CTRL1: 0x40 = address auto-increment, SPI/I²C options.
    pub const CTRL1: u8 = 0x02;
    /// CTRL2: accelerometer full scale and ODR.
    pub const CTRL2: u8 = 0x03;
    /// CTRL7: sensor enables (bit 0 accel, bit 1 gyro).
    pub const CTRL7: u8 = 0x08;
    /// Accelerometer X low byte; six bytes X/Y/Z little endian.
    pub const AX_L: u8 = 0x35;
    /// Expected identity.
    pub const ID: u8 = 0x05;
}

/// The accelerometer, if present.
pub struct Imu {
    addr: u8,
}

impl Imu {
    /// Probe both addresses and configure the accelerometer at ±4 g, 31.25 Hz.
    pub fn init(i2c: &mut Bus) -> Option<Imu> {
        for addr in ADDR_IMU {
            let mut id = [0u8; 1];
            if i2c.write_read(addr, &[imu::WHO_AM_I], &mut id).is_ok() && id[0] == imu::ID {
                // aFS = ±4 g (0b001 << 4), aODR = 31.25 Hz low power (0b0111).
                let _ = i2c.write(addr, &[imu::CTRL1, 0x40]);
                let _ = i2c.write(addr, &[imu::CTRL2, 0x17]);
                let _ = i2c.write(addr, &[imu::CTRL7, 0x01]);
                return Some(Imu { addr });
            }
        }
        None
    }

    /// Raw acceleration, 8192 LSB per g at ±4 g.
    pub fn accel(&self, i2c: &mut Bus) -> Option<[i16; 3]> {
        let mut b = [0u8; 6];
        i2c.write_read(self.addr, &[imu::AX_L], &mut b).ok()?;
        Some([
            i16::from_le_bytes([b[0], b[1]]),
            i16::from_le_bytes([b[2], b[3]]),
            i16::from_le_bytes([b[4], b[5]]),
        ])
    }
}

/// Gravity direction from an accelerometer sample: which panel edge points up.
/// Returns 0 = portrait (buttons at the bottom), 1 = rotated 90° clockwise, 2 = upside
/// down, 3 = 90° counter-clockwise, or `None` when the device is lying flat.
pub fn orientation(a: [i16; 3]) -> Option<u8> {
    let (x, y, z) = (a[0] as i32, a[1] as i32, a[2] as i32);
    if z.abs() > x.abs().max(y.abs()) * 2 {
        return None;
    }
    Some(if y.abs() >= x.abs() {
        if y > 0 {
            0
        } else {
            2
        }
    } else if x > 0 {
        1
    } else {
        3
    })
}

/// Shake: the magnitude departs from 1 g by more than half a g.
pub fn is_shake(a: [i16; 3]) -> bool {
    let (x, y, z) = (a[0] as i64, a[1] as i64, a[2] as i64);
    let m2 = x * x + y * y + z * z;
    let g = 8192i64;
    m2 > (g * 3 / 2) * (g * 3 / 2) || m2 < (g / 2) * (g / 2)
}

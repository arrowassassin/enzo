//! The X3 pin map (firmware-design/02-hardware.md §3), as GPIO numbers.

/// I²C SCL.
pub const I2C_SCL: u8 = 0;
/// Key ladder, group 1 (Back, Confirm, Left, Right).
pub const KEYS_1: u8 = 1;
/// Key ladder, group 2 (Up, Down).
pub const KEYS_2: u8 = 2;
/// Power key, active low, the deep-sleep wake source.
pub const POWER: u8 = 3;
/// E-paper data/command.
pub const EPD_DC: u8 = 4;
/// E-paper reset, active low.
pub const EPD_RST: u8 = 5;
/// E-paper busy, low = busy.
pub const EPD_BUSY: u8 = 6;
/// SD MISO.
pub const SD_MISO: u8 = 7;
/// SPI clock shared by the panel and the card.
pub const SPI_SCLK: u8 = 8;
/// SPI MOSI shared by the panel and the card.
pub const SPI_MOSI: u8 = 10;
/// SD chip select (held high while the panel is driven).
pub const SD_CS: u8 = 12;
/// SD power rail enable, active high; driven low and held through deep sleep.
pub const SD_POWER: u8 = 13;
/// I²C SDA.
pub const I2C_SDA: u8 = 20;
/// E-paper chip select.
pub const EPD_CS: u8 = 21;

/// BQ27220 fuel gauge address.
pub const ADDR_GAUGE: u8 = 0x55;
/// DS3231 real-time clock address.
pub const ADDR_RTC: u8 = 0x68;
/// QMI8658 IMU addresses (SA0 low / high).
pub const ADDR_IMU: [u8; 2] = [0x6B, 0x6A];

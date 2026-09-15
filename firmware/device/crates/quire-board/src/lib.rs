//! Board support for the Xteink X3 (ESP32-C3): the pin map, the two-ladder key decoder,
//! the SD card filesystem behind `quire_fs::Fs`, the I²C fuel gauge, clock and IMU, the
//! resume state kept across sleep, the flash partitions and the OTA writer. Both the
//! app and the recovery firmware build on it, so it stays free of the UI environment
//! and the network layer (those live in `bin/quire-x3`).
//!
//! The pure logic (key decoding, path splitting, BCD, gauge maths, the firmware image
//! walk and the otadata entries) has no HAL dependency and is unit-tested on the host
//! with `--no-default-features`.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

pub mod image;
pub mod keys;
pub mod otadata;
pub mod path;
pub mod pins;
pub mod util;

#[cfg(feature = "hal")]
pub mod assets;
#[cfg(feature = "hal")]
pub mod bus;
#[cfg(feature = "hal")]
pub mod flash;
#[cfg(feature = "hal")]
pub mod i2c;
#[cfg(feature = "hal")]
pub mod ota;
#[cfg(feature = "hal")]
pub mod power;
#[cfg(feature = "hal")]
pub mod sdfs;

//! Board support for the Xteink X3 (ESP32-C3): the pin map, the two-ladder key decoder,
//! the SD card filesystem behind `quire_fs::Fs`, the I²C fuel gauge, clock and IMU, the
//! resume state kept across sleep, and the [`Env`](quire_ui::Env) the UI runs on.
//!
//! The pure logic (key decoding, path splitting, BCD, gauge maths) has no HAL dependency
//! and is unit-tested on the host with `--no-default-features`.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

pub mod keys;
pub mod path;
pub mod pins;
pub mod util;

#[cfg(feature = "hal")]
pub mod bus;
#[cfg(feature = "hal")]
pub mod env;
#[cfg(feature = "hal")]
pub mod i2c;
#[cfg(feature = "hal")]
pub mod power;
#[cfg(feature = "hal")]
pub mod sdfs;

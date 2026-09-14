//! Driver-level tests on recording fakes: every command/data transaction the driver puts on the
//! bus is captured and compared against the sequences in the references (PapyriX `Display.cpp`
//! `_x3Mode`, `Uc8279X3Driver.cpp`, `x3-uc8279-driver-reference.md`, `x3-lut-waveforms.md`,
//! `X3DisplayProbeClassifier.cpp`). The waveform tables are re-typed here from the reference
//! sources, independently of the `const fn` builders in the crate.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::rc::Rc;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{ErrorType as PinErrorType, InputPin, OutputPin};
use embedded_hal::spi::{ErrorType as SpiErrorType, SpiBus};
use quire_epd::probe::{self, ProbeBus, Verdict};
use quire_epd::{
    plane_pixel, rotate_bits_to_plane, rotate_frame_to_plane, uc8253, uc8279, Controller, Epd, Error, Mode, Rotation, FRAME_H,
    FRAME_STRIDE, FRAME_W, PANEL_H, PANEL_W, PLANE_BYTES, ROW_BYTES,
};

// ------------------------------------------------------------------------------------------------
// Fakes
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ev {
    Cs(bool),
    Dc(bool),
    Rst(bool),
    Spi(Vec<u8>),
    /// Milliseconds.
    Delay(u32),
}

type Log = Rc<RefCell<Vec<Ev>>>;

struct Spi(Log);

impl SpiErrorType for Spi {
    type Error = Infallible;
}

impl SpiBus<u8> for Spi {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Infallible> {
        words.fill(0);
        Ok(())
    }
    fn write(&mut self, words: &[u8]) -> Result<(), Infallible> {
        self.0.borrow_mut().push(Ev::Spi(words.to_vec()));
        Ok(())
    }
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Infallible> {
        read.fill(0);
        self.write(write)
    }
    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Infallible> {
        let w = words.to_vec();
        words.fill(0);
        self.write(&w)
    }
    fn flush(&mut self) -> Result<(), Infallible> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Which {
    Cs,
    Dc,
    Rst,
}

struct Pin(Log, Which);

impl PinErrorType for Pin {
    type Error = Infallible;
}

impl OutputPin for Pin {
    fn set_low(&mut self) -> Result<(), Infallible> {
        self.set(false)
    }
    fn set_high(&mut self) -> Result<(), Infallible> {
        self.set(true)
    }
}

impl Pin {
    fn set(&mut self, high: bool) -> Result<(), Infallible> {
        self.0.borrow_mut().push(match self.1 {
            Which::Cs => Ev::Cs(high),
            Which::Dc => Ev::Dc(high),
            Which::Rst => Ev::Rst(high),
        });
        Ok(())
    }
}

/// Scripted BUSY line: each `is_low()` pops the next scripted level; when the script is empty the
/// line reads `idle_low` (default: high = idle).
#[derive(Default)]
struct BusyScript {
    queue: VecDeque<bool>,
    idle_low: bool,
}

struct Busy(Rc<RefCell<BusyScript>>);

impl PinErrorType for Busy {
    type Error = Infallible;
}

impl InputPin for Busy {
    fn is_high(&mut self) -> Result<bool, Infallible> {
        self.is_low().map(|l| !l)
    }
    fn is_low(&mut self) -> Result<bool, Infallible> {
        let mut s = self.0.borrow_mut();
        Ok(s.queue.pop_front().unwrap_or(s.idle_low))
    }
}

struct Delay(Log);

impl DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        self.0.borrow_mut().push(Ev::Delay(ns / 1_000_000));
    }
}

type Dut = Epd<Spi, Pin, Pin, Busy, Pin, Delay>;

/// One CS window: the command byte and everything sent with DC high after it.
#[derive(Clone, PartialEq, Eq)]
struct Tx {
    cmd: u8,
    data: Vec<u8>,
}

impl std::fmt::Debug for Tx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let head: Vec<String> = self.data.iter().take(8).map(|b| format!("{b:02X}")).collect();
        write!(f, "0x{:02X} [{} bytes: {}{}]", self.cmd, self.data.len(), head.join(" "), if self.data.len() > 8 { " …" } else { "" })
    }
}

fn tx(cmd: u8, data: &[u8]) -> Tx {
    Tx { cmd, data: data.to_vec() }
}

struct Rig {
    log: Log,
    busy: Rc<RefCell<BusyScript>>,
}

impl Rig {
    /// Collapse the log into transactions and clear it. Panics on a malformed transaction.
    fn take_txs(&self) -> Vec<Tx> {
        let events = std::mem::take(&mut *self.log.borrow_mut());
        let mut txs = Vec::new();
        let mut dc = true;
        let mut cur: Option<(Option<u8>, Vec<u8>)> = None;
        for ev in events {
            match ev {
                Ev::Dc(h) => dc = h,
                Ev::Cs(false) => {
                    assert!(cur.is_none(), "CS asserted twice");
                    cur = Some((None, Vec::new()));
                }
                Ev::Cs(true) => {
                    if let Some((cmd, data)) = cur.take() {
                        txs.push(Tx { cmd: cmd.expect("CS window without a command byte"), data });
                    }
                }
                Ev::Spi(bytes) => {
                    let (cmd, data) = cur.as_mut().expect("SPI write with CS high");
                    if dc {
                        assert!(cmd.is_some(), "data before command");
                        data.extend_from_slice(&bytes);
                    } else {
                        assert!(cmd.is_none() && bytes.len() == 1, "exactly one command byte per window");
                        *cmd = Some(bytes[0]);
                    }
                }
                Ev::Rst(_) | Ev::Delay(_) => {}
            }
        }
        assert!(cur.is_none(), "CS left asserted");
        txs
    }

    /// Reset-pin events and delays so far, in order (the log is left in place for `take_txs`).
    fn timing(&self) -> Vec<Ev> {
        self.log.borrow().iter().filter(|e| matches!(e, Ev::Rst(_) | Ev::Delay(_))).cloned().collect()
    }

    fn delays(&self) -> Vec<u32> {
        self.log.borrow().iter().filter_map(|e| if let Ev::Delay(ms) = e { Some(*ms) } else { None }).collect()
    }

    /// Script `n` BUSY cycles: high (not yet asserted), low, low, high (complete).
    fn expect_busy_waits(&self, n: usize) {
        let mut s = self.busy.borrow_mut();
        for _ in 0..n {
            s.queue.extend([false, true, true, false]);
        }
    }

    fn busy_left(&self) -> usize {
        self.busy.borrow().queue.len()
    }

    fn set_busy_idle_low(&self, low: bool) {
        self.busy.borrow_mut().idle_low = low;
    }
}

fn rig(controller: Controller) -> (Dut, Rig) {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let busy = Rc::new(RefCell::new(BusyScript::default()));
    let epd = Epd::new(
        Spi(log.clone()),
        Pin(log.clone(), Which::Dc),
        Pin(log.clone(), Which::Rst),
        Busy(busy.clone()),
        Pin(log.clone(), Which::Cs),
        Delay(log.clone()),
        controller,
    );
    (epd, Rig { log, busy })
}

fn assert_txs(actual: &[Tx], expected: &[Tx]) {
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert!(a == e, "transaction {i}: got {a:?}, expected {e:?}");
    }
    assert_eq!(actual.len(), expected.len(), "transaction count; extra: {:?}", &actual[expected.len().min(actual.len())..]);
}

// ------------------------------------------------------------------------------------------------
// Plane helpers
// ------------------------------------------------------------------------------------------------

type Plane = [u8; PLANE_BYTES];

fn plane_pattern(seed: u8) -> Box<Plane> {
    let mut p = Box::new([0u8; PLANE_BYTES]);
    for r in 0..PANEL_H {
        for c in 0..ROW_BYTES {
            p[r * ROW_BYTES + c] = (r as u8).wrapping_mul(31) ^ (c as u8) ^ seed;
        }
    }
    p
}

/// What the panel must receive for `plane`: rows in reverse order (row 527 first), optionally inverted.
fn stream(plane: &Plane, invert: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(PLANE_BYTES);
    for r in (0..PANEL_H).rev() {
        out.extend(plane[r * ROW_BYTES..(r + 1) * ROW_BYTES].iter().map(|&b| if invert { !b } else { b }));
    }
    assert_eq!(out.len(), 52_272);
    out
}

fn plane_tx(cmd: u8, plane: &Plane, invert: bool) -> Tx {
    Tx { cmd, data: stream(plane, invert) }
}

// ------------------------------------------------------------------------------------------------
// Reference tables, re-typed
// ------------------------------------------------------------------------------------------------

const fn pad<const N: usize>(src: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < src.len() {
        out[i] = src[i];
        i += 1;
    }
    out
}

/// PapyriX `lut_x3_*_full` (Display.cpp, "X3 reverse-exact full refresh LUTs"). Trailing bytes are 0x00.
const PX_FULL: [[u8; 42]; 5] = [
    pad(&[0x00, 0x06, 0x02, 0x06, 0x06, 0x01, 0x00, 0x05, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x20, 0x06, 0x02, 0x06, 0x06, 0x01, 0x00, 0x05, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0xAA, 0x06, 0x02, 0x06, 0x06, 0x01, 0x80, 0x05, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x55, 0x06, 0x02, 0x06, 0x06, 0x01, 0x40, 0x05, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x10, 0x06, 0x02, 0x06, 0x06, 0x01, 0x00, 0x05, 0x01, 0x00, 0x00, 0x01]),
];
/// PapyriX `lut_x3_*_turbo`.
const PX_TURBO: [[u8; 42]; 5] = [
    pad(&[0x00, 0x04, 0x02, 0x04, 0x04, 0x01, 0x00, 0x04, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x20, 0x04, 0x02, 0x04, 0x04, 0x01, 0x00, 0x04, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0xAA, 0x04, 0x02, 0x04, 0x04, 0x01, 0x80, 0x04, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x55, 0x04, 0x02, 0x04, 0x04, 0x01, 0x40, 0x04, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x10, 0x04, 0x02, 0x04, 0x04, 0x01, 0x00, 0x04, 0x01, 0x00, 0x00, 0x01]),
];
/// PapyriX `lut_x3_*_half`.
const PX_HALF: [[u8; 42]; 5] = [
    pad(&[0x00, 0x06, 0x01, 0x06, 0x06, 0x01, 0x00, 0x04, 0x01, 0x01, 0x00, 0x01]),
    pad(&[0xAA, 0x06, 0x01, 0x06, 0x06, 0x01, 0xA0, 0x04, 0x01, 0x01, 0x00, 0x01]),
    pad(&[0xAA, 0x06, 0x01, 0x06, 0x06, 0x01, 0xA0, 0x04, 0x01, 0x01, 0x00, 0x01]),
    pad(&[0x55, 0x06, 0x01, 0x06, 0x06, 0x01, 0x50, 0x04, 0x01, 0x01, 0x00, 0x01]),
    pad(&[0x55, 0x06, 0x01, 0x06, 0x06, 0x01, 0x50, 0x04, 0x01, 0x01, 0x00, 0x01]),
];
/// PapyriX `lut_x3_*_img` ("X3 stock image-write LUTs").
const PX_IMG: [[u8; 42]; 5] = [
    pad(&[0x00, 0x08, 0x0B, 0x02, 0x03, 0x01, 0x00, 0x0C, 0x02, 0x07, 0x02, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x01]),
    pad(&[0xA8, 0x08, 0x0B, 0x02, 0x03, 0x01, 0x44, 0x0C, 0x02, 0x07, 0x02, 0x01, 0x04, 0x01, 0x00, 0x02, 0x00, 0x01]),
    pad(&[0x80, 0x08, 0x0B, 0x02, 0x03, 0x01, 0x62, 0x0C, 0x02, 0x07, 0x02, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x01]),
    pad(&[0x88, 0x08, 0x0B, 0x02, 0x03, 0x01, 0x60, 0x0C, 0x02, 0x07, 0x02, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x01]),
    pad(&[0x00, 0x08, 0x0B, 0x02, 0x03, 0x01, 0x4A, 0x0C, 0x02, 0x07, 0x02, 0x01, 0x88, 0x01, 0x00, 0x02, 0x00, 0x01]),
];
/// PapyriX `lut_x3_*_gray` ("gray_tuned").
const PX_GRAY: [[u8; 42]; 5] = [
    pad(&[0x00, 0x03, 0x02, 0x01, 0x01, 0x01]),
    pad(&[0x20, 0x03, 0x02, 0x01, 0x01, 0x01]),
    pad(&[0x80, 0x03, 0x02, 0x01, 0x01, 0x01]),
    pad(&[0x00, 0x03, 0x02, 0x01, 0x01, 0x01]),
    pad(&[0x00, 0x03, 0x02, 0x01, 0x01, 0x01]),
];

/// PapyriX `initDisplayController`, `_x3Mode` branch, before the LUT load.
fn px_uc8253_init_txs() -> Vec<Tx> {
    vec![
        tx(0x00, &[0x3F, 0x08]),
        tx(0x61, &[0x03, 0x18, 0x02, 0x58]),
        tx(0x65, &[0x00, 0x00, 0x00, 0x00]),
        tx(0x03, &[0x1D]),
        tx(0x01, &[0x07, 0x17, 0x3F, 0x3F, 0x17]),
        tx(0x82, &[0x1D]),
        tx(0x06, &[0x25, 0x25, 0x3C, 0x37]),
        tx(0x30, &[0x09]),
        tx(0xE1, &[0x02]),
    ]
}

fn bank_txs(bank: &[[u8; 42]; 5]) -> Vec<Tx> {
    bank.iter().enumerate().map(|(i, t)| tx(0x20 + i as u8, t)).collect()
}

/// FreeInk `kUc8279X3_Init` (fi-Uc8279X3Luts.h), `{cmd, len, data...}` records.
const FI_INIT: &[u8] = &[
    0x00, 2, 0x3F, 0x4A, // PSR
    0x91, 0, // PTIN
    0x90, 9, 0x00, 0x00, 0x03, 0x17, 0x00, 0x00, 0x02, 0x0F, 0x01, // PTL: 792x528 window
    0x03, 1, 0x20, // PFS
    0x01, 5, 0x43, 0x00, 0x78, 0x78, 0x17, // PWR
    0x82, 1, 0x24, // VDCS
    0x06, 3, 0x25, 0x25, 0x3C, // BTST
    0x30, 1, 0x0F, // PLL
    0xE1, 1, 0x02, // gate scan
];

fn fi_uc8279_init_txs() -> Vec<Tx> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < FI_INIT.len() {
        let cmd = FI_INIT[i];
        let len = FI_INIT[i + 1] as usize;
        out.push(tx(cmd, &FI_INIT[i + 2..i + 2 + len]));
        i += 2 + len;
    }
    out
}

/// FreeInk `kUc8279X3_BwGc`.
const FI_BW_GC: [[u8; 43]; 5] = [
    pad(&[0x20, 0x01, 0x1A, 0x1A, 0x01, 0x00, 0x01, 0x01]),
    pad(&[0x21, 0x01, 0x5A, 0x9A, 0x01, 0x00, 0x01, 0x01]),
    pad(&[0x22, 0x01, 0x1A, 0x9A, 0x01, 0x00, 0x01, 0x01]),
    pad(&[0x23, 0x01, 0x1A, 0x5A, 0x01, 0x00, 0x01, 0x01]),
    pad(&[0x24, 0x01, 0x9A, 0x5A, 0x01, 0x00, 0x01, 0x01]),
];
/// FreeInk `kUc8279X3_BwDu`.
const FI_BW_DU: [[u8; 43]; 5] = [
    pad(&[0x20, 0x01, 0x07, 0x01, 0x06, 0x06, 0x01, 0x01, 0x01, 0x06, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x21, 0x01, 0x07, 0x81, 0x06, 0x06, 0x01, 0x01, 0x01, 0x06, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x22, 0x01, 0x87, 0x81, 0x86, 0x86, 0x01, 0x01, 0x01, 0x86, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x23, 0x01, 0x47, 0x41, 0x46, 0x46, 0x01, 0x01, 0x01, 0x46, 0x01, 0x00, 0x00, 0x01]),
    pad(&[0x24, 0x01, 0x07, 0x01, 0x06, 0x06, 0x01, 0x01, 0x01, 0x06, 0x01, 0x00, 0x00, 0x01]),
];
/// FreeInk `kUc8279X3_XtfAa`.
const FI_XTF_AA: [[u8; 49]; 5] = [
    pad(&[0x01, 0x03, 0x02, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01]),
    pad(&[0x01, 0x03, 0x02, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01]),
    pad(&[0x01, 0x83, 0x82, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01]),
    pad(&[0x01, 0x03, 0x82, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01]),
    pad(&[0x01, 0x03, 0x02, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01]),
];
/// FreeInk `kUc8279X3_Xth4`.
const FI_XTH4: [[u8; 49]; 5] = [
    pad(&[
        0x01, 0x10, 0x06, 0x02, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x02, 0x01, 0x01, 0x01, 0x14, 0x03, 0x02, 0x03, 0x01, 0x01, 0x01,
        0x01, 0x01, 0x01, 0x07, 0x01, 0x01, 0x01,
    ]),
    pad(&[
        0x01, 0x90, 0x86, 0x02, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x02, 0x01, 0x01, 0x01, 0x54, 0x83, 0x02, 0x83, 0x01, 0x01, 0x01,
        0x81, 0x41, 0x41, 0x47, 0x01, 0x01, 0x01,
    ]),
    pad(&[
        0x01, 0x90, 0x06, 0x82, 0x83, 0x01, 0x01, 0x01, 0x01, 0x41, 0x42, 0x42, 0x01, 0x01, 0x01, 0x54, 0x83, 0x42, 0x83, 0x01, 0x01, 0x01,
        0x01, 0x01, 0x01, 0x07, 0x01, 0x01, 0x01,
    ]),
    pad(&[
        0x01, 0x90, 0x06, 0x82, 0x83, 0x01, 0x01, 0x01, 0x81, 0x41, 0x42, 0x42, 0x01, 0x01, 0x01, 0x54, 0x83, 0x02, 0x03, 0x01, 0x01, 0x01,
        0x01, 0x01, 0x01, 0x07, 0x01, 0x01, 0x01,
    ]),
    pad(&[
        0x01, 0x10, 0x06, 0x82, 0x83, 0x01, 0x01, 0x01, 0x81, 0x01, 0x42, 0x42, 0x01, 0x01, 0x01, 0x54, 0x83, 0x82, 0x83, 0x01, 0x01, 0x01,
        0x81, 0x81, 0x81, 0x87, 0x01, 0x01, 0x01,
    ]),
];
/// FreeInk `kUc8279X3_XtfPreBwMid`.
const FI_XTF_PRE_BW_MID: [[u8; 43]; 5] = [
    pad(&[0x20, 0x01, 0x06, 0x01, 0x06, 0x06, 0x01, 0x01, 0x01, 0x02, 0x04, 0x00, 0x00, 0x01, 0x01]),
    pad(&[0x21, 0x01, 0x06, 0x81, 0x06, 0x06, 0x01, 0x01, 0x01, 0x02, 0x04, 0x00, 0x00, 0x01, 0x01]),
    pad(&[0x22, 0x01, 0x86, 0x81, 0x86, 0x86, 0x01, 0x01, 0x01, 0x82, 0x84, 0x00, 0x00, 0x01, 0x01]),
    pad(&[0x23, 0x01, 0x46, 0x41, 0x46, 0x46, 0x01, 0x01, 0x01, 0x42, 0x44, 0x00, 0x00, 0x01, 0x01]),
    pad(&[0x24, 0x01, 0x06, 0x01, 0x06, 0x06, 0x01, 0x01, 0x01, 0x02, 0x44, 0x00, 0x00, 0x01, 0x01]),
];

/// Command-prefixed bank as the driver must send it: command byte, then the 42 data bytes.
fn bank43_txs(bank: &[[u8; 43]; 5]) -> Vec<Tx> {
    bank.iter().map(|t| tx(t[0], &t[1..])).collect()
}

fn gray49_txs(bank: &[[u8; 49]; 5]) -> Vec<Tx> {
    bank.iter().enumerate().map(|(i, t)| tx(0x20 + i as u8, t)).collect()
}

const WINDOW: [u8; 9] = [0x00, 0x00, 0x03, 0x17, 0x00, 0x00, 0x02, 0x0F, 0x01];
const WHITE_PLANE_LEN: usize = 52_272;

// ------------------------------------------------------------------------------------------------
// Tables
// ------------------------------------------------------------------------------------------------

#[test]
fn uc8253_lut_banks_match_papyrix_byte_for_byte() {
    let check = |set: &uc8253::LutSet, px: &[[u8; 42]; 5], name: &str| {
        for (i, (c, t)) in set.tables().iter().enumerate() {
            assert_eq!(*c, 0x20 + i as u8);
            assert_eq!(*t, &px[i], "{name} table 0x{c:02X}");
        }
    };
    check(&uc8253::FULL, &PX_FULL, "full");
    check(&uc8253::TURBO, &PX_TURBO, "turbo");
    check(&uc8253::HALF, &PX_HALF, "half");
    check(&uc8253::IMG, &PX_IMG, "img");
    check(&uc8253::GRAY, &PX_GRAY, "gray");
    assert_eq!(uc8253::CDI_FULL_SYNC, [0xA9, 0x07]);
    assert_eq!(uc8253::CDI_FAST, [0x29, 0x07]);
    assert_eq!(uc8253::PTL_FULL_WINDOW, WINDOW);
}

#[test]
fn uc8279_tables_match_freeink_header_byte_for_byte() {
    assert_eq!(uc8279::BW_GC, FI_BW_GC);
    assert_eq!(uc8279::BW_DU, FI_BW_DU);
    assert_eq!(uc8279::XTF_AA, FI_XTF_AA);
    assert_eq!(uc8279::XTH4, FI_XTH4);
    assert_eq!(uc8279::XTF_PRE_BW_MID, FI_XTF_PRE_BW_MID);
    assert_eq!(uc8279::FULL_WINDOW, WINDOW);
    assert_eq!((uc8279::CDI_FIRST, uc8279::CDI_LATER), (0x97, 0xD7));
    assert_eq!((uc8279::AA_PRE_E0, uc8279::AA_PRE_E5), (0x02, 0x5A));
    assert_eq!(uc8279::INITIAL_GC_REFRESHES, 2);
    assert_eq!(uc8279::INIT_SCRIPT.iter().map(|s| tx(s.cmd, s.data)).collect::<Vec<_>>(), fi_uc8279_init_txs());
}

// ------------------------------------------------------------------------------------------------
// Init
// ------------------------------------------------------------------------------------------------

#[test]
fn uc8253_init_sends_reset_script_and_full_bank() {
    let (mut epd, rig) = rig(Controller::Uc8253);
    epd.init().unwrap();
    let timing = rig.timing();
    // PapyriX resetDisplay (_x3Mode): HIGH, 20 ms, LOW, 10 ms, HIGH, 20 ms, then 50 ms.
    assert_eq!(timing, vec![Ev::Rst(true), Ev::Delay(20), Ev::Rst(false), Ev::Delay(10), Ev::Rst(true), Ev::Delay(20), Ev::Delay(50)]);
    let mut expected = px_uc8253_init_txs();
    expected.extend(bank_txs(&PX_FULL));
    assert_txs(&rig.take_txs(), &expected);
    assert!(!epd.is_powered());
    assert!(!epd.baseline_valid());
    assert!(epd.needs_gc());
    assert_eq!(rig.busy_left(), 0);
}

#[test]
fn uc8279_init_sends_reset_and_freeink_script() {
    let (mut epd, rig) = rig(Controller::Uc8279);
    epd.init().unwrap();
    let timing = rig.timing();
    // Reference "Power-On Register Script": RST HIGH 10 ms, LOW 10 ms, HIGH 10 ms, then 50 ms settle.
    assert_eq!(timing, vec![Ev::Rst(true), Ev::Delay(10), Ev::Rst(false), Ev::Delay(10), Ev::Rst(true), Ev::Delay(10), Ev::Delay(50)]);
    assert_txs(&rig.take_txs(), &fi_uc8279_init_txs());
    assert!(!epd.is_powered());
    assert!(!epd.baseline_valid());
}

#[test]
fn methods_before_init_fail() {
    let (mut epd, _rig) = rig(Controller::Uc8253);
    let p = plane_pattern(0);
    assert_eq!(epd.refresh(&p, Mode::Du), Err(Error::NotInitialized));
    assert_eq!(epd.begin_refresh_gray(&p, &p), Err(Error::NotInitialized));
    assert_eq!(epd.poll_refresh(0), Err(Error::NoPendingRefresh));
}

// ------------------------------------------------------------------------------------------------
// UC8253 refresh sequences
// ------------------------------------------------------------------------------------------------

/// Init plus the first (escalated) full sync, log cleared; returns the frame on the glass.
fn bring_up_uc8253() -> (Dut, Rig, Box<Plane>) {
    let (mut epd, rig) = rig(Controller::Uc8253);
    epd.init().unwrap();
    let p = plane_pattern(1);
    rig.expect_busy_waits(4);
    epd.refresh(&p, Mode::Du).unwrap();
    assert_eq!(rig.busy_left(), 0);
    rig.take_txs();
    (epd, rig, p)
}

#[test]
fn uc8253_first_refresh_escalates_to_img_full_sync_with_condition_and_turbo_passes() {
    let (mut epd, rig) = rig(Controller::Uc8253);
    epd.init().unwrap();
    rig.take_txs();
    let p = plane_pattern(1);
    // PON (was off), DRF, conditioning DRF, no-op turbo DRF.
    rig.expect_busy_waits(4);
    epd.refresh(&p, Mode::Du).unwrap();
    assert_eq!(rig.busy_left(), 0, "exactly four BUSY waits");
    assert!(rig.delays().contains(&200), "200 ms settle after a non-fast refresh");

    let mut expected = bank_txs(&PX_IMG);
    expected.extend([plane_tx(0x13, &p, true), plane_tx(0x10, &p, true), tx(0x50, &[0xA9, 0x07]), tx(0x04, &[]), tx(0x12, &[])]);
    // One-time conditioning pass (PapyriX `_x3InitialFullSyncsRemaining == 1`).
    expected.extend(bank_txs(&PX_FULL));
    expected.extend([tx(0x50, &[0x29, 0x07]), tx(0x91, &[]), tx(0x90, &WINDOW), plane_tx(0x13, &p, false), tx(0x92, &[]), tx(0x12, &[])]);
    // RED RAM sync, then the no-op turbo pass.
    expected.push(plane_tx(0x10, &p, false));
    expected.extend(bank_txs(&PX_TURBO));
    expected.extend([tx(0x50, &[0x29, 0x07]), plane_tx(0x13, &p, false), tx(0x12, &[])]);
    assert_txs(&rig.take_txs(), &expected);

    assert!(epd.is_powered());
    assert!(epd.baseline_valid());
    assert!(!epd.needs_gc());
    assert_eq!(epd.frames_since_gc(), 0);
}

#[test]
fn uc8253_plane_stream_is_52272_bytes_in_reverse_row_order() {
    let (mut epd, rig, _) = bring_up_uc8253();
    let p = plane_pattern(9);
    rig.expect_busy_waits(1);
    epd.refresh(&p, Mode::Du).unwrap();
    let txs = rig.take_txs();
    let dtm2 = txs.iter().find(|t| t.cmd == 0x13).unwrap();
    assert_eq!(dtm2.data.len(), WHITE_PLANE_LEN);
    assert_eq!(&dtm2.data[..ROW_BYTES], &p[527 * ROW_BYTES..528 * ROW_BYTES], "row 527 first");
    assert_eq!(&dtm2.data[527 * ROW_BYTES..], &p[..ROW_BYTES], "row 0 last");
}

#[test]
fn uc8253_du_refresh_sequence() {
    let (mut epd, rig, _) = bring_up_uc8253();
    let p = plane_pattern(2);
    rig.expect_busy_waits(1);
    epd.refresh(&p, Mode::Du).unwrap();
    assert_eq!(rig.busy_left(), 0);
    assert!(!rig.delays().contains(&200), "no settle delay on a fast refresh");
    // Turbo bank is already loaded (cache) from the no-op pass; PON not re-sent while powered.
    assert_txs(&rig.take_txs(), &[plane_tx(0x13, &p, false), tx(0x50, &[0x29, 0x07]), tx(0x12, &[]), plane_tx(0x10, &p, false)]);
    assert_eq!(epd.frames_since_gc(), 1);
    assert!(epd.baseline_valid());
}

#[test]
fn uc8253_gc_refresh_sequence() {
    let (mut epd, rig, _) = bring_up_uc8253();
    let p = plane_pattern(3);
    rig.expect_busy_waits(2); // DRF + no-op turbo DRF
    epd.refresh(&p, Mode::Gc).unwrap();
    assert_eq!(rig.busy_left(), 0);
    assert!(rig.delays().contains(&200));
    let mut expected = bank_txs(&PX_FULL);
    expected.extend([plane_tx(0x13, &p, false), tx(0x50, &[0x29, 0x07]), tx(0x12, &[]), plane_tx(0x10, &p, false)]);
    expected.extend(bank_txs(&PX_TURBO));
    expected.extend([tx(0x50, &[0x29, 0x07]), plane_tx(0x13, &p, false), tx(0x12, &[])]);
    assert_txs(&rig.take_txs(), &expected);
    assert_eq!(epd.frames_since_gc(), 0);
}

#[test]
fn uc8253_img_refresh_sequence_resends_pon_without_waiting() {
    let (mut epd, rig, _) = bring_up_uc8253();
    let p = plane_pattern(4);
    rig.expect_busy_waits(2); // DRF + no-op turbo DRF (PON is sent but not waited for while on)
    epd.refresh(&p, Mode::Img).unwrap();
    assert_eq!(rig.busy_left(), 0);
    let mut expected = bank_txs(&PX_IMG);
    expected.extend([
        plane_tx(0x13, &p, true),
        plane_tx(0x10, &p, true),
        tx(0x50, &[0xA9, 0x07]),
        tx(0x04, &[]),
        tx(0x12, &[]),
        plane_tx(0x10, &p, false),
    ]);
    // The conditioning pass ran once after init and is not repeated.
    expected.extend(bank_txs(&PX_TURBO));
    expected.extend([tx(0x50, &[0x29, 0x07]), plane_tx(0x13, &p, false), tx(0x12, &[])]);
    assert_txs(&rig.take_txs(), &expected);
}

#[test]
fn uc8253_half_refresh_sequence() {
    let (mut epd, rig, _) = bring_up_uc8253();
    let p = plane_pattern(5);
    rig.expect_busy_waits(1);
    epd.refresh(&p, Mode::Half).unwrap();
    assert_eq!(rig.busy_left(), 0);
    let mut expected = bank_txs(&PX_HALF);
    expected.extend([plane_tx(0x13, &p, false), tx(0x50, &[0xA9, 0x07]), tx(0x12, &[]), plane_tx(0x10, &p, false)]);
    assert_txs(&rig.take_txs(), &expected);
    assert_eq!(epd.frames_since_gc(), 0);
}

#[test]
fn uc8253_du_while_powered_off_becomes_half_and_reloads_luts() {
    let (mut epd, rig, _) = bring_up_uc8253();
    rig.expect_busy_waits(1);
    epd.power_off().unwrap();
    assert_txs(&rig.take_txs(), &[tx(0x02, &[])]);
    assert!(!epd.is_powered());

    let p = plane_pattern(6);
    rig.expect_busy_waits(2); // PON + DRF
    epd.refresh(&p, Mode::Du).unwrap();
    assert_eq!(rig.busy_left(), 0);
    let mut expected = bank_txs(&PX_HALF); // LUT cache dropped by the power-off
    expected.extend([plane_tx(0x13, &p, false), tx(0x50, &[0xA9, 0x07]), tx(0x04, &[]), tx(0x12, &[]), plane_tx(0x10, &p, false)]);
    assert_txs(&rig.take_txs(), &expected);
    assert!(epd.is_powered());
}

#[test]
fn uc8253_request_gc_escalates_next_du_to_full_sync() {
    let (mut epd, rig, _) = bring_up_uc8253();
    epd.request_gc();
    assert!(epd.needs_gc());
    let p = plane_pattern(7);
    rig.expect_busy_waits(2);
    epd.refresh(&p, Mode::Du).unwrap();
    let txs = rig.take_txs();
    assert_eq!(txs[0], tx(0x20, &PX_IMG[0]), "img bank loaded");
    assert!(txs.contains(&tx(0x50, &[0xA9, 0x07])));
    assert!(txs.contains(&plane_tx(0x10, &p, true)), "both RAMs written inverted");
    assert!(!epd.needs_gc());
}

#[test]
fn uc8253_gc_interval_hint() {
    let (mut epd, rig, _) = bring_up_uc8253();
    epd.set_gc_interval(2);
    for i in 0..2 {
        rig.expect_busy_waits(1);
        epd.refresh(&plane_pattern(10 + i), Mode::Du).unwrap();
    }
    assert!(epd.needs_gc());
    rig.expect_busy_waits(2);
    epd.refresh(&plane_pattern(20), Mode::Gc).unwrap();
    assert!(!epd.needs_gc());
}

#[test]
fn uc8253_gray_refresh_sequence_then_full_sync() {
    let (mut epd, rig, _) = bring_up_uc8253();
    let lsb = plane_pattern(0x40);
    let msb = plane_pattern(0x80);
    rig.expect_busy_waits(1);
    epd.refresh_gray(&lsb, &msb).unwrap();
    assert_eq!(rig.busy_left(), 0);
    let mut expected = vec![plane_tx(0x10, &lsb, false), plane_tx(0x13, &msb, false)];
    expected.extend(bank_txs(&PX_GRAY));
    expected.extend([tx(0x50, &[0x29, 0x07]), tx(0x12, &[])]);
    assert_txs(&rig.take_txs(), &expected);
    assert!(!epd.baseline_valid(), "grey planes overwrote both RAMs");

    // The next B/W refresh is a full sync with a fresh LUT load.
    let p = plane_pattern(8);
    rig.expect_busy_waits(2);
    epd.refresh(&p, Mode::Du).unwrap();
    let txs = rig.take_txs();
    assert_eq!(&txs[..5], &bank_txs(&PX_IMG)[..]);
    assert_eq!(txs[5], plane_tx(0x13, &p, true));
    assert!(epd.baseline_valid());
}

// ------------------------------------------------------------------------------------------------
// UC8279 refresh sequences
// ------------------------------------------------------------------------------------------------

fn white_seed_tx() -> Tx {
    Tx { cmd: 0x10, data: vec![0xFF; WHITE_PLANE_LEN] }
}

#[test]
fn uc8279_first_two_refreshes_are_gc_then_du() {
    let (mut epd, rig) = rig(Controller::Uc8279);
    epd.init().unwrap();
    rig.take_txs();

    // Refresh 1: Du requested → GC (no baseline, initial GC count). DTM1 seeded white, CDI 0x97.
    let p1 = plane_pattern(1);
    rig.expect_busy_waits(2); // PON + DRF
    epd.refresh(&p1, Mode::Du).unwrap();
    assert_eq!(rig.busy_left(), 0);
    let mut expected = vec![tx(0x91, &[]), white_seed_tx(), tx(0x11, &[]), plane_tx(0x13, &p1, false), tx(0x11, &[]), tx(0x50, &[0x97])];
    expected.extend(bank43_txs(&FI_BW_GC));
    expected.extend([tx(0x04, &[]), tx(0x12, &[]), tx(0x50, &[0xD7]), plane_tx(0x10, &p1, false), tx(0x11, &[]), tx(0x92, &[])]);
    assert_txs(&rig.take_txs(), &expected);
    assert!(epd.baseline_valid());
    assert_eq!(epd.frames_since_gc(), 0);

    // Refresh 2: still GC (second initial refresh); real baseline in DTM1, CDI 0xD7, no PON.
    let p2 = plane_pattern(2);
    rig.expect_busy_waits(1);
    epd.refresh(&p2, Mode::Du).unwrap();
    assert_eq!(rig.busy_left(), 0);
    let mut expected = vec![tx(0x91, &[]), plane_tx(0x13, &p2, false), tx(0x11, &[]), tx(0x50, &[0xD7])];
    expected.extend(bank43_txs(&FI_BW_GC));
    expected.extend([tx(0x12, &[]), tx(0x50, &[0xD7]), plane_tx(0x10, &p2, false), tx(0x11, &[]), tx(0x92, &[])]);
    assert_txs(&rig.take_txs(), &expected);
    assert_eq!(epd.frames_since_gc(), 0);

    // Refresh 3: DU bank.
    let p3 = plane_pattern(3);
    rig.expect_busy_waits(1);
    epd.refresh(&p3, Mode::Du).unwrap();
    let mut expected = vec![tx(0x91, &[]), plane_tx(0x13, &p3, false), tx(0x11, &[]), tx(0x50, &[0xD7])];
    expected.extend(bank43_txs(&FI_BW_DU));
    expected.extend([tx(0x12, &[]), tx(0x50, &[0xD7]), plane_tx(0x10, &p3, false), tx(0x11, &[]), tx(0x92, &[])]);
    assert_txs(&rig.take_txs(), &expected);
    assert_eq!(epd.frames_since_gc(), 1);

    // Half / Img with a valid baseline: GC bank, no white seed.
    for (mode, seed) in [(Mode::Half, 4), (Mode::Img, 5)] {
        let p = plane_pattern(seed);
        rig.expect_busy_waits(1);
        epd.refresh(&p, mode).unwrap();
        let mut expected = vec![tx(0x91, &[]), plane_tx(0x13, &p, false), tx(0x11, &[]), tx(0x50, &[0xD7])];
        expected.extend(bank43_txs(&FI_BW_GC));
        expected.extend([tx(0x12, &[]), tx(0x50, &[0xD7]), plane_tx(0x10, &p, false), tx(0x11, &[]), tx(0x92, &[])]);
        assert_txs(&rig.take_txs(), &expected);
        assert_eq!(epd.frames_since_gc(), 0);
    }
}

#[test]
fn uc8279_plane_byte_count_and_row_order() {
    let (mut epd, rig) = rig(Controller::Uc8279);
    epd.init().unwrap();
    rig.take_txs();
    let p = plane_pattern(0x33);
    rig.expect_busy_waits(2);
    epd.refresh(&p, Mode::Gc).unwrap();
    let txs = rig.take_txs();
    let dtm1_seed = &txs[1];
    assert_eq!((dtm1_seed.cmd, dtm1_seed.data.len()), (0x10, 52_272));
    assert!(dtm1_seed.data.iter().all(|&b| b == 0xFF));
    let dtm2 = txs.iter().find(|t| t.cmd == 0x13).unwrap();
    assert_eq!(dtm2.data.len(), 52_272);
    assert_eq!(&dtm2.data[..ROW_BYTES], &p[527 * ROW_BYTES..]);
    let dtm1_sync = txs.iter().filter(|t| t.cmd == 0x10).nth(1).unwrap();
    assert_eq!(dtm1_sync.data, stream(&p, false));
}

#[test]
fn uc8279_dark_background_du_writes_complement_baseline() {
    let (mut epd, rig) = rig(Controller::Uc8279);
    epd.init().unwrap();
    for i in 0..2 {
        rig.expect_busy_waits(if i == 0 { 2 } else { 1 });
        epd.refresh(&plane_pattern(i), Mode::Du).unwrap();
    }
    rig.take_txs();
    epd.set_dark_background(true);
    let p = plane_pattern(0x55);
    rig.expect_busy_waits(1);
    epd.refresh(&p, Mode::Du).unwrap();
    let mut expected =
        vec![tx(0x91, &[]), plane_tx(0x10, &p, true), tx(0x11, &[]), plane_tx(0x13, &p, false), tx(0x11, &[]), tx(0x50, &[0xD7])];
    expected.extend(bank43_txs(&FI_BW_DU));
    expected.extend([tx(0x12, &[]), tx(0x50, &[0xD7]), plane_tx(0x10, &p, false), tx(0x11, &[]), tx(0x92, &[])]);
    assert_txs(&rig.take_txs(), &expected);

    // A GC refresh ignores the hint.
    let p = plane_pattern(0x56);
    rig.expect_busy_waits(1);
    epd.refresh(&p, Mode::Gc).unwrap();
    let txs = rig.take_txs();
    assert_eq!(txs[1], plane_tx(0x13, &p, false));
}

#[test]
fn uc8279_gray_refresh_sequence() {
    let (mut epd, rig) = rig(Controller::Uc8279);
    epd.init().unwrap();
    rig.take_txs();
    let lsb = plane_pattern(0x40);
    let msb = plane_pattern(0x80);
    rig.expect_busy_waits(2); // PON + DRF
    epd.refresh_gray(&lsb, &msb).unwrap();
    assert_eq!(rig.busy_left(), 0);
    let mut expected = vec![
        tx(0x91, &[]),
        tx(0x90, &WINDOW),
        plane_tx(0x10, &lsb, false),
        tx(0x11, &[]),
        plane_tx(0x13, &msb, false),
        tx(0x11, &[]),
        tx(0x92, &[]),
    ];
    expected.extend(gray49_txs(&FI_XTF_AA));
    expected.extend([tx(0x50, &[0x97]), tx(0x04, &[]), tx(0x12, &[])]);
    assert_txs(&rig.take_txs(), &expected);
    assert!(!epd.baseline_valid());

    // Next B/W refresh: white seed, GC, and CDI is now 0xD7 (first refresh consumed by the grey pass).
    let p = plane_pattern(1);
    rig.expect_busy_waits(1);
    epd.refresh(&p, Mode::Du).unwrap();
    let txs = rig.take_txs();
    assert_eq!(txs[1], white_seed_tx());
    assert_eq!(txs[5], tx(0x50, &[0xD7]));
    assert_eq!(txs[6], tx(0x20, &FI_BW_GC[0][1..]));
}

// ------------------------------------------------------------------------------------------------
// Split refresh and BUSY timeouts
// ------------------------------------------------------------------------------------------------

#[test]
fn split_refresh_polls_busy_then_finishes() {
    let (mut epd, rig) = rig(Controller::Uc8279);
    epd.init().unwrap();
    rig.take_txs();
    let p = plane_pattern(1);
    rig.expect_busy_waits(1); // the PON wait inside begin_refresh
    epd.begin_refresh(&p, Mode::Gc).unwrap();
    assert_eq!(rig.busy_left(), 0);
    assert_eq!(epd.begin_refresh(&p, Mode::Gc), Err(Error::RefreshPending));
    assert_eq!(epd.refresh(&p, Mode::Gc), Err(Error::RefreshPending));
    assert_eq!(epd.power_off(), Err(Error::RefreshPending));
    assert_eq!(epd.sleep(), Err(Error::RefreshPending));
    assert_eq!(epd.finish_refresh_gray(), Err(Error::NoPendingRefresh));
    let txs = rig.take_txs();
    assert_eq!(txs.last(), Some(&tx(0x12, &[])), "begin ends with DRF");

    // BUSY: still high right after DRF, then low for a while, then high.
    rig.busy.borrow_mut().queue.extend([false, true, true, false]);
    assert_eq!(epd.poll_refresh(0), Ok(false));
    assert_eq!(epd.poll_refresh(5), Ok(false));
    assert_eq!(epd.poll_refresh(50), Ok(false));
    assert_eq!(epd.poll_refresh(300), Ok(true));
    assert!(rig.take_txs().is_empty(), "polling touches no register");

    epd.finish_refresh(&p).unwrap();
    assert_txs(&rig.take_txs(), &[tx(0x50, &[0xD7]), plane_tx(0x10, &p, false), tx(0x11, &[]), tx(0x92, &[])]);
    assert!(epd.baseline_valid());
    assert_eq!(epd.finish_refresh(&p), Err(Error::NoPendingRefresh));
    assert_eq!(epd.poll_refresh(400), Err(Error::NoPendingRefresh));
}

#[test]
fn busy_assert_timeout_marks_baseline_invalid() {
    let (mut epd, rig, _) = bring_up_uc8253();
    let p = plane_pattern(2);
    epd.begin_refresh(&p, Mode::Du).unwrap();
    // BUSY never goes low.
    assert_eq!(epd.poll_refresh(999), Ok(false));
    assert_eq!(epd.poll_refresh(1_000), Err(Error::BusyAssertTimeout));
    assert!(!epd.baseline_valid());
    assert!(epd.needs_gc());
    assert_eq!(epd.finish_refresh(&p), Err(Error::NoPendingRefresh));
    rig.take_txs();

    // The next refresh escalates to a full sync.
    rig.expect_busy_waits(2);
    epd.refresh(&p, Mode::Du).unwrap();
    let txs = rig.take_txs();
    assert_eq!(txs[0], tx(0x20, &PX_IMG[0]));
    assert!(epd.baseline_valid());
}

#[test]
fn busy_complete_timeout_marks_baseline_invalid() {
    let (mut epd, rig) = rig(Controller::Uc8279);
    epd.init().unwrap();
    rig.expect_busy_waits(2);
    epd.refresh(&plane_pattern(1), Mode::Gc).unwrap();
    assert!(epd.baseline_valid());
    let p = plane_pattern(2);
    epd.begin_refresh(&p, Mode::Gc).unwrap();
    rig.set_busy_idle_low(true); // BUSY asserts and never releases
    assert_eq!(epd.poll_refresh(1), Ok(false));
    assert_eq!(epd.poll_refresh(29_999), Ok(false));
    assert_eq!(epd.poll_refresh(30_000), Err(Error::BusyCompleteTimeout));
    assert!(!epd.baseline_valid());
    assert!(epd.needs_gc());
    assert_eq!(epd.finish_refresh(&p), Err(Error::NoPendingRefresh));
}

#[test]
fn blocking_refresh_times_out_after_one_second_of_polling() {
    let (mut epd, rig, _) = bring_up_uc8253();
    let p = plane_pattern(2);
    assert_eq!(epd.refresh(&p, Mode::Du), Err(Error::BusyAssertTimeout));
    let polls = rig.delays().iter().filter(|&&d| d == 1).count();
    assert_eq!(polls, 1_000, "1 ms polls until the assert timeout");
    assert!(!epd.baseline_valid());
    let txs = rig.take_txs();
    assert_eq!(txs.last(), Some(&tx(0x12, &[])), "aborted before the RAM sync");
}

#[test]
fn pon_timeout_during_begin_leaves_no_pending_refresh() {
    let (mut epd, rig) = rig(Controller::Uc8279);
    epd.init().unwrap();
    rig.take_txs();
    let p = plane_pattern(1);
    assert_eq!(epd.begin_refresh(&p, Mode::Du), Err(Error::BusyAssertTimeout));
    assert!(!epd.is_powered());
    assert_eq!(epd.poll_refresh(0), Err(Error::NoPendingRefresh));
    let txs = rig.take_txs();
    assert_eq!(txs.last(), Some(&tx(0x04, &[])), "stopped at PON");
}

// ------------------------------------------------------------------------------------------------
// Power off, sleep, wake
// ------------------------------------------------------------------------------------------------

#[test]
fn power_off_is_pof_then_busy_wait_and_idempotent() {
    let (mut epd, rig, _) = bring_up_uc8253();
    rig.expect_busy_waits(1);
    epd.power_off().unwrap();
    assert_eq!(rig.busy_left(), 0);
    assert_txs(&rig.take_txs(), &[tx(0x02, &[])]);
    assert!(!epd.is_powered());
    assert!(epd.baseline_valid(), "power-off keeps the RAM baseline");
    epd.power_off().unwrap();
    assert!(rig.take_txs().is_empty());
}

#[test]
fn power_off_timeout_still_marks_off_and_forces_gc() {
    let (mut epd, rig, _) = bring_up_uc8253();
    assert_eq!(epd.power_off(), Err(Error::BusyAssertTimeout));
    assert!(!epd.is_powered());
    assert!(epd.needs_gc());
    rig.take_txs();
}

#[test]
fn sleep_sends_pof_then_dslp_a5_and_wake_reinitialises() {
    for controller in [Controller::Uc8253, Controller::Uc8279] {
        let (mut epd, rig) = rig(controller);
        epd.init().unwrap();
        rig.expect_busy_waits(if controller == Controller::Uc8253 { 4 } else { 2 });
        epd.refresh(&plane_pattern(1), Mode::Du).unwrap();
        rig.take_txs();
        assert!(epd.is_powered());

        rig.expect_busy_waits(1);
        epd.sleep().unwrap();
        assert_eq!(rig.busy_left(), 0);
        assert_txs(&rig.take_txs(), &[tx(0x02, &[]), tx(0x07, &[0xA5])]);
        assert!(!epd.is_powered());
        assert!(!epd.baseline_valid());
        assert_eq!(epd.refresh(&plane_pattern(2), Mode::Du), Err(Error::NotInitialized));
        assert_eq!(epd.power_off(), Ok(()), "{controller:?}: nothing to do while asleep");
        assert!(rig.take_txs().is_empty());

        epd.wake().unwrap();
        let timing = rig.timing();
        assert!(matches!(timing.as_slice(), [Ev::Rst(true), Ev::Delay(_), Ev::Rst(false), Ev::Delay(_), Ev::Rst(true), ..]));
        let txs = rig.take_txs();
        let expected = match controller {
            Controller::Uc8253 => {
                let mut e = px_uc8253_init_txs();
                e.extend(bank_txs(&PX_FULL));
                e
            }
            Controller::Uc8279 => fi_uc8279_init_txs(),
        };
        assert_txs(&txs, &expected);
        assert!(!epd.baseline_valid(), "first refresh after wake is a full sync");
    }
}

#[test]
fn sleep_while_already_off_skips_pof() {
    let (mut epd, rig) = rig(Controller::Uc8279);
    epd.init().unwrap();
    rig.take_txs();
    epd.sleep().unwrap();
    assert_txs(&rig.take_txs(), &[tx(0x07, &[0xA5])]);
}

// ------------------------------------------------------------------------------------------------
// Probe
// ------------------------------------------------------------------------------------------------

/// Bit-banged ProbeBus fake: decodes the command shifted out on SDA (captured on the SCLK rising
/// edge while DC is low), then answers the read with a scripted byte string. Beyond the script the
/// line "floats" to the pull-up level.
struct FakeProbe {
    /// cmd → successive responses (the last one repeats).
    responses: HashMap<u8, Vec<Vec<u8>>>,
    read_index: HashMap<u8, usize>,
    /// Level of a released line with nothing driving it.
    floating_high: bool,
    sclk: bool,
    dc: bool,
    cs: bool,
    rst: bool,
    mosi: Option<bool>,
    shift: u8,
    nbits: u8,
    last_cmd: Option<u8>,
    /// Every register read: (command, bytes the probe clocked in).
    reads: Vec<(u8, usize)>,
    bits: VecDeque<bool>,
    bytes_read: usize,
    total_us: u64,
    rst_low_since: Option<u64>,
    reset_lows_ms: Vec<u32>,
    sampled_with_sclk_high: bool,
    cmd_with_cs_high: bool,
}

impl FakeProbe {
    fn new(floating_high: bool) -> Self {
        FakeProbe {
            responses: HashMap::new(),
            read_index: HashMap::new(),
            floating_high,
            sclk: false,
            dc: true,
            cs: true,
            rst: true,
            mosi: Some(false),
            shift: 0,
            nbits: 0,
            last_cmd: None,
            reads: Vec::new(),
            bits: VecDeque::new(),
            bytes_read: 0,
            total_us: 0,
            rst_low_since: None,
            reset_lows_ms: Vec::new(),
            sampled_with_sclk_high: false,
            cmd_with_cs_high: false,
        }
    }

    fn respond(mut self, cmd: u8, bytes: &[u8]) -> Self {
        self.responses.entry(cmd).or_default().push(bytes.to_vec());
        self
    }

    fn end_read(&mut self) {
        if let Some(cmd) = self.last_cmd.take() {
            if self.bytes_read > 0 {
                self.reads.push((cmd, self.bytes_read));
            }
        }
        self.bits.clear();
        self.bytes_read = 0;
    }
}

impl ProbeBus for FakeProbe {
    fn rst(&mut self, high: bool) {
        if !high && self.rst {
            self.rst_low_since = Some(self.total_us);
        }
        if high {
            if let Some(since) = self.rst_low_since.take() {
                self.reset_lows_ms.push(((self.total_us - since) / 1000) as u32);
            }
        }
        self.rst = high;
    }
    fn cs(&mut self, high: bool) {
        if high && !self.cs {
            self.end_read();
        }
        self.cs = high;
        self.shift = 0;
        self.nbits = 0;
    }
    fn dc(&mut self, high: bool) {
        self.dc = high;
    }
    fn sclk(&mut self, high: bool) {
        if high && !self.sclk {
            if let Some(level) = self.mosi {
                if !self.dc {
                    if self.cs {
                        self.cmd_with_cs_high = true;
                    }
                    self.shift = (self.shift << 1) | u8::from(level);
                    self.nbits += 1;
                    if self.nbits == 8 {
                        self.last_cmd = Some(self.shift);
                        self.nbits = 0;
                        self.shift = 0;
                    }
                }
            }
        }
        self.sclk = high;
    }
    fn mosi_drive(&mut self, high: bool) {
        self.mosi = Some(high);
    }
    fn mosi_release(&mut self) {
        self.mosi = None;
        self.bits.clear();
        if let Some(cmd) = self.last_cmd {
            if let Some(list) = self.responses.get(&cmd) {
                let idx = *self.read_index.get(&cmd).unwrap_or(&0);
                let resp = &list[idx.min(list.len() - 1)];
                self.read_index.insert(cmd, idx + 1);
                for &b in resp {
                    for bit in 0..8 {
                        self.bits.push_back(b & (0x80 >> bit) != 0);
                    }
                }
            }
        }
    }
    fn mosi_read(&mut self) -> bool {
        if self.sclk {
            self.sampled_with_sclk_high = true;
        }
        if self.nbits == 7 {
            self.bytes_read += 1;
            self.nbits = 0;
        } else {
            self.nbits += 1;
        }
        self.bits.pop_front().unwrap_or(self.floating_high)
    }
    fn delay_us(&mut self, us: u32) {
        self.total_us += u64::from(us);
    }
}

fn mtp_field_dump() -> [u8; 49] {
    // One dummy byte, then 48 bytes of a blank field MTP: zeros with the LUT version stamp at 0x1A.
    let mut d = [0u8; 49];
    d[0] = 0xFF;
    d[1 + 0x1A] = 0x66;
    d
}

#[test]
fn probe_confirms_uc8279_from_structured_ver() {
    let mut bus =
        FakeProbe::new(true).respond(0x71, &[0x13]).respond(0x70, &[0x00, 0x03, 0x00, 0x00, 0x66]).respond(0xA2, &mtp_field_dump());
    let r = probe::probe(&mut bus);
    assert_eq!(r.verdict, Verdict::Uc8279Confirmed);
    assert_eq!(r.controller(), Controller::Uc8279);
    assert!(r.conclusive());
    assert_eq!(r.pass1.ver, [0x00, 0x03, 0x00, 0x00, 0x66]);
    assert_eq!(r.pass2, r.pass1);
    assert!(r.mtp_valid);
    assert_eq!(r.mtp[0x1A], 0x66);
    // Structured pass 1 → no repeat; pass 2 with the 50 ms reset. Reset: 2 ms high, low, 30 ms settle.
    assert_eq!(bus.reset_lows_ms, vec![1, 50]);
    assert_eq!(bus.reads, vec![(0x71, 1), (0x70, 5), (0x71, 1), (0x70, 5), (0xA2, 49)]);
    assert!(!bus.sampled_with_sclk_high, "SDA sampled while SCLK is low");
    assert!(!bus.cmd_with_cs_high);
    assert!(bus.cs && !bus.sclk && bus.rst && bus.mosi == Some(false), "pins handed back: CS high, SCLK low, RST high, MOSI driven low");
}

#[test]
fn probe_confirms_uc8279_field_module_by_repeatable_blank_mtp() {
    let mut bus = FakeProbe::new(true).respond(0x71, &[0x13]).respond(0x70, &[0xFF; 5]).respond(0xA2, &mtp_field_dump());
    let r = probe::probe(&mut bus);
    assert_eq!(r.verdict, Verdict::Uc8279Confirmed);
    assert!(r.mtp_valid && r.mtp_repeatable);
    assert_eq!(r.mtp[0x1A], 0x66);
    assert_eq!(r.mtp[0], 0x00);
    // Pass 1 (1 ms) not structured → repeated with 50 ms; pass 2 with 1 ms; RMTP read twice.
    assert_eq!(bus.reset_lows_ms, vec![1, 50, 1]);
    assert_eq!(bus.reads.iter().filter(|(c, _)| *c == 0xA2).count(), 2);

    // A dump that does not repeat is what a floating bus would produce: inconclusive.
    let mut d2 = mtp_field_dump();
    d2[5] = 0x01;
    let mut bus = FakeProbe::new(true).respond(0x71, &[0x13]).respond(0x70, &[0xFF; 5]).respond(0xA2, &mtp_field_dump()).respond(0xA2, &d2);
    let r = probe::probe(&mut bus);
    assert_eq!(r.verdict, Verdict::Inconclusive);
    assert!(!r.mtp_repeatable);
    assert_eq!(r.controller(), Controller::Uc8253);
    assert!(!r.conclusive());

    // A programmed MTP (0xA5 key) needs no repeat read.
    let mut keyed = [0u8; 49];
    keyed[1] = 0xA5;
    let mut bus = FakeProbe::new(true).respond(0x71, &[0x13]).respond(0x70, &[0xFF; 5]).respond(0xA2, &keyed);
    let r = probe::probe(&mut bus);
    assert_eq!(r.verdict, Verdict::Uc8279Confirmed);
    assert_eq!(bus.reads.iter().filter(|(c, _)| *c == 0xA2).count(), 1);
}

#[test]
fn probe_classifies_floating_bus_as_uc8253() {
    // Nothing answers: pull-up reads 0xFF everywhere. FLG is not driven idle, so RMTP is never read.
    let mut bus = FakeProbe::new(true);
    let r = probe::probe(&mut bus);
    assert_eq!(r.verdict, Verdict::Uc8253StableDefault);
    assert_eq!(r.controller(), Controller::Uc8253);
    assert!(r.conclusive());
    assert_eq!((r.pass1.flg, r.pass1.ver), (0xFF, [0xFF; 5]));
    assert!(!r.mtp_valid);
    assert_eq!(bus.reset_lows_ms, vec![1, 50, 1]);
    assert_eq!(bus.reads, vec![(0x71, 1), (0x70, 5), (0x71, 1), (0x70, 5), (0x71, 1), (0x70, 5)]);

    // Same with a line that floats low.
    let mut bus = FakeProbe::new(false);
    let r = probe::probe(&mut bus);
    assert_eq!(r.verdict, Verdict::Uc8253StableDefault);
    assert_eq!((r.pass1.flg, r.pass1.ver), (0x00, [0x00; 5]));

    // Driven idle FLG but the RMTP line floats (uniform dump): UC8253, RMTP read once.
    let mut bus = FakeProbe::new(true).respond(0x71, &[0x13]).respond(0x70, &[0xFF; 5]);
    let r = probe::probe(&mut bus);
    assert_eq!(r.verdict, Verdict::Uc8253StableDefault);
    assert!(r.mtp_valid && !r.mtp_repeatable);
    assert_eq!(r.mtp, [0xFF; 48]);
    assert_eq!(bus.reads.iter().filter(|(c, _)| *c == 0xA2).count(), 1);
}

#[test]
fn probe_read_register_decodes_scripted_bytes() {
    let mut bus = FakeProbe::new(true).respond(0x70, &[0x00, 0x03, 0x00, 0x00, 0x66]);
    let mut ver = [0u8; 5];
    probe::read_register(&mut bus, 0x70, &mut ver);
    assert_eq!(ver, [0x00, 0x03, 0x00, 0x00, 0x66]);
    let mut extra = [0u8; 2];
    probe::read_register(&mut bus, 0x99, &mut extra);
    assert_eq!(extra, [0xFF, 0xFF], "unknown register floats to the pull-up");
}

// ------------------------------------------------------------------------------------------------
// Rotation helper (public API)
// ------------------------------------------------------------------------------------------------

fn frame_with(rot: Rotation, pixels: &[(usize, usize)]) -> Vec<u8> {
    let (w, h) = rot.frame_size();
    let stride = w / 8;
    let mut f = vec![0u8; stride * h];
    for &(x, y) in pixels {
        f[y * stride + x / 8] |= 0x80 >> (x % 8);
    }
    f
}

fn black_pixels(plane: &Plane) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for py in 0..PANEL_H {
        for px in 0..PANEL_W {
            if !plane_pixel(plane, px, py) {
                out.push((px, py));
            }
        }
    }
    out
}

#[test]
fn rotation_helper_places_a_known_pixel_in_all_four_rotations() {
    // Content pixel (3, 5) → device-portrait (dx, dy) → panel (dy, 527 - dx).
    let cases = [
        (Rotation::Portrait, (3, 5)),
        (Rotation::Flip180, (527 - 3, 791 - 5)),
        (Rotation::Cw90, (527 - 5, 3)),
        (Rotation::Ccw90, (5, 791 - 3)),
    ];
    for (rot, (dx, dy)) in cases {
        assert_eq!(rot.to_device(3, 5), (dx, dy), "{rot:?}");
        let mut plane = Box::new([0u8; PLANE_BYTES]);
        rotate_frame_to_plane(&frame_with(rot, &[(3, 5)]), &mut plane, rot);
        assert_eq!(black_pixels(&plane), vec![(dy, 527 - dx)], "{rot:?}");
    }
}

#[test]
fn rotation_helper_inverts_polarity_and_grey_planes_keep_light_bits() {
    // Frame: 1 = ink → plane: 1 = white, so an empty frame is an all-0xFF plane.
    let mut plane = Box::new([0u8; PLANE_BYTES]);
    rotate_frame_to_plane(&vec![0u8; FRAME_STRIDE * FRAME_H], &mut plane, Rotation::Portrait);
    assert!(plane.iter().all(|&b| b == 0xFF));
    // Full-ink frame → all black.
    rotate_frame_to_plane(&vec![0xFFu8; FRAME_STRIDE * FRAME_H], &mut plane, Rotation::Portrait);
    assert!(plane.iter().all(|&b| b == 0x00));
    // Grey planes: set bit = lighter = 1 in the plane, starting from all black.
    let mut grey = Box::new([0xAAu8; PLANE_BYTES]);
    rotate_bits_to_plane(&frame_with(Rotation::Portrait, &[(0, 0), (FRAME_W - 1, FRAME_H - 1)]), &mut grey, Rotation::Portrait, false);
    assert_eq!(grey.iter().map(|b| b.count_ones()).sum::<u32>(), 2);
    assert!(plane_pixel(&grey, 0, 527));
    assert!(plane_pixel(&grey, 791, 0));
}

#[test]
fn rotation_helper_round_trips_through_plane_pixel() {
    let pixels: Vec<(usize, usize)> = (0..300).map(|i| ((i * 37) % FRAME_W, (i * 91) % FRAME_H)).collect();
    for rot in [Rotation::Portrait, Rotation::Flip180] {
        let mut plane = Box::new([0u8; PLANE_BYTES]);
        rotate_frame_to_plane(&frame_with(rot, &pixels), &mut plane, rot);
        let mut expected: Vec<(usize, usize)> = pixels
            .iter()
            .map(|&(x, y)| {
                let (dx, dy) = rot.to_device(x, y);
                (dy, 527 - dx)
            })
            .collect();
        expected.sort_unstable();
        expected.dedup();
        let mut got = black_pixels(&plane);
        got.sort_unstable();
        assert_eq!(got, expected, "{rot:?}");
    }
    // Flip180 applied twice is the identity on device coordinates.
    for &(x, y) in &[(0, 0), (17, 300), (527, 791)] {
        let (dx, dy) = Rotation::Flip180.to_device(x, y);
        assert_eq!(Rotation::Flip180.to_device(dx, dy), (x, y));
    }
}

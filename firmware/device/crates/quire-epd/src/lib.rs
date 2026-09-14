//! Xteink X3 e-paper panel driver for Quire.
//!
//! One driver, two controllers: the original **UC8253** and the **UC8279d_B** fitted to units
//! shipped since July 2026. Which one is present is decided at boot by [`probe`]; the register
//! scripts, waveform tables and refresh sequences of each live in [`uc8253`] and [`uc8279`], the
//! shared transaction/BUSY/state machinery here.
//!
//! * `#![no_std]`, no allocation: the caller owns the 52,272-byte plane buffers.
//! * Only `embedded-hal` 1.0 traits (`SpiBus`, `OutputPin`, `InputPin`, `DelayNs`), so the crate
//!   runs on esp-hal's SPI/GPIO types on the device and on recording fakes on the host.
//! * The caller guarantees exclusive use of the shared SPI bus (SD CS high) while a method runs;
//!   the driver only asserts/deasserts its own CS around every transaction.
//!
//! Panel plane conventions and the framebuffer rotation helper are in [`plane`].
//!
//! # Refresh flow
//!
//! [`Epd::refresh`] runs the whole three-phase flow and blocks (~400 ms). To keep polling keys
//! meanwhile, use the split form: [`Epd::begin_refresh`] (uploads the frame and triggers the
//! waveform), [`Epd::poll_refresh`] (non-blocking BUSY check with the same bounded timeouts) and
//! [`Epd::finish_refresh`] (syncs the old-frame RAM and leaves the window). Sources:
//! x3-lut-waveforms.md "Refresh Flow", x3-uc8279-driver-reference.md "Refresh Sequences" /
//! "BUSY Handling", PapyriX `Display.cpp` (`_x3Mode`) and `Uc8279X3Driver.cpp`.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod plane;
pub mod probe;
pub mod uc8253;
pub mod uc8279;

pub use plane::{
    plane_pixel, portrait_to_panel, rotate_bits_to_plane, rotate_frame_to_plane, Rotation, FRAME_H, FRAME_STRIDE, FRAME_W,
    PANEL_H, PANEL_W, PLANE_BYTES, ROW_BYTES,
};
pub use probe::{classify, probe, ProbeBus, ProbeResult, ProbeSample, Verdict};

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiBus;
use uc8253::Step;

/// Which panel controller the unit has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    /// Original X3 controller: 10 MHz SPI max, LUTs in registers, no window RAM commands.
    Uc8253,
    /// UC8279d_B (units since July 2026): 20 MHz SPI, blank MTP, external waveforms.
    Uc8279,
}

impl Controller {
    /// Maximum safe SPI clock. UC8253: 10 MHz (20 MHz caused pixel damage in PapyriX testing).
    /// UC8279d: 20 MHz ("Clock rate up to 20 MHz", datasheet).
    pub const fn spi_hz(self) -> u32 {
        match self {
            Controller::Uc8253 => 10_000_000,
            Controller::Uc8279 => 20_000_000,
        }
    }
}

/// SPI clock for a controller (4-wire, mode 0, MSB first on both).
pub const fn spi_hz(controller: Controller) -> u32 {
    controller.spi_hz()
}

/// Refresh mode requested by the caller. The driver may escalate (never de-escalate) a request:
/// a `Du` without a valid old-frame baseline becomes a full sync, a `Du` while the analog power
/// is off becomes a `Half` on the UC8253, and the first two UC8279 refreshes after init use GC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Fast differential page turn (UC8253 turbo bank ~382 ms; UC8279 `BW_DU`).
    Du,
    /// Quality differential refresh: the periodic "clean" update (UC8253 full bank ~472 ms;
    /// UC8279 `BW_GC`). Followed by one no-op turbo pass on the UC8253.
    Gc,
    /// Scrub refresh that does not depend on the old frame and does not flash (UC8253 half
    /// bank; UC8279 `BW_GC`).
    Half,
    /// Full image sync: both RAMs rewritten, longest and cleanest (UC8253 img bank ~908 ms plus a
    /// no-op turbo pass; UC8279 `BW_GC`).
    Img,
}

/// Driver errors. Timeouts leave the driver usable: they mark the baseline invalid so the next
/// refresh is a full sync (x3-uc8279-driver-reference.md "BUSY Handling").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The SPI bus returned an error.
    Spi,
    /// A GPIO returned an error.
    Pin,
    /// BUSY did not assert within [`BUSY_ASSERT_TIMEOUT_MS`] after PON/DRF/POF.
    BusyAssertTimeout,
    /// BUSY stayed asserted longer than [`BUSY_COMPLETE_TIMEOUT_MS`].
    BusyCompleteTimeout,
    /// [`Epd::init`] has not run (or the panel is asleep).
    NotInitialized,
    /// A refresh is already in flight (`begin_refresh` twice, or `refresh` during a split refresh).
    RefreshPending,
    /// `poll_refresh`/`finish_refresh` without a matching `begin_refresh`, or the wrong finish.
    NoPendingRefresh,
}

/// Time allowed for BUSY to assert after a command, ms (PapyriX).
pub const BUSY_ASSERT_TIMEOUT_MS: u32 = 1_000;
/// Time allowed for a waveform to complete once BUSY is asserted, ms (PapyriX).
pub const BUSY_COMPLETE_TIMEOUT_MS: u32 = 30_000;
/// Default number of fast refreshes after which [`Epd::needs_gc`] recommends a `Gc`.
pub const DEFAULT_GC_INTERVAL: u32 = 8;

/// Shared UC81xx command bytes.
pub mod cmd {
    /// Panel setting.
    pub const PSR: u8 = 0x00;
    /// Power off.
    pub const POF: u8 = 0x02;
    /// Power on.
    pub const PON: u8 = 0x04;
    /// Deep sleep (data 0xA5).
    pub const DSLP: u8 = 0x07;
    /// Data transmission 1 — old frame ("RED RAM" on the UC8253, DTM1 on the UC8279).
    pub const DTM1: u8 = 0x10;
    /// Data stop (UC8279).
    pub const DSP: u8 = 0x11;
    /// Display refresh.
    pub const DRF: u8 = 0x12;
    /// Data transmission 2 — new frame.
    pub const DTM2: u8 = 0x13;
    /// VCOM and data interval.
    pub const CDI: u8 = 0x50;
    /// Partial window.
    pub const PTL: u8 = 0x90;
    /// Partial in.
    pub const PTIN: u8 = 0x91;
    /// Partial out.
    pub const PTOUT: u8 = 0x92;
    /// Deep-sleep check code.
    pub const DSLP_KEY: u8 = 0xA5;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BusyPhase {
    WaitingLow,
    WaitingHigh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingKind {
    /// A B/W refresh; `mode` is the resolved (possibly escalated) mode.
    Bw { mode: Mode },
    /// A grey refresh.
    Gray,
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    kind: PendingKind,
    phase: BusyPhase,
}

/// The panel driver. Generic over the bus and pins so it runs unchanged on esp-hal and on fakes.
pub struct Epd<SPI, DC, RST, BUSY, CS, D> {
    spi: SPI,
    dc: DC,
    rst: RST,
    busy: BUSY,
    cs: CS,
    delay: D,
    controller: Controller,
    initialized: bool,
    powered: bool,
    /// The old-frame RAM (0x10 / DTM1) holds what is on the glass.
    baseline_valid: bool,
    /// Next refresh must be a full sync (timeout, explicit request, or after a grey pass).
    force_gc: bool,
    frames_since_gc: u32,
    gc_interval: u32,
    pending: Option<Pending>,
    // UC8253
    loaded_bank: Option<uc8253::Bank>,
    // UC8279
    first_refresh: bool,
    initial_gc_remaining: u8,
    dark_background: bool,
}

type Plane = [u8; PLANE_BYTES];

impl<SPI, DC, RST, BUSY, CS, D> Epd<SPI, DC, RST, BUSY, CS, D>
where
    SPI: SpiBus,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    CS: OutputPin,
    D: DelayNs,
{
    /// Wrap the peripherals. Nothing is sent; call [`Epd::init`] next. The SPI bus must already
    /// be configured for mode 0, MSB first, at most [`Controller::spi_hz`].
    pub fn new(spi: SPI, dc: DC, rst: RST, busy: BUSY, cs: CS, delay: D, controller: Controller) -> Self {
        Epd {
            spi,
            dc,
            rst,
            busy,
            cs,
            delay,
            controller,
            initialized: false,
            powered: false,
            baseline_valid: false,
            force_gc: false,
            frames_since_gc: 0,
            gc_interval: DEFAULT_GC_INTERVAL,
            pending: None,
            loaded_bank: None,
            first_refresh: true,
            initial_gc_remaining: uc8279::INITIAL_GC_REFRESHES,
            dark_background: false,
        }
    }

    /// Give the peripherals back.
    pub fn release(self) -> (SPI, DC, RST, BUSY, CS, D) {
        (self.spi, self.dc, self.rst, self.busy, self.cs, self.delay)
    }

    /// Borrow the SPI bus (for the SD card between panel transactions; the panel CS is high).
    pub fn spi_mut(&mut self) -> &mut SPI {
        &mut self.spi
    }

    /// The controller this driver was built for.
    pub const fn controller(&self) -> Controller {
        self.controller
    }

    /// Analog power (charge pump) is on.
    pub const fn is_powered(&self) -> bool {
        self.powered
    }

    /// The old-frame RAM matches the glass, so a `Du` will really be differential.
    pub const fn baseline_valid(&self) -> bool {
        self.baseline_valid
    }

    /// Fast refreshes since the last GC-class refresh (`Gc`/`Img`, or the UC8279 GC bank).
    pub const fn frames_since_gc(&self) -> u32 {
        self.frames_since_gc
    }

    /// Set how many fast refreshes [`Epd::needs_gc`] tolerates before recommending a `Gc`.
    pub fn set_gc_interval(&mut self, frames: u32) {
        self.gc_interval = frames.max(1);
    }

    /// Hint: the next refresh should be a `Gc` (ghosting budget spent, baseline lost, or a
    /// resync was requested). The driver escalates a lost baseline on its own; the interval is
    /// only advice.
    pub const fn needs_gc(&self) -> bool {
        !self.baseline_valid || self.force_gc || self.frames_since_gc >= self.gc_interval
    }

    /// Force the next refresh to a full sync (`requestResync` in PapyriX).
    pub fn request_gc(&mut self) {
        self.force_gc = true;
    }

    /// UC8279 only: the page is mostly black. Before a DU refresh the driver then writes DTM1 as
    /// the complement of the new frame so every pixel drives towards its target (reference
    /// "B/W DU"). Ignored on the UC8253.
    pub fn set_dark_background(&mut self, dark: bool) {
        self.dark_background = dark;
    }

    // ------------------------------------------------------------------------------------------
    // Transactions
    // ------------------------------------------------------------------------------------------

    fn select(&mut self) -> Result<(), Error> {
        self.cs.set_low().map_err(|_| Error::Pin)
    }

    fn deselect(&mut self) -> Result<(), Error> {
        self.spi.flush().map_err(|_| Error::Spi)?;
        self.cs.set_high().map_err(|_| Error::Pin)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.spi.write(bytes).map_err(|_| Error::Spi)
    }

    /// One command, no data: CS low, DC low, byte, CS high.
    fn command(&mut self, c: u8) -> Result<(), Error> {
        self.command_data(c, &[])
    }

    /// One command with data in a single CS window.
    fn command_data(&mut self, c: u8, data: &[u8]) -> Result<(), Error> {
        self.dc.set_low().map_err(|_| Error::Pin)?;
        self.select()?;
        self.write(&[c])?;
        if !data.is_empty() {
            self.dc.set_high().map_err(|_| Error::Pin)?;
            self.write(data)?;
        }
        self.deselect()
    }

    fn run_script(&mut self, script: &[Step]) -> Result<(), Error> {
        for s in script {
            self.command_data(s.cmd, s.data)?;
        }
        Ok(())
    }

    /// Stream a plane after `c`, rows in reverse order (row 527 first), optionally inverted.
    fn send_plane(&mut self, c: u8, plane: &Plane, invert: bool) -> Result<(), Error> {
        self.dc.set_low().map_err(|_| Error::Pin)?;
        self.select()?;
        self.write(&[c])?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        let mut row = [0u8; ROW_BYTES];
        for r in (0..PANEL_H).rev() {
            let src = &plane[r * ROW_BYTES..(r + 1) * ROW_BYTES];
            if invert {
                for (d, s) in row.iter_mut().zip(src) {
                    *d = !s;
                }
                self.write(&row)?;
            } else {
                self.write(src)?;
            }
        }
        self.deselect()
    }

    /// Fill a plane register with one byte value (UC8279 white DTM1 seed).
    fn fill_plane(&mut self, c: u8, value: u8) -> Result<(), Error> {
        self.dc.set_low().map_err(|_| Error::Pin)?;
        self.select()?;
        self.write(&[c])?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        let row = [value; ROW_BYTES];
        for _ in 0..PANEL_H {
            self.write(&row)?;
        }
        self.deselect()
    }

    fn load_uc8253_bank(&mut self, bank: uc8253::Bank) -> Result<(), Error> {
        if self.loaded_bank == Some(bank) {
            return Ok(());
        }
        for (c, table) in bank.set().tables() {
            self.command_data(c, table)?;
        }
        self.loaded_bank = Some(bank);
        Ok(())
    }

    fn load_uc8279_bank(&mut self, bank: &uc8279::Bank) -> Result<(), Error> {
        for table in bank {
            self.command_data(table[0], &table[1..])?;
        }
        Ok(())
    }

    fn load_uc8279_gray_bank(&mut self) -> Result<(), Error> {
        for (i, table) in uc8279::XTF_AA.iter().enumerate() {
            self.command_data(0x20 + i as u8, table)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------
    // BUSY
    // ------------------------------------------------------------------------------------------

    /// One non-blocking step of the two-phase BUSY wait (`waitForRefresh` in PapyriX): first wait
    /// for the LOW edge (bounded by the assert timeout), then for HIGH (completion timeout).
    fn busy_step(&mut self, phase: &mut BusyPhase, elapsed_ms: u32) -> Result<bool, Error> {
        let low = self.busy.is_low().map_err(|_| Error::Pin)?;
        match *phase {
            BusyPhase::WaitingLow => {
                if low {
                    *phase = BusyPhase::WaitingHigh;
                    Ok(false)
                } else if elapsed_ms >= BUSY_ASSERT_TIMEOUT_MS {
                    Err(Error::BusyAssertTimeout)
                } else {
                    Ok(false)
                }
            }
            BusyPhase::WaitingHigh => {
                if !low {
                    Ok(true)
                } else if elapsed_ms >= BUSY_COMPLETE_TIMEOUT_MS {
                    Err(Error::BusyCompleteTimeout)
                } else {
                    Ok(false)
                }
            }
        }
    }

    /// Blocking BUSY wait, 1 ms polling. On timeout the baseline is invalidated and the next
    /// refresh forced to a full sync.
    fn wait_busy(&mut self) -> Result<(), Error> {
        let mut phase = BusyPhase::WaitingLow;
        let mut elapsed = 0u32;
        loop {
            match self.busy_step(&mut phase, elapsed) {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    self.delay.delay_ms(1);
                    elapsed += 1;
                }
                Err(e) => {
                    self.invalidate_after_timeout();
                    return Err(e);
                }
            }
        }
    }

    fn invalidate_after_timeout(&mut self) {
        self.baseline_valid = false;
        self.force_gc = true;
        self.pending = None;
    }

    // ------------------------------------------------------------------------------------------
    // Init / power / sleep
    // ------------------------------------------------------------------------------------------

    /// Hardware reset pulse. UC8253: high 20 ms, low 10 ms, high 20 ms + 50 ms settle (PapyriX
    /// `resetDisplay`, `_x3Mode`). UC8279d: high 10 ms, low 50 ms, high 10 ms + 50 ms settle
    /// (reference "Power-On Register Script" gives low 10 ms; PapyriX `bus.reset(50)` uses 50 —
    /// the longer pulse is harmless and is what runs in the field).
    fn hardware_reset(&mut self) -> Result<(), Error> {
        let (pre, low, post, settle) = match self.controller {
            Controller::Uc8253 => (20, 10, 20, 50),
            Controller::Uc8279 => (10, 50, 10, 50),
        };
        self.rst.set_high().map_err(|_| Error::Pin)?;
        self.delay.delay_ms(pre);
        self.rst.set_low().map_err(|_| Error::Pin)?;
        self.delay.delay_ms(low);
        self.rst.set_high().map_err(|_| Error::Pin)?;
        self.delay.delay_ms(post);
        self.delay.delay_ms(settle);
        Ok(())
    }

    /// Hardware reset followed by the controller's register script (and, on the UC8253, the
    /// `FULL` LUT bank as PapyriX does). Leaves the panel powered off with no valid baseline: the
    /// first refresh is a full sync.
    pub fn init(&mut self) -> Result<(), Error> {
        self.cs.set_high().map_err(|_| Error::Pin)?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        self.hardware_reset()?;
        self.initialized = false;
        self.powered = false;
        self.baseline_valid = false;
        self.force_gc = false;
        self.frames_since_gc = 0;
        self.pending = None;
        self.loaded_bank = None;
        self.first_refresh = true;
        self.initial_gc_remaining = uc8279::INITIAL_GC_REFRESHES;
        match self.controller {
            Controller::Uc8253 => {
                self.run_script(uc8253::INIT_SCRIPT)?;
                self.load_uc8253_bank(uc8253::Bank::Full)?;
            }
            Controller::Uc8279 => self.run_script(uc8279::INIT_SCRIPT)?,
        }
        self.initialized = true;
        Ok(())
    }

    /// Reset and re-initialise after [`Epd::sleep`] (identical to [`Epd::init`]).
    pub fn wake(&mut self) -> Result<(), Error> {
        self.init()
    }

    /// Turn the analog power off (POF, wait BUSY). No-op when already off. A BUSY timeout still
    /// marks the panel off and forces a full sync next (`recoverAfterPowerOffTimeout`).
    pub fn power_off(&mut self) -> Result<(), Error> {
        if self.pending.is_some() {
            return Err(Error::RefreshPending);
        }
        if !self.powered {
            return Ok(());
        }
        self.command(cmd::POF)?;
        let r = self.wait_busy();
        self.powered = false;
        // PapyriX drops its LUT cache after a power-off; mirror it so the next refresh reloads.
        self.loaded_bank = None;
        if r.is_err() {
            self.force_gc = true;
        }
        r
    }

    /// Power off (if on) then DSLP 0x07 / 0xA5. Only a hardware reset ([`Epd::wake`]) leaves deep
    /// sleep; every other method returns [`Error::NotInitialized`] until then.
    pub fn sleep(&mut self) -> Result<(), Error> {
        if self.pending.is_some() {
            return Err(Error::RefreshPending);
        }
        if self.powered {
            // Ignore a POF timeout: we are going to sleep anyway (PapyriX/FreeInk do the same).
            let _ = self.power_off();
        }
        self.command_data(cmd::DSLP, &[cmd::DSLP_KEY])?;
        self.initialized = false;
        self.powered = false;
        self.baseline_valid = false;
        self.loaded_bank = None;
        Ok(())
    }

    fn ensure_ready(&mut self) -> Result<(), Error> {
        if !self.initialized {
            return Err(Error::NotInitialized);
        }
        if self.pending.is_some() {
            return Err(Error::RefreshPending);
        }
        Ok(())
    }

    /// Power on if off: PON then wait BUSY. Returns whether PON was sent.
    fn power_on_if_off(&mut self) -> Result<bool, Error> {
        if self.powered {
            return Ok(false);
        }
        self.command(cmd::PON)?;
        self.wait_busy()?;
        self.powered = true;
        Ok(true)
    }

    // ------------------------------------------------------------------------------------------
    // B/W refresh
    // ------------------------------------------------------------------------------------------

    /// Full blocking refresh: upload, trigger, wait for BUSY, sync the old-frame RAM.
    pub fn refresh(&mut self, plane: &Plane, mode: Mode) -> Result<(), Error> {
        self.begin_refresh(plane, mode)?;
        self.wait_pending()?;
        self.finish_refresh(plane)
    }

    /// Two-plane 4-level grey refresh. `plane_lsb`/`plane_msb` are the low and high bit of each
    /// pixel's grey value (0 = black … 3 = white, so both planes carry 1 = lighter; see
    /// [`rotate_bits_to_plane`]). Afterwards the old-frame baseline is invalid: the next B/W
    /// refresh is a full sync, which is also how the grey image is cleaned off.
    pub fn refresh_gray(&mut self, plane_lsb: &Plane, plane_msb: &Plane) -> Result<(), Error> {
        self.begin_refresh_gray(plane_lsb, plane_msb)?;
        self.wait_pending()?;
        self.finish_refresh_gray()
    }

    fn wait_pending(&mut self) -> Result<(), Error> {
        let r = self.wait_busy();
        if r.is_err() {
            self.pending = None;
        }
        r
    }

    /// Phase 1 of a split refresh: upload the frame, load the waveform, power on if needed and
    /// send DRF. Returns immediately; the panel is now busy for the waveform's duration.
    pub fn begin_refresh(&mut self, plane: &Plane, mode: Mode) -> Result<(), Error> {
        self.ensure_ready()?;
        let resolved = match self.controller {
            Controller::Uc8253 => self.uc8253_begin(plane, mode)?,
            Controller::Uc8279 => self.uc8279_begin(plane, mode)?,
        };
        self.pending = Some(Pending { kind: PendingKind::Bw { mode: resolved }, phase: BusyPhase::WaitingLow });
        Ok(())
    }

    /// Phase 1 of a split grey refresh (see [`Epd::refresh_gray`]).
    pub fn begin_refresh_gray(&mut self, plane_lsb: &Plane, plane_msb: &Plane) -> Result<(), Error> {
        self.ensure_ready()?;
        match self.controller {
            Controller::Uc8253 => self.uc8253_begin_gray(plane_lsb, plane_msb)?,
            Controller::Uc8279 => self.uc8279_begin_gray(plane_lsb, plane_msb)?,
        }
        self.pending = Some(Pending { kind: PendingKind::Gray, phase: BusyPhase::WaitingLow });
        Ok(())
    }

    /// Phase 2: non-blocking BUSY check. `elapsed_ms` is the caller's time since `begin_refresh`
    /// (the driver has no clock). Returns `Ok(true)` once the waveform has completed; a timeout
    /// aborts the refresh, invalidates the baseline and returns the error.
    pub fn poll_refresh(&mut self, elapsed_ms: u32) -> Result<bool, Error> {
        let Some(mut p) = self.pending else {
            return Err(Error::NoPendingRefresh);
        };
        match self.busy_step(&mut p.phase, elapsed_ms) {
            Ok(done) => {
                self.pending = Some(p);
                Ok(done)
            }
            Err(e) => {
                self.invalidate_after_timeout();
                Err(e)
            }
        }
    }

    /// Phase 3 of a B/W refresh, after [`Epd::poll_refresh`] returned `Ok(true)`: the settle delay,
    /// old-frame RAM sync (same `plane` as `begin_refresh`), PTOUT, and on the UC8253 the no-op
    /// turbo pass after a `Gc`/`Img` (blocking, ~382 ms).
    pub fn finish_refresh(&mut self, plane: &Plane) -> Result<(), Error> {
        let Some(Pending { kind: PendingKind::Bw { mode }, .. }) = self.pending else {
            return Err(Error::NoPendingRefresh);
        };
        self.pending = None;
        let r = match self.controller {
            Controller::Uc8253 => self.uc8253_finish(plane, mode),
            Controller::Uc8279 => self.uc8279_finish(plane, mode),
        };
        if r.is_ok() {
            self.baseline_valid = true;
            self.force_gc = false;
            if mode == Mode::Du {
                self.frames_since_gc += 1;
            } else {
                self.frames_since_gc = 0;
            }
        }
        r
    }

    /// Phase 3 of a grey refresh.
    pub fn finish_refresh_gray(&mut self) -> Result<(), Error> {
        let Some(Pending { kind: PendingKind::Gray, .. }) = self.pending else {
            return Err(Error::NoPendingRefresh);
        };
        self.pending = None;
        // The grey planes overwrote both RAMs; the next B/W refresh must be a full sync.
        self.baseline_valid = false;
        self.first_refresh = false;
        if self.controller == Controller::Uc8253 {
            self.loaded_bank = None;
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------
    // UC8253 sequences (PapyriX Display.cpp, `_x3Mode` branch of refreshDisplay/displayGray)
    // ------------------------------------------------------------------------------------------

    fn uc8253_resolve(&self, mode: Mode) -> Mode {
        if !self.baseline_valid || self.force_gc {
            // `!_x3RedRamSynced || forcedFullSync` → full sync with the img bank.
            Mode::Img
        } else if mode == Mode::Du && !self.powered {
            // `_x3Mode && !isScreenOn && FAST_REFRESH` → HALF_REFRESH.
            Mode::Half
        } else {
            mode
        }
    }

    fn uc8253_begin(&mut self, plane: &Plane, mode: Mode) -> Result<Mode, Error> {
        let mode = self.uc8253_resolve(mode);
        match mode {
            Mode::Img => {
                // Full sync: img LUTs, inverted data to both RAMs, border drive active.
                self.load_uc8253_bank(uc8253::Bank::Img)?;
                self.send_plane(cmd::DTM2, plane, true)?;
                self.send_plane(cmd::DTM1, plane, true)?;
                self.command_data(cmd::CDI, &uc8253::CDI_FULL_SYNC)?;
            }
            Mode::Half => {
                self.load_uc8253_bank(uc8253::Bank::Half)?;
                self.send_plane(cmd::DTM2, plane, false)?;
                self.command_data(cmd::CDI, &uc8253::CDI_FULL_SYNC)?;
            }
            Mode::Gc => {
                // Quality differential pass with the full bank; PapyriX only uses this bank for its
                // conditioning pass, with CDI 0x29 0x07 (border held), so that is what we send.
                self.load_uc8253_bank(uc8253::Bank::Full)?;
                self.send_plane(cmd::DTM2, plane, false)?;
                self.command_data(cmd::CDI, &uc8253::CDI_FAST)?;
            }
            Mode::Du => {
                self.load_uc8253_bank(uc8253::Bank::Turbo)?;
                self.send_plane(cmd::DTM2, plane, false)?;
                self.command_data(cmd::CDI, &uc8253::CDI_FAST)?;
            }
        }
        // `if (wasOff || doFullSync) sendCommand(0x04); if (wasOff) wait;`
        let was_off = !self.powered;
        if was_off || matches!(mode, Mode::Img | Mode::Gc) {
            self.command(cmd::PON)?;
            if was_off {
                self.wait_busy()?;
            }
            self.powered = true;
        }
        self.command(cmd::DRF)?;
        Ok(mode)
    }

    fn uc8253_finish(&mut self, plane: &Plane, mode: Mode) -> Result<(), Error> {
        if mode != Mode::Du {
            self.delay.delay_ms(uc8253::SETTLE_MS);
        }
        // Sync RED RAM (0x10) with the non-inverted frame for the next differential update.
        self.send_plane(cmd::DTM1, plane, false)?;
        if matches!(mode, Mode::Img | Mode::Gc) {
            // One no-op turbo pass on the same frame (x3-lut-waveforms.md "Refresh Flow"): cleans
            // up the first differential refresh that follows.
            self.load_uc8253_bank(uc8253::Bank::Turbo)?;
            self.command_data(cmd::CDI, &uc8253::CDI_FAST)?;
            self.send_plane(cmd::DTM2, plane, false)?;
            self.command(cmd::DRF)?;
            self.wait_busy()?;
        }
        Ok(())
    }

    fn uc8253_begin_gray(&mut self, lsb: &Plane, msb: &Plane) -> Result<(), Error> {
        // LSB → old-data RAM 0x10, MSB → 0x13, grey bank, CDI 0x29 0x07, PON if off, DRF.
        self.send_plane(cmd::DTM1, lsb, false)?;
        self.send_plane(cmd::DTM2, msb, false)?;
        self.loaded_bank = None; // always reload: the bank cache is not trusted across grey passes
        self.load_uc8253_bank(uc8253::Bank::Gray)?;
        self.command_data(cmd::CDI, &uc8253::CDI_FAST)?;
        self.power_on_if_off()?;
        self.command(cmd::DRF)?;
        Ok(())
    }

    // ------------------------------------------------------------------------------------------
    // UC8279 sequences (Uc8279X3Driver.cpp display/displayGray, reference "Refresh Sequences")
    // ------------------------------------------------------------------------------------------

    fn uc8279_begin(&mut self, plane: &Plane, mode: Mode) -> Result<Mode, Error> {
        let use_gc = mode != Mode::Du || !self.baseline_valid || self.force_gc || self.initial_gc_remaining > 0;
        let resolved = if use_gc && mode == Mode::Du { Mode::Gc } else { mode };
        self.command(cmd::PTIN)?;
        if !self.baseline_valid {
            // Seed DTM1 white only when there is no baseline (first paint after init, after a
            // timeout or a grey pass).
            self.fill_plane(cmd::DTM1, 0xFF)?;
            self.command(cmd::DSP)?;
        } else if self.dark_background && !use_gc {
            self.send_plane(cmd::DTM1, plane, true)?;
            self.command(cmd::DSP)?;
        }
        self.send_plane(cmd::DTM2, plane, false)?;
        self.command(cmd::DSP)?;
        self.command_data(cmd::CDI, &[if self.first_refresh { uc8279::CDI_FIRST } else { uc8279::CDI_LATER }])?;
        if use_gc {
            self.load_uc8279_bank(&uc8279::BW_GC)?;
        } else {
            self.load_uc8279_bank(&uc8279::BW_DU)?;
        }
        self.power_on_if_off()?;
        self.command(cmd::DRF)?;
        Ok(resolved)
    }

    fn uc8279_finish(&mut self, plane: &Plane, mode: Mode) -> Result<(), Error> {
        self.command_data(cmd::CDI, &[uc8279::CDI_LATER])?;
        self.send_plane(cmd::DTM1, plane, false)?;
        self.command(cmd::DSP)?;
        self.command(cmd::PTOUT)?;
        self.first_refresh = false;
        if mode != Mode::Du {
            self.initial_gc_remaining = self.initial_gc_remaining.saturating_sub(1);
        }
        Ok(())
    }

    fn uc8279_begin_gray(&mut self, lsb: &Plane, msb: &Plane) -> Result<(), Error> {
        // Stock sequence (FUN_42015108 + FUN_42013be0): PTIN, PTL, DTM1 = A (LSB), DSP,
        // DTM2 = B (MSB), DSP, PTOUT, load XTF_AA, CDI, PON, DRF.
        self.command(cmd::PTIN)?;
        self.command_data(cmd::PTL, &uc8279::FULL_WINDOW)?;
        self.send_plane(cmd::DTM1, lsb, false)?;
        self.command(cmd::DSP)?;
        self.send_plane(cmd::DTM2, msb, false)?;
        self.command(cmd::DSP)?;
        self.command(cmd::PTOUT)?;
        self.load_uc8279_gray_bank()?;
        self.command_data(cmd::CDI, &[if self.first_refresh { uc8279::CDI_FIRST } else { uc8279::CDI_LATER }])?;
        self.power_on_if_off()?;
        self.command(cmd::DRF)?;
        Ok(())
    }
}

/// Nominal duration of a refresh, milliseconds, for UI progress hints. UC8279d timings are not
/// documented; the GC/DU figures are rough field observations from the same class of panel.
pub const fn nominal_refresh_ms(controller: Controller, mode: Mode) -> u32 {
    match controller {
        Controller::Uc8253 => match mode {
            Mode::Du => uc8253::TURBO.nominal_ms(),
            Mode::Gc => uc8253::FULL.nominal_ms() + uc8253::TURBO.nominal_ms(),
            Mode::Half => uc8253::HALF.nominal_ms(),
            Mode::Img => uc8253::IMG.nominal_ms() + uc8253::TURBO.nominal_ms(),
        },
        Controller::Uc8279 => match mode {
            Mode::Du => 400,
            Mode::Gc | Mode::Half | Mode::Img => 900,
        },
    }
}

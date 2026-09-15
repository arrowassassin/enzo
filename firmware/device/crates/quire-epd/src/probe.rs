//! Boot-time controller detection: UC8253 vs UC8279d_B.
//!
//! Protocol and classifier per PapyriX `docs/x3-specifications.md` "Live Probe",
//! `docs/x3-uc8279-driver-reference.md` "Controller Identification" / "Probe Timing" and
//! `lib/EInkDisplay/src/X3DisplayProbeClassifier.cpp` (`runX3DisplayProbe`, `classifyX3Display`).
//!
//! The reads are half-duplex on the panel's single SDA line (our MOSI, GPIO 10), bit-banged at
//! roughly 500 kHz: shift the command out with DC low, release SDA to an input, then sample SDA
//! while SCLK is low and pulse SCLK for each bit. The hardware SPI peripheral cannot do that,
//! so the caller hands us the pins through [`ProbeBus`] and re-claims them for SPI afterwards.
//! SD CS (GPIO 12) must be high throughout — the SD card shares SCLK and MOSI.

use crate::Controller;

/// The GPIO-level access the probe needs. esp-hal's `Flex` pin implements this in a few lines:
/// `mosi_drive` = `set_output_enable(true)` + level, `mosi_release` = `set_output_enable(false)`
/// with the input enabled (internal pull-up on, so a floating line reads `0xFF`).
pub trait ProbeBus {
    /// Drive RST (active low).
    fn rst(&mut self, high: bool);
    /// Drive the panel CS (active low).
    fn cs(&mut self, high: bool);
    /// Drive DC (low = command, high = data).
    fn dc(&mut self, high: bool);
    /// Drive SCLK.
    fn sclk(&mut self, high: bool);
    /// Make MOSI/SDA an output (if it is not already) and drive it to `high`.
    fn mosi_drive(&mut self, high: bool);
    /// Release MOSI/SDA to an input with the pull-up enabled.
    fn mosi_release(&mut self);
    /// Sample MOSI/SDA (only meaningful after [`ProbeBus::mosi_release`]).
    fn mosi_read(&mut self) -> bool;
    /// Busy-wait for `us` microseconds.
    fn delay_us(&mut self, us: u32);
    /// Busy-wait for `ms` milliseconds.
    fn delay_ms(&mut self, ms: u32) {
        self.delay_us(ms.saturating_mul(1000));
    }
}

/// Half period of the bit-bang clock, microseconds (~500 kHz).
pub const HALF_PERIOD_US: u32 = 1;
/// RST high time before the pulse, ms.
pub const RESET_PRE_HIGH_MS: u32 = 2;
/// Settle after RST goes high again, ms.
pub const RESET_SETTLE_MS: u32 = 30;
/// Short reset pulse (screening pass), ms.
pub const RESET_LOW_SHORT_MS: u32 = 1;
/// Long reset pulse (vendor identification timing), ms.
pub const RESET_LOW_LONG_MS: u32 = 50;
/// Pause between passes, ms (`transport.pause(2)`).
pub const PASS_PAUSE_MS: u32 = 2;
/// Bytes of the MTP dump the classifier looks at.
pub const MTP_SIZE: usize = 48;
/// MTP byte 0 of a factory-programmed module.
pub const MTP_REFRESH_KEY: u8 = 0xA5;

/// Register read commands.
pub const CMD_VER: u8 = 0x70;
/// FLG — 1 status byte, bit 0 = BUSY_N (1 = idle); datasheet idle default 0x13.
pub const CMD_FLG: u8 = 0x71;
/// RMTP — 1 dummy byte then MTP[0..].
pub const CMD_RMTP: u8 = 0xA2;

/// One read pass: FLG then VER.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeSample {
    /// FLG (0x71) status byte.
    pub flg: u8,
    /// VER (0x70): reserved, CHIP_VER, LUT_VER[23:0].
    pub ver: [u8; 5],
}

/// The classifier's decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// UC8279d_B confirmed (structured VER, or the field-module MTP signature).
    Uc8279Confirmed,
    /// UC8253 (floating bus with a stable idle shape).
    Uc8253StableDefault,
    /// Neither shape matched; the caller should default to UC8253 and not cache the result.
    Inconclusive,
}

/// Raw samples plus the verdict — everything the developer screen shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeResult {
    /// First pass (1 ms reset; repeated with 50 ms if not structured).
    pub pass1: ProbeSample,
    /// Second pass after an independent reset.
    pub pass2: ProbeSample,
    /// First 48 bytes of the RMTP dump (valid only when `mtp_valid`).
    pub mtp: [u8; MTP_SIZE],
    /// RMTP was read (pass 1 showed a driven idle FLG).
    pub mtp_valid: bool,
    /// A second RMTP read returned the same 48 bytes.
    pub mtp_repeatable: bool,
    /// Classification.
    pub verdict: Verdict,
}

impl ProbeResult {
    /// The controller to drive: an inconclusive probe defaults to UC8253 (x3-specifications.md
    /// "Controller Detection" step 4).
    pub const fn controller(&self) -> Controller {
        match self.verdict {
            Verdict::Uc8279Confirmed => Controller::Uc8279,
            Verdict::Uc8253StableDefault | Verdict::Inconclusive => Controller::Uc8253,
        }
    }

    /// True when the result is worth caching across boots (conclusive).
    pub const fn conclusive(&self) -> bool {
        !matches!(self.verdict, Verdict::Inconclusive)
    }
}

fn uniform(bytes: &[u8]) -> bool {
    bytes.iter().all(|&b| b == bytes[0])
}

/// A FLG the controller is driving and that reports idle (bit 0 set), i.e. not a floating line.
pub fn driven_idle(flg: u8) -> bool {
    flg != 0x00 && flg != 0xFF && (flg & 0x01) != 0
}

fn structured(s: &ProbeSample) -> bool {
    driven_idle(s.flg) && !uniform(&s.ver)
}

/// Field UC8279d shape: equal passes, driven idle FLG, VER uniform 0xFF.
fn field_shape(r: &ProbeResult) -> bool {
    r.pass1.ver == r.pass2.ver && driven_idle(r.pass1.flg) && uniform(&r.pass1.ver) && r.pass1.ver[0] == 0xFF
}

fn mtp_confirms_uc8279(r: &ProbeResult) -> bool {
    if !r.mtp_valid {
        return false;
    }
    if r.mtp[0] == MTP_REFRESH_KEY {
        return true;
    }
    !uniform(&r.mtp) && r.mtp_repeatable
}

/// Pure classifier (`classifyX3Display`), exposed so the shapes can be unit-tested and so a
/// cached raw result can be re-classified.
pub fn classify(r: &ProbeResult) -> Verdict {
    let same = r.pass1.ver == r.pass2.ver;
    if same && structured(&r.pass1) && structured(&r.pass2) {
        return Verdict::Uc8279Confirmed;
    }
    if field_shape(r) && mtp_confirms_uc8279(r) {
        return Verdict::Uc8279Confirmed;
    }
    let floating_mtp = same && driven_idle(r.pass1.flg) && uniform(&r.pass1.ver) && r.mtp_valid && uniform(&r.mtp);
    if floating_mtp {
        return Verdict::Uc8253StableDefault;
    }
    let floating_flg = r.pass1.flg == 0x00 || r.pass1.flg == 0xFF;
    let stable_default = same && r.pass1.flg == r.pass2.flg && floating_flg && uniform(&r.pass1.ver);
    if stable_default {
        Verdict::Uc8253StableDefault
    } else {
        Verdict::Inconclusive
    }
}

/// Reset pulse: RST high 2 ms, low `low_ms`, high, 30 ms settle. No BUSY wait — its polarity
/// depends on the controller under test.
fn reset_pulse<B: ProbeBus>(bus: &mut B, low_ms: u32) {
    bus.rst(true);
    bus.delay_ms(RESET_PRE_HIGH_MS);
    bus.rst(false);
    bus.delay_ms(low_ms);
    bus.rst(true);
    bus.delay_ms(RESET_SETTLE_MS);
}

fn shift_out<B: ProbeBus>(bus: &mut B, byte: u8) {
    for bit in 0..8 {
        bus.mosi_drive(byte & (0x80 >> bit) != 0);
        bus.delay_us(HALF_PERIOD_US);
        bus.sclk(true);
        bus.delay_us(HALF_PERIOD_US);
        bus.sclk(false);
    }
}

fn shift_in<B: ProbeBus>(bus: &mut B) -> u8 {
    let mut byte = 0u8;
    for _ in 0..8 {
        bus.delay_us(HALF_PERIOD_US);
        // Sample while SCLK is low, then pulse.
        byte = (byte << 1) | u8::from(bus.mosi_read());
        bus.sclk(true);
        bus.delay_us(HALF_PERIOD_US);
        bus.sclk(false);
    }
    byte
}

/// Read `out.len()` bytes of register `cmd`, half-duplex.
pub fn read_register<B: ProbeBus>(bus: &mut B, cmd: u8, out: &mut [u8]) {
    bus.sclk(false);
    bus.dc(false);
    bus.cs(false);
    shift_out(bus, cmd);
    bus.dc(true);
    bus.mosi_release();
    bus.delay_us(HALF_PERIOD_US);
    for b in out.iter_mut() {
        *b = shift_in(bus);
    }
    bus.cs(true);
    bus.mosi_drive(false);
}

fn read_pass<B: ProbeBus>(bus: &mut B, reset_low_ms: u32) -> ProbeSample {
    reset_pulse(bus, reset_low_ms);
    let mut s = ProbeSample::default();
    let mut flg = [0u8; 1];
    read_register(bus, CMD_FLG, &mut flg);
    s.flg = flg[0];
    read_register(bus, CMD_VER, &mut s.ver);
    s
}

fn read_mtp<B: ProbeBus>(bus: &mut B) -> [u8; MTP_SIZE] {
    // One dummy byte, then MTP[0..n].
    let mut raw = [0u8; MTP_SIZE + 1];
    read_register(bus, CMD_RMTP, &mut raw);
    let mut mtp = [0u8; MTP_SIZE];
    mtp.copy_from_slice(&raw[1..]);
    mtp
}

/// Run the full detection sequence (`runX3DisplayProbe`): at most three read sequences plus up to
/// two RMTP dumps. Leaves RST high, CS high, SCLK low, MOSI driven low; the caller then hands the
/// pins to the SPI peripheral.
pub fn probe<B: ProbeBus>(bus: &mut B) -> ProbeResult {
    let mut r = ProbeResult {
        pass1: ProbeSample::default(),
        pass2: ProbeSample::default(),
        mtp: [0; MTP_SIZE],
        mtp_valid: false,
        mtp_repeatable: false,
        verdict: Verdict::Inconclusive,
    };
    bus.cs(true);
    bus.sclk(false);
    bus.mosi_drive(false);

    r.pass1 = read_pass(bus, RESET_LOW_SHORT_MS);
    if !structured(&r.pass1) {
        bus.delay_ms(PASS_PAUSE_MS);
        r.pass1 = read_pass(bus, RESET_LOW_LONG_MS);
    }
    bus.delay_ms(PASS_PAUSE_MS);
    r.pass2 = read_pass(bus, if structured(&r.pass1) { RESET_LOW_LONG_MS } else { RESET_LOW_SHORT_MS });

    if driven_idle(r.pass1.flg) {
        r.mtp = read_mtp(bus);
        r.mtp_valid = true;
        if field_shape(&r) && r.mtp[0] != MTP_REFRESH_KEY && !uniform(&r.mtp) {
            let second = read_mtp(bus);
            r.mtp_repeatable = second == r.mtp;
        }
    }

    bus.cs(true);
    bus.sclk(false);
    bus.mosi_drive(false);
    r.verdict = classify(&r);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(p1: ProbeSample, p2: ProbeSample, mtp: Option<([u8; MTP_SIZE], bool)>) -> ProbeResult {
        let (mtp, valid, rep) = match mtp {
            Some((m, rep)) => (m, true, rep),
            None => ([0; MTP_SIZE], false, false),
        };
        ProbeResult { pass1: p1, pass2: p2, mtp, mtp_valid: valid, mtp_repeatable: rep, verdict: Verdict::Inconclusive }
    }

    #[test]
    fn structured_ver_confirms_uc8279() {
        let s = ProbeSample { flg: 0x13, ver: [0x00, 0x03, 0x00, 0x00, 0x66] };
        assert_eq!(classify(&result(s, s, None)), Verdict::Uc8279Confirmed);
        // Different VER between passes → not structured-stable.
        let s2 = ProbeSample { flg: 0x13, ver: [0x00, 0x03, 0x00, 0x00, 0x67] };
        assert_eq!(classify(&result(s, s2, None)), Verdict::Inconclusive);
    }

    #[test]
    fn field_module_blank_mtp_confirms_uc8279_only_when_repeatable() {
        let s = ProbeSample { flg: 0x13, ver: [0xFF; 5] };
        let mut mtp = [0u8; MTP_SIZE];
        mtp[0x1A] = 0x66;
        assert_eq!(classify(&result(s, s, Some((mtp, true)))), Verdict::Uc8279Confirmed);
        assert_eq!(classify(&result(s, s, Some((mtp, false)))), Verdict::Inconclusive);
        let mut keyed = [0u8; MTP_SIZE];
        keyed[0] = MTP_REFRESH_KEY;
        assert_eq!(classify(&result(s, s, Some((keyed, false)))), Verdict::Uc8279Confirmed);
    }

    #[test]
    fn uc8253_shapes() {
        // Floating FLG (0xFF), uniform VER, equal passes.
        let s = ProbeSample { flg: 0xFF, ver: [0xFF; 5] };
        assert_eq!(classify(&result(s, s, None)), Verdict::Uc8253StableDefault);
        let z = ProbeSample { flg: 0x00, ver: [0x00; 5] };
        assert_eq!(classify(&result(z, z, None)), Verdict::Uc8253StableDefault);
        // Driven idle FLG but the RMTP line floats (uniform dump).
        let d = ProbeSample { flg: 0x13, ver: [0xFF; 5] };
        assert_eq!(classify(&result(d, d, Some(([0xFF; MTP_SIZE], false)))), Verdict::Uc8253StableDefault);
        // Different FLG between passes → inconclusive.
        let a = ProbeSample { flg: 0x00, ver: [0xFF; 5] };
        let b = ProbeSample { flg: 0xFF, ver: [0xFF; 5] };
        assert_eq!(classify(&result(a, b, None)), Verdict::Inconclusive);
    }

    #[test]
    fn inconclusive_defaults_to_uc8253_and_is_not_cacheable() {
        let s = ProbeSample { flg: 0x12, ver: [0xFF; 5] }; // bit 0 clear: not driven idle
        let r = result(s, s, None);
        let mut r2 = r;
        r2.verdict = classify(&r);
        assert_eq!(r2.verdict, Verdict::Inconclusive);
        assert_eq!(r2.controller(), Controller::Uc8253);
        assert!(!r2.conclusive());
    }
}

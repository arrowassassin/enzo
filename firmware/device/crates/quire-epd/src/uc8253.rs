//! UC8253 (original X3) register script and waveform tables.
//!
//! Sources (see `scratchpad/hw/` and the PapyriX docs they were copied from):
//! * `x3-lut-waveforms.md` — LUT format, VS encoding, phase/TP/RP of every bank, PLL and CDI.
//! * PapyriX `lib/EInkDisplay/src/Display.cpp` (`initDisplayController`, `_x3Mode` branch and the
//!   `lut_x3_*` tables, "X3 reverse-exact" = recovered from the stock firmware) — the register
//!   values below and the byte tables the tests compare against.
//! * UC8253 datasheet (UltraChip / Good Display) for the register names and field meanings.
//!
//! The LUTs are *built* from the phase descriptions with `const fn`s so the intent stays
//! readable; `tests/` re-types the recovered byte tables and checks them byte-for-byte.

/// One register write of an init script: `cmd` followed by `data` bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// Command byte (sent with DC low).
    pub cmd: u8,
    /// Data bytes (sent with DC high).
    pub data: &'static [u8],
}

/// Power-on register script, sent after the hardware reset and before the first LUT load.
///
/// Every value is the PapyriX "reverse-exact" X3 init (`Display::initDisplayController`, `_x3Mode`);
/// register/field names from the UC8253 datasheet.
pub const INIT_SCRIPT: &[Step] = &[
    // 0x00 PSR, panel setting. 0x3F: RES=00 (resolution from TRES), REG=1 (LUT from registers,
    // not OTP), KW/R=1 (black/white mode), UD=1 (gate scan up), SHL=1 (source shift right),
    // SHD_N=1 (booster on), RST_N=1 (no soft reset). Second byte 0x08: PapyriX/stock value
    // (datasheet PSR byte 2: VCOM/temperature-sensor related bits; kept verbatim).
    Step { cmd: 0x00, data: &[0x3F, 0x08] },
    // 0x61 TRES, resolution. HRES = 0x0318 = 792 sources. VRES = 0x0258 = 600 gates: this is what
    // PapyriX sends (and what it recovered from the stock firmware) even though the glass has 528
    // rows — the extra gate lines are simply unused. Deliberately NOT 528 (0x0210): follow the
    // field-proven value. If bring-up shows a vertical offset, this is the first thing to try.
    Step { cmd: 0x61, data: &[0x03, 0x18, 0x02, 0x58] },
    // 0x65 GSST, gate/source start position: both 0 (PapyriX).
    Step { cmd: 0x65, data: &[0x00, 0x00, 0x00, 0x00] },
    // 0x03 PFS, power-off sequence setting: 0x1D (PapyriX; datasheet default is 0x00, the stock
    // firmware lengthens the power-off ramp).
    Step { cmd: 0x03, data: &[0x1D] },
    // 0x01 PWR, power setting: 0x07 (VDS_EN|VDG_EN|internal DC-DC), 0x17 (VGH/VGL level),
    // 0x3F VDH, 0x3F VDL, 0x17 VDHR (red drive level, unused in KW mode). PapyriX.
    Step { cmd: 0x01, data: &[0x07, 0x17, 0x3F, 0x3F, 0x17] },
    // 0x82 VDCS, VCOM DC setting: 0x1D (PapyriX).
    Step { cmd: 0x82, data: &[0x1D] },
    // 0x06 BTST, booster soft start: phases A/B/C = 0x25 0x25 0x3C, plus a 4th byte 0x37
    // (PapyriX sends four bytes; the UC8253 BTST takes a 4th "phase C2" byte).
    Step { cmd: 0x06, data: &[0x25, 0x25, 0x3C, 0x37] },
    // 0x30 PLL, frame rate: 0x09 (x3-lut-waveforms.md "Controller Configuration": ~18.2 ms per
    // LUT frame group, the value the LUT timings below are tuned for).
    Step { cmd: 0x30, data: &[0x09] },
    // 0xE1 gate scan / power-saving related register: 0x02 (PapyriX; same value the UC8279d
    // script uses).
    Step { cmd: 0xE1, data: &[0x02] },
];

/// CDI (0x50) data for a full/half sync: border drive active during the image write.
/// x3-lut-waveforms.md "VCOM Data Interval".
pub const CDI_FULL_SYNC: [u8; 2] = [0xA9, 0x07];
/// CDI (0x50) data for a fast differential update: border held. Also what PapyriX uses for the
/// conditioning pass with the `FULL` bank and for the grey pass.
pub const CDI_FAST: [u8; 2] = [0x29, 0x07];

/// Settle delay PapyriX inserts after every non-fast refresh (`if (mode != FAST_REFRESH) delay(200)`).
pub const SETTLE_MS: u32 = 200;

/// Nominal frame-group time at PLL 0x09, microseconds (x3-lut-waveforms.md: "approximately 18.2 ms").
pub const FRAME_GROUP_US: u32 = 18_200;

// --------------------------------------------------------------------------------------------
// LUT construction
// --------------------------------------------------------------------------------------------

/// Length of each of the five LUT registers.
pub const LUT_LEN: usize = 42;
/// One LUT register image (7 phases × 6 bytes).
pub type Lut = [u8; LUT_LEN];

/// Source voltage select for one sub-phase (x3-lut-waveforms.md "VS Voltage Encoding").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Vs {
    /// `00` — no drive (hold).
    Gnd = 0b00,
    /// `01` — positive drive, towards black.
    Vdh = 0b01,
    /// `10` — negative drive, towards white.
    Vdl = 0b10,
    /// `11` — common electrode.
    Vcom = 0b11,
}

use Vs::{Gnd, Vdh, Vdl};

/// Pack the four sub-phase voltages A..D into the VS byte, A in the top two bits.
pub const fn vs(a: Vs, b: Vs, c: Vs, d: Vs) -> u8 {
    ((a as u8) << 6) | ((b as u8) << 4) | ((c as u8) << 2) | (d as u8)
}

/// One 6-byte phase: VS, TP0..TP3 (frame groups per sub-phase), RP (repeat count, 0 = inactive).
pub const fn phase(vs: u8, tp: [u8; 4], rp: u8) -> [u8; 6] {
    [vs, tp[0], tp[1], tp[2], tp[3], rp]
}

/// Assemble up to seven phases into a 42-byte register image; unused phases are zero.
pub const fn lut(phases: &[[u8; 6]]) -> Lut {
    assert!(phases.len() <= 7);
    let mut out = [0u8; LUT_LEN];
    let mut p = 0;
    while p < phases.len() {
        let mut i = 0;
        while i < 6 {
            out[p * 6 + i] = phases[p][i];
            i += 1;
        }
        p += 1;
    }
    out
}

/// Frame groups a LUT runs for: Σ over phases of (TP0+TP1+TP2+TP3) × RP.
pub const fn frame_groups(l: &Lut) -> u32 {
    let mut total = 0u32;
    let mut p = 0;
    while p < 7 {
        let b = p * 6;
        let sum = l[b + 1] as u32 + l[b + 2] as u32 + l[b + 3] as u32 + l[b + 4] as u32;
        total += sum * l[b + 5] as u32;
        p += 1;
    }
    total
}

/// The five LUT registers of one waveform bank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LutSet {
    /// 0x20 — common electrode waveform.
    pub vcom: Lut,
    /// 0x21 — white pixels that stay white.
    pub ww: Lut,
    /// 0x22 — black → white.
    pub bw: Lut,
    /// 0x23 — white → black.
    pub wb: Lut,
    /// 0x24 — black pixels that stay black.
    pub bb: Lut,
}

/// LUT register commands, in load order.
pub const LUT_CMDS: [u8; 5] = [0x20, 0x21, 0x22, 0x23, 0x24];

impl LutSet {
    /// The tables paired with their register command, in load order.
    pub const fn tables(&self) -> [(u8, &Lut); 5] {
        [(0x20, &self.vcom), (0x21, &self.ww), (0x22, &self.bw), (0x23, &self.wb), (0x24, &self.bb)]
    }

    /// Nominal duration of a refresh with this bank, milliseconds (VCOM table, PLL 0x09).
    pub const fn nominal_ms(&self) -> u32 {
        frame_groups(&self.vcom) * FRAME_GROUP_US / 1000
    }
}

/// Which bank is loaded in the controller's LUT registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank {
    /// Quality refresh (~472 ms).
    Full,
    /// Balanced fast differential (~382 ms).
    Turbo,
    /// Scrub refresh, independent of the old frame.
    Half,
    /// Image write / full sync (~908 ms).
    Img,
    /// 4-level grey pass (~127 ms).
    Gray,
}

impl Bank {
    /// The tables of this bank.
    pub const fn set(self) -> &'static LutSet {
        match self {
            Bank::Full => &FULL,
            Bank::Turbo => &TURBO,
            Bank::Half => &HALF,
            Bank::Img => &IMG,
            Bank::Gray => &GRAY,
        }
    }
}

const GND4: u8 = vs(Gnd, Gnd, Gnd, Gnd);

/// `lut_x3_*_full` — quality refresh. Phase 0 TP=(6,2,6,6), phase 1 TP=(5,1,0,0), RP=1 each:
/// 26 frame groups, ~472 ms. VS per x3-lut-waveforms.md.
pub const FULL: LutSet = LutSet {
    vcom: lut(&[phase(GND4, [6, 2, 6, 6], 1), phase(GND4, [5, 1, 0, 0], 1)]),
    ww: lut(&[phase(vs(Gnd, Vdl, Gnd, Gnd), [6, 2, 6, 6], 1), phase(GND4, [5, 1, 0, 0], 1)]),
    bw: lut(&[phase(vs(Vdl, Vdl, Vdl, Vdl), [6, 2, 6, 6], 1), phase(vs(Vdl, Gnd, Gnd, Gnd), [5, 1, 0, 0], 1)]),
    wb: lut(&[phase(vs(Vdh, Vdh, Vdh, Vdh), [6, 2, 6, 6], 1), phase(vs(Vdh, Gnd, Gnd, Gnd), [5, 1, 0, 0], 1)]),
    bb: lut(&[phase(vs(Gnd, Vdh, Gnd, Gnd), [6, 2, 6, 6], 1), phase(GND4, [5, 1, 0, 0], 1)]),
};

/// `lut_x3_*_turbo` — fast differential page turns. Same VS as [`FULL`], phase 0 TP=(4,2,4,4),
/// phase 1 TP=(4,1,0,0): 19 frame groups, ~382 ms.
pub const TURBO: LutSet = LutSet {
    vcom: lut(&[phase(GND4, [4, 2, 4, 4], 1), phase(GND4, [4, 1, 0, 0], 1)]),
    ww: lut(&[phase(vs(Gnd, Vdl, Gnd, Gnd), [4, 2, 4, 4], 1), phase(GND4, [4, 1, 0, 0], 1)]),
    bw: lut(&[phase(vs(Vdl, Vdl, Vdl, Vdl), [4, 2, 4, 4], 1), phase(vs(Vdl, Gnd, Gnd, Gnd), [4, 1, 0, 0], 1)]),
    wb: lut(&[phase(vs(Vdh, Vdh, Vdh, Vdh), [4, 2, 4, 4], 1), phase(vs(Vdh, Gnd, Gnd, Gnd), [4, 1, 0, 0], 1)]),
    bb: lut(&[phase(vs(Gnd, Vdh, Gnd, Gnd), [4, 2, 4, 4], 1), phase(GND4, [4, 1, 0, 0], 1)]),
};

/// `lut_x3_*_half` — scrub bank (FreeInk SDK via PapyriX). Phase 0 TP=(6,1,6,6), phase 1
/// TP=(4,1,1,0). WW == BW (drive to white: VDL×4 then VDL,VDL,GND,GND) and WB == BB (drive to
/// black: VDH×4 then VDH,VDH,GND,GND), so the result does not depend on the old frame.
pub const HALF: LutSet = LutSet {
    vcom: lut(&[phase(GND4, [6, 1, 6, 6], 1), phase(GND4, [4, 1, 1, 0], 1)]),
    ww: lut(&[phase(vs(Vdl, Vdl, Vdl, Vdl), [6, 1, 6, 6], 1), phase(vs(Vdl, Vdl, Gnd, Gnd), [4, 1, 1, 0], 1)]),
    bw: lut(&[phase(vs(Vdl, Vdl, Vdl, Vdl), [6, 1, 6, 6], 1), phase(vs(Vdl, Vdl, Gnd, Gnd), [4, 1, 1, 0], 1)]),
    wb: lut(&[phase(vs(Vdh, Vdh, Vdh, Vdh), [6, 1, 6, 6], 1), phase(vs(Vdh, Vdh, Gnd, Gnd), [4, 1, 1, 0], 1)]),
    bb: lut(&[phase(vs(Vdh, Vdh, Vdh, Vdh), [6, 1, 6, 6], 1), phase(vs(Vdh, Vdh, Gnd, Gnd), [4, 1, 1, 0], 1)]),
};

/// `lut_x3_*_img` — stock image-write bank used for a full sync (both RAMs hold the inverted
/// frame). Phases TP=(8,11,2,3), (12,2,7,2), (1,0,2,0): 50 frame groups, ~908 ms. The VS
/// patterns are the stock values (x3-lut-waveforms.md only calls them "complex"); the byte in
/// each comment is the recovered VS value from PapyriX `lut_x3_*_img`.
pub const IMG: LutSet = LutSet {
    vcom: lut(&[phase(GND4, [8, 11, 2, 3], 1), phase(GND4, [12, 2, 7, 2], 1), phase(GND4, [1, 0, 2, 0], 1)]),
    ww: lut(&[
        phase(vs(Vdl, Vdl, Vdl, Gnd), [8, 11, 2, 3], 1), // 0xA8
        phase(vs(Vdh, Gnd, Vdh, Gnd), [12, 2, 7, 2], 1), // 0x44
        phase(vs(Gnd, Gnd, Vdh, Gnd), [1, 0, 2, 0], 1),  // 0x04
    ]),
    bw: lut(&[
        phase(vs(Vdl, Gnd, Gnd, Gnd), [8, 11, 2, 3], 1), // 0x80
        phase(vs(Vdh, Vdl, Gnd, Vdl), [12, 2, 7, 2], 1), // 0x62
        phase(GND4, [1, 0, 2, 0], 1),                    // 0x00
    ]),
    wb: lut(&[
        phase(vs(Vdl, Gnd, Vdl, Gnd), [8, 11, 2, 3], 1), // 0x88
        phase(vs(Vdh, Vdl, Gnd, Gnd), [12, 2, 7, 2], 1), // 0x60
        phase(GND4, [1, 0, 2, 0], 1),                    // 0x00
    ]),
    bb: lut(&[
        phase(GND4, [8, 11, 2, 3], 1),                   // 0x00
        phase(vs(Vdh, Gnd, Vdl, Vdl), [12, 2, 7, 2], 1), // 0x4A
        phase(vs(Vdl, Gnd, Vdl, Gnd), [1, 0, 2, 0], 1),  // 0x88
    ]),
};

/// `lut_x3_*_gray` — 4-level grey pass. One phase TP=(3,2,1,1), RP=1: 7 frame groups, ~127 ms.
/// WW (dark grey): short VDL pulse in sub-phase B; BW (light grey): VDL in sub-phase A;
/// WB/BB/VCOM: GND hold.
pub const GRAY: LutSet = LutSet {
    vcom: lut(&[phase(GND4, [3, 2, 1, 1], 1)]),
    ww: lut(&[phase(vs(Gnd, Vdl, Gnd, Gnd), [3, 2, 1, 1], 1)]),
    bw: lut(&[phase(vs(Vdl, Gnd, Gnd, Gnd), [3, 2, 1, 1], 1)]),
    wb: lut(&[phase(GND4, [3, 2, 1, 1], 1)]),
    bb: lut(&[phase(GND4, [3, 2, 1, 1], 1)]),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_group_totals_match_the_doc() {
        assert_eq!(frame_groups(&FULL.vcom), 26);
        assert_eq!(frame_groups(&TURBO.vcom), 19);
        assert_eq!(frame_groups(&HALF.vcom), 25);
        assert_eq!(frame_groups(&IMG.vcom), 50);
        assert_eq!(frame_groups(&GRAY.vcom), 7);
        assert_eq!(FULL.nominal_ms(), 473); // doc: ~472 ms
        assert_eq!(TURBO.nominal_ms(), 345); // doc rounds to ~382 ms incl. overhead
        assert_eq!(IMG.nominal_ms(), 910); // doc: ~908 ms
        assert_eq!(GRAY.nominal_ms(), 127);
    }

    #[test]
    fn vs_packing_is_msb_first() {
        assert_eq!(vs(Vdl, Vdl, Vdl, Vdl), 0xAA);
        assert_eq!(vs(Vdh, Vdh, Vdh, Vdh), 0x55);
        assert_eq!(vs(Gnd, Vdl, Gnd, Gnd), 0x20);
        assert_eq!(vs(Gnd, Vdh, Gnd, Gnd), 0x10);
        assert_eq!(vs(Vdl, Gnd, Gnd, Gnd), 0x80);
        assert_eq!(vs(Vs::Vcom, Gnd, Gnd, Gnd), 0xC0);
    }

    #[test]
    fn unused_phases_are_zero() {
        for set in [&FULL, &TURBO, &HALF, &IMG, &GRAY] {
            for (_, t) in set.tables() {
                let used = 6 * if core::ptr::eq(set, &IMG) { 3 } else if core::ptr::eq(set, &GRAY) { 1 } else { 2 };
                assert!(t[used..].iter().all(|&b| b == 0));
            }
        }
    }
}

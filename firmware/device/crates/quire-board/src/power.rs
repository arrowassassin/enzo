//! Resume state across sleep and resets (RTC fast memory), the crash counter that
//! opens safe mode, and the SD rail hold for deep sleep.

use core::sync::atomic::AtomicU32;

use esp_hal::ram;

/// Magic marking a valid resume block.
const MAGIC: u32 = 0x5155_4952; // "QUIR"

/// What survives a sleep or reset: where the reader was, and how many crashes in a row.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Resume {
    /// Valid when equal to `MAGIC`.
    pub magic: u32,
    /// Open book id (0 = none).
    pub book: u64,
    /// Characters into the book.
    pub chars: u32,
    /// Consecutive crashes since the last clean run.
    pub crashes: u32,
    /// Local time when the block was written (to age the crash counter).
    pub written_at: u32,
    /// Local time at the last clean shutdown, so the clock survives a dead card.
    pub clock: u32,
    /// Simple checksum of the fields above.
    pub check: u32,
}

impl Resume {
    fn checksum(&self) -> u32 {
        self.magic
            .wrapping_mul(31)
            .wrapping_add(self.book as u32)
            .wrapping_add((self.book >> 32) as u32)
            .wrapping_add(self.chars.wrapping_mul(7))
            .wrapping_add(self.crashes.wrapping_mul(13))
            .wrapping_add(self.written_at.wrapping_mul(17))
            .wrapping_add(self.clock.wrapping_mul(19))
    }
}

#[ram(unstable(rtc_fast, persistent))]
static mut RESUME: [u32; 8] = [0; 8];

fn unpack(w: &[u32; 8]) -> Resume {
    Resume {
        magic: w[0],
        book: (w[1] as u64) | ((w[2] as u64) << 32),
        chars: w[3],
        crashes: w[4],
        written_at: w[5],
        clock: w[6],
        check: w[7],
    }
}

fn pack(r: &Resume) -> [u32; 8] {
    [r.magic, r.book as u32, (r.book >> 32) as u32, r.chars, r.crashes, r.written_at, r.clock, r.check]
}

/// Read the resume block; `None` when it was never written or is corrupt.
pub fn load() -> Option<Resume> {
    // SAFETY: single-threaded access from the main task; the block is plain data.
    let words = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(RESUME)) };
    let r = unpack(&words);
    (r.magic == MAGIC && r.check == r.checksum()).then_some(r)
}

/// Write the resume block.
pub fn store(mut r: Resume) {
    r.magic = MAGIC;
    r.check = r.checksum();
    // SAFETY: as above.
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(RESUME), pack(&r)) };
}

/// Crashes in a row that open safe mode.
pub const SAFE_MODE_CRASHES: u32 = 3;

/// Count a boot that was not a clean shutdown or a wake from sleep; returns the new count.
pub fn note_boot(clean: bool, now: u32) -> u32 {
    let mut r = load().unwrap_or_default();
    if clean {
        r.crashes = 0;
    } else if now.saturating_sub(r.written_at) < 120 {
        r.crashes += 1;
    } else {
        r.crashes = 1;
    }
    r.written_at = now;
    let c = r.crashes;
    store(r);
    c
}

/// Milliseconds since boot, kept by the main loop for code without HAL access.
pub static UPTIME_MS: AtomicU32 = AtomicU32::new(0);

/// Hold GPIO13 (the SD rail) at its current level through deep sleep. The digital pad
/// hold register keeps the pad driven while the core is off, and the pad is released
/// again by [`release_holds`] after a wake.
pub fn hold_sd_rail(hold: bool) {
    let rtc = esp_hal::peripherals::LPWR::regs();
    rtc.dig_pad_hold().modify(|r, w| unsafe {
        let bits = r.bits();
        w.bits(if hold { bits | (1 << 13) } else { bits & !(1 << 13) })
    });
}

/// Release every digital pad hold left over from the last deep sleep.
pub fn release_holds() {
    let rtc = esp_hal::peripherals::LPWR::regs();
    rtc.dig_pad_hold().write(|w| unsafe { w.bits(0) });
}

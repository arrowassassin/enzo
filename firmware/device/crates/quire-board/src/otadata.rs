//! The `otadata` partition as the ESP-IDF bootloader reads it: two 32-byte selection
//! entries, one at the start of each 4 KB sector, each carrying a sequence number, an
//! image state and a CRC of the sequence number. The entry with the highest bootable
//! sequence number wins and `(seq - 1) % slots` names the `ota_N` partition to boot.
//!
//! Pure logic over byte slices, mirroring `bootloader_common.c` and `esp_ota_ops.c`:
//! the device code in [`crate::ota`] only reads and writes the bytes.

/// Length of one selection entry.
pub const ENTRY_LEN: usize = 32;
/// Sequence number of an erased entry.
pub const SEQ_NONE: u32 = 0xFFFF_FFFF;
/// Number of `ota_N` app partitions in `partitions.csv`.
pub const SLOT_COUNT: u32 = 2;

/// The OTA app slots of the partition table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppSlot {
    /// `ota_0`, "slot A".
    Ota0,
    /// `ota_1`, "slot B".
    Ota1,
}

impl AppSlot {
    /// Slot by index (0 or 1).
    pub const fn from_index(i: u32) -> Option<AppSlot> {
        match i {
            0 => Some(AppSlot::Ota0),
            1 => Some(AppSlot::Ota1),
            _ => None,
        }
    }
    /// 0 for `ota_0`, 1 for `ota_1`.
    pub const fn index(self) -> u32 {
        match self {
            AppSlot::Ota0 => 0,
            AppSlot::Ota1 => 1,
        }
    }
    /// The other slot.
    pub const fn other(self) -> AppSlot {
        match self {
            AppSlot::Ota0 => AppSlot::Ota1,
            AppSlot::Ota1 => AppSlot::Ota0,
        }
    }
    /// Partition label in `partitions.csv`.
    pub const fn label(self) -> &'static str {
        match self {
            AppSlot::Ota0 => "ota_0",
            AppSlot::Ota1 => "ota_1",
        }
    }
    /// The letter the screens use ("slot A").
    pub const fn letter(self) -> &'static str {
        match self {
            AppSlot::Ota0 => "A",
            AppSlot::Ota1 => "B",
        }
    }
}

/// Image state of a selected slot (`esp_ota_img_states_t`), with app-rollback semantics:
/// a freshly installed slot is `New`; a rollback-aware bootloader turns that into
/// `PendingVerify` on the first boot and into `Aborted` if the app never confirmed it;
/// the app confirms with `Valid`. `Invalid` and `Aborted` entries are never booted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ImageState {
    /// Just installed, not yet booted.
    New = 0,
    /// Booted once, awaiting the app's confirmation.
    PendingVerify = 1,
    /// Confirmed working.
    Valid = 2,
    /// Marked bad by the app.
    Invalid = 3,
    /// Given up by the bootloader.
    Aborted = 4,
    /// Erased or unknown: booted without rollback tracking.
    Undefined = 0xFFFF_FFFF,
}

impl ImageState {
    /// Decode; unknown values read as `Undefined`, which is how the bootloader treats them.
    pub const fn from_u32(v: u32) -> ImageState {
        match v {
            0 => ImageState::New,
            1 => ImageState::PendingVerify,
            2 => ImageState::Valid,
            3 => ImageState::Invalid,
            4 => ImageState::Aborted,
            _ => ImageState::Undefined,
        }
    }
    /// Short name for the screens.
    pub const fn name(self) -> &'static str {
        match self {
            ImageState::New => "new",
            ImageState::PendingVerify => "pending",
            ImageState::Valid => "valid",
            ImageState::Invalid => "invalid",
            ImageState::Aborted => "aborted",
            ImageState::Undefined => "undefined",
        }
    }
}

/// CRC-32 as the ROM's `crc32_le(UINT32_MAX, data)` computes it for the otadata entries:
/// reflected polynomial 0xEDB88320, initial register 0, final complement.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// One selection entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Sequence number; `SEQ_NONE` when erased.
    pub seq: u32,
    /// Raw image state.
    pub state: u32,
    /// CRC of `seq` as stored.
    pub crc: u32,
}

impl Entry {
    /// Decode the first 32 bytes of an otadata sector.
    pub fn parse(b: &[u8; ENTRY_LEN]) -> Entry {
        let w = |at: usize| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        Entry { seq: w(0), state: w(24), crc: w(28) }
    }

    /// A fresh entry with a correct CRC.
    pub fn new(seq: u32, state: ImageState) -> Entry {
        Entry { seq, state: state as u32, crc: crc32(&seq.to_le_bytes()) }
    }

    /// Encode (the 20-byte label stays erased).
    pub fn to_bytes(self) -> [u8; ENTRY_LEN] {
        let mut b = [0xFFu8; ENTRY_LEN];
        b[0..4].copy_from_slice(&self.seq.to_le_bytes());
        b[24..28].copy_from_slice(&self.state.to_le_bytes());
        b[28..32].copy_from_slice(&self.crc.to_le_bytes());
        b
    }

    /// The decoded state.
    pub const fn image_state(&self) -> ImageState {
        ImageState::from_u32(self.state)
    }

    /// Whether the bootloader would consider this entry (`bootloader_common_ota_select_valid`):
    /// CRC intact, a sequence number, and not marked invalid or aborted.
    pub fn bootable(&self) -> bool {
        self.seq != SEQ_NONE
            && self.crc == crc32(&self.seq.to_le_bytes())
            && !matches!(self.image_state(), ImageState::Invalid | ImageState::Aborted)
    }

    /// Whether the entry carries anything at all (a sequence number with a good CRC).
    pub fn written(&self) -> bool {
        self.seq != SEQ_NONE && self.crc == crc32(&self.seq.to_le_bytes())
    }
}

/// Why the factory (recovery) app is running instead of an OTA slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diagnosis {
    /// No slot is selected: a fresh flash, or the app asked for recovery.
    Empty,
    /// The last update in this slot was abandoned (or marked invalid) and nothing else
    /// was bootable.
    Abandoned(AppSlot),
    /// A slot is selected, so its image failed to load.
    Unbootable(AppSlot),
}

/// Both entries of the partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OtaData {
    /// Entry 0 (sector 0) and entry 1 (sector 1).
    pub entries: [Entry; 2],
}

impl OtaData {
    /// Decode from the first 32 bytes of each sector.
    pub fn parse(sector0: &[u8; ENTRY_LEN], sector1: &[u8; ENTRY_LEN]) -> OtaData {
        OtaData { entries: [Entry::parse(sector0), Entry::parse(sector1)] }
    }

    /// The entry the bootloader acts on: the bootable one with the higher sequence
    /// number (entry 0 on a tie), as `bootloader_common_get_active_otadata`.
    pub fn active(&self) -> Option<usize> {
        let [a, b] = self.entries;
        match (a.bootable(), b.bootable()) {
            (true, true) => Some(if a.seq >= b.seq { 0 } else { 1 }),
            (true, false) => Some(0),
            (false, true) => Some(1),
            (false, false) => None,
        }
    }

    /// The slot the bootloader will try next.
    pub fn selected(&self) -> Option<AppSlot> {
        let e = self.entries[self.active()?];
        AppSlot::from_index((e.seq - 1) % SLOT_COUNT)
    }

    /// State of the selected slot.
    pub fn state(&self) -> Option<ImageState> {
        Some(self.entries[self.active()?].image_state())
    }

    /// Select `slot` for the next boot with `state`, as `esp_rewrite_ota_data` does: the
    /// new sequence number is the smallest one at or above the active one that maps to
    /// `slot`, written into the entry the bootloader is not using. Returns the entry
    /// index to write and its contents.
    pub fn select(&self, slot: AppSlot, state: ImageState) -> (usize, Entry) {
        let target = (slot.index() + 1) % SLOT_COUNT;
        match self.active() {
            Some(active) => {
                let seq = self.entries[active].seq;
                let mut i = 0u32;
                while seq > target + i * SLOT_COUNT {
                    i += 1;
                }
                (active ^ 1, Entry::new(target + i * SLOT_COUNT, state))
            }
            None => (0, Entry::new(slot.index() + 1, state)),
        }
    }

    /// Change the selected slot's state in place. `None` when nothing is selected.
    pub fn set_state(&self, state: ImageState) -> Option<(usize, Entry)> {
        let i = self.active()?;
        let e = self.entries[i];
        Some((i, Entry { state: state as u32, ..e }))
    }

    /// Why the factory app is running (only meaningful when it is).
    pub fn diagnosis(&self) -> Diagnosis {
        if let Some(slot) = self.selected() {
            return Diagnosis::Unbootable(slot);
        }
        // Nothing bootable: was something written and given up on?
        let given_up = self
            .entries
            .iter()
            .filter(|e| e.written() && matches!(e.image_state(), ImageState::Invalid | ImageState::Aborted))
            .max_by_key(|e| e.seq);
        match given_up.and_then(|e| AppSlot::from_index((e.seq - 1) % SLOT_COUNT)) {
            Some(slot) => Diagnosis::Abandoned(slot),
            None => Diagnosis::Empty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERASED: [u8; 32] = [0xFF; 32];

    fn entry(seq: u32, state: u32) -> [u8; 32] {
        Entry { seq, state, crc: crc32(&seq.to_le_bytes()) }.to_bytes()
    }

    #[test]
    fn crc_matches_the_rom() {
        // Vectors from esp-bootloader-esp-idf's tests (ROM crc32_le with an all-ones seed).
        assert_eq!(crc32(&1u32.to_le_bytes()).to_le_bytes(), [154, 152, 67, 71]);
        assert_eq!(crc32(&2u32.to_le_bytes()).to_le_bytes(), [116, 55, 246, 85]);
        assert_eq!(crc32(&3u32.to_le_bytes()).to_le_bytes(), [17, 80, 74, 237]);
    }

    #[test]
    fn entry_round_trips() {
        let e = Entry::new(7, ImageState::Valid);
        let b = e.to_bytes();
        assert_eq!(&b[4..24], &[0xFF; 20]);
        assert_eq!(Entry::parse(&b), e);
        assert!(e.bootable());
        assert!(Entry::parse(&ERASED).seq == SEQ_NONE);
        assert!(!Entry::parse(&ERASED).bootable());
        let mut bad = b;
        bad[28] ^= 1;
        assert!(!Entry::parse(&bad).bootable());
    }

    #[test]
    fn erased_partition_selects_nothing() {
        let d = OtaData::parse(&ERASED, &ERASED);
        assert_eq!(d.active(), None);
        assert_eq!(d.selected(), None);
        assert_eq!(d.state(), None);
        assert_eq!(d.diagnosis(), Diagnosis::Empty);
        assert_eq!(d.set_state(ImageState::Valid), None);
        // First selection lands in entry 0 with seq = slot + 1.
        assert_eq!(d.select(AppSlot::Ota0, ImageState::New), (0, Entry::new(1, ImageState::New)));
        assert_eq!(d.select(AppSlot::Ota1, ImageState::Valid), (0, Entry::new(2, ImageState::Valid)));
    }

    #[test]
    fn highest_bootable_sequence_wins() {
        let d = OtaData::parse(&entry(1, 2), &entry(2, 0));
        assert_eq!(d.active(), Some(1));
        assert_eq!(d.selected(), Some(AppSlot::Ota1));
        assert_eq!(d.state(), Some(ImageState::New));
        let d = OtaData::parse(&entry(3, 2), &entry(2, 2));
        assert_eq!(d.selected(), Some(AppSlot::Ota0));
        // A tie goes to entry 0, as in the bootloader.
        let d = OtaData::parse(&entry(4, 2), &entry(4, 2));
        assert_eq!(d.active(), Some(0));
    }

    #[test]
    fn aborted_and_invalid_entries_are_skipped() {
        let d = OtaData::parse(&entry(1, 2), &entry(2, 4));
        assert_eq!(d.selected(), Some(AppSlot::Ota0));
        assert_eq!(d.state(), Some(ImageState::Valid));
        let d = OtaData::parse(&entry(1, 3), &entry(2, 4));
        assert_eq!(d.selected(), None);
        assert_eq!(d.diagnosis(), Diagnosis::Abandoned(AppSlot::Ota1));
        let d = OtaData::parse(&entry(5, 3), &ERASED);
        assert_eq!(d.diagnosis(), Diagnosis::Abandoned(AppSlot::Ota0));
    }

    #[test]
    fn selecting_the_other_slot_bumps_the_sequence_into_the_other_entry() {
        // seq 1 → ota_0 in entry 0; switching to ota_1 needs the next even seq (2) in entry 1.
        let d = OtaData::parse(&entry(1, 2), &ERASED);
        assert_eq!(d.select(AppSlot::Ota1, ImageState::New), (1, Entry::new(2, ImageState::New)));
        let d = OtaData::parse(&entry(1, 2), &entry(2, 0));
        assert_eq!(d.select(AppSlot::Ota0, ImageState::Valid), (0, Entry::new(3, ImageState::Valid)));
        // Re-selecting the current slot keeps the sequence number.
        assert_eq!(d.select(AppSlot::Ota1, ImageState::Valid), (0, Entry::new(2, ImageState::Valid)));
        // The bootloader agrees with what we wrote.
        let (i, e) = d.select(AppSlot::Ota0, ImageState::Valid);
        let mut n = d;
        n.entries[i] = e;
        assert_eq!(n.selected(), Some(AppSlot::Ota0));
        assert_eq!(n.state(), Some(ImageState::Valid));
    }

    #[test]
    fn set_state_edits_the_active_entry_only() {
        let d = OtaData::parse(&entry(1, 2), &entry(2, 0));
        let (i, e) = d.set_state(ImageState::Valid).unwrap();
        assert_eq!(i, 1);
        assert_eq!(e, Entry::new(2, ImageState::Valid));
        let (i, e) = d.set_state(ImageState::Invalid).unwrap();
        let mut n = d;
        n.entries[i] = e;
        // Invalidating the new slot falls back to the old one.
        assert_eq!(n.selected(), Some(AppSlot::Ota0));
        assert_eq!(n.diagnosis(), Diagnosis::Unbootable(AppSlot::Ota0));
    }

    #[test]
    fn long_histories_keep_alternating() {
        let mut d = OtaData::parse(&ERASED, &ERASED);
        let mut want = AppSlot::Ota0;
        for _ in 0..50 {
            let (i, e) = d.select(want, ImageState::New);
            d.entries[i] = e;
            assert_eq!(d.selected(), Some(want));
            want = want.other();
        }
        assert_eq!(d.entries[0].seq.max(d.entries[1].seq), 50);
    }
}

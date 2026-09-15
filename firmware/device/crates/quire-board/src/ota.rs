//! Firmware updates into the two `ota_N` slots, with ESP-IDF rollback semantics.
//!
//! An install streams an image into the slot that is not running ([`OtaWriter`]), which
//! is erased a 64 KB block at a time as the write reaches it; nothing is selected until
//! the whole image has been read back and verified ([`crate::image`]). The slot is then
//! written into `otadata` as the next boot in state `New`: a rollback-aware bootloader
//! turns that into `PendingVerify` and gives up on it (`Aborted`) if the app does not
//! call [`mark_valid`] before the next reset; every bootloader skips entries that are
//! `Invalid` or `Aborted`, and boots the factory (recovery) app when nothing is left.
//!
//! The pure parts — the image walk and the entry arithmetic — live in [`crate::image`]
//! and [`crate::otadata`]; this module reads and writes the flash.

use alloc::vec::Vec;
use core::fmt;

use embedded_storage::nor_flash::NorFlash;
use embedded_storage::ReadStorage;
use esp_bootloader_esp_idf::partitions::{self, AppPartitionSubType, DataPartitionSubType, PartitionTable, PartitionType};
use esp_storage::FlashStorage;
use quire_fs::{Fs, FsError, ReadAt};

use crate::flash::Flash;
pub use crate::image::{AppDesc, ImageError, ImageInfo};
use crate::image::{Verifier, HEAD_LEN};
pub use crate::otadata::{AppSlot, Diagnosis, ImageState, OtaData};
use crate::otadata::{Entry, ENTRY_LEN};

/// Flash sector: the erase and otadata entry granularity.
pub const SECTOR: u32 = FlashStorage::SECTOR_SIZE;
/// Flash block: the writer erases one of these at a time.
pub const BLOCK: u32 = FlashStorage::BLOCK_SIZE;

/// Card paths an update is looked for at, in order.
pub const CARD_PATHS: [&str; 3] = ["/quire/update.bin", "/quire-x3.bin", "/quire-update.bin"];

/// What can go wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OtaError {
    /// The partition table could not be read or lacks `otadata` or an `ota_N` slot.
    Table,
    /// A flash read, erase or write failed.
    Flash,
    /// The image is not a valid ESP32-C3 firmware image.
    Image(ImageError),
    /// The card file could not be read.
    Fs(FsError),
    /// The file is larger than a slot.
    TooLarge {
        /// File length.
        len: u64,
        /// Slot length.
        capacity: u32,
    },
    /// Nothing is selected in `otadata`.
    NoSlotSelected,
    /// Neither slot holds a verifiable image.
    NoBootableImage,
    /// A written otadata entry read back differently.
    Verify,
}

impl From<ImageError> for OtaError {
    fn from(e: ImageError) -> Self {
        OtaError::Image(e)
    }
}

impl From<FsError> for OtaError {
    fn from(e: FsError) -> Self {
        OtaError::Fs(e)
    }
}

impl fmt::Display for OtaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OtaError::Table => write!(f, "partition table unreadable"),
            OtaError::Flash => write!(f, "flash error"),
            OtaError::Image(e) => write!(f, "{e}"),
            OtaError::Fs(e) => write!(f, "card: {e}"),
            OtaError::TooLarge { len, capacity } => write!(f, "file is {} KB, slot holds {} KB", len / 1024, capacity / 1024),
            OtaError::NoSlotSelected => write!(f, "no firmware selected"),
            OtaError::NoBootableImage => write!(f, "no valid firmware in either slot"),
            OtaError::Verify => write!(f, "otadata write did not stick"),
        }
    }
}

/// Where things are, from the partition table.
#[derive(Clone, Copy, Debug)]
struct Layout {
    otadata: u32,
    /// (offset, length) of `ota_0` and `ota_1`.
    slots: [(u32, u32); 2],
    /// Which OTA slot the running image was mapped from (`None`: the factory app).
    booted: Option<AppSlot>,
}

impl Layout {
    fn slot(&self, slot: AppSlot) -> (u32, u32) {
        self.slots[slot.index() as usize]
    }
}

fn read_layout(f: &mut FlashStorage<'static>) -> Result<Layout, OtaError> {
    let mut table = [0u8; partitions::PARTITION_TABLE_MAX_LEN];
    let pt = partitions::read_partition_table(f, &mut table).map_err(|_| OtaError::Table)?;
    let find = |t: PartitionType| pt.find_partition(t).ok().flatten().ok_or(OtaError::Table);
    let ota = find(PartitionType::Data(DataPartitionSubType::Ota))?;
    if ota.len() < 2 * SECTOR {
        return Err(OtaError::Table);
    }
    let s0 = find(PartitionType::App(AppPartitionSubType::Ota0))?;
    let s1 = find(PartitionType::App(AppPartitionSubType::Ota1))?;
    let booted = booted_of(&pt);
    Ok(Layout { otadata: ota.offset(), slots: [(s0.offset(), s0.len()), (s1.offset(), s1.len())], booted })
}

fn booted_of(pt: &PartitionTable<'_>) -> Option<AppSlot> {
    match pt.booted_partition().ok().flatten()?.partition_type() {
        PartitionType::App(AppPartitionSubType::Ota0) => Some(AppSlot::Ota0),
        PartitionType::App(AppPartitionSubType::Ota1) => Some(AppSlot::Ota1),
        _ => None,
    }
}

fn read_entries(f: &mut FlashStorage<'static>, l: &Layout) -> Result<OtaData, OtaError> {
    let mut a = [0u8; ENTRY_LEN];
    let mut b = [0u8; ENTRY_LEN];
    f.read(l.otadata, &mut a).map_err(|_| OtaError::Flash)?;
    f.read(l.otadata + SECTOR, &mut b).map_err(|_| OtaError::Flash)?;
    Ok(OtaData::parse(&a, &b))
}

/// Erase one otadata sector and write its entry, then read it back.
fn write_entry(f: &mut FlashStorage<'static>, l: &Layout, index: usize, entry: Entry) -> Result<(), OtaError> {
    let off = l.otadata + index as u32 * SECTOR;
    NorFlash::erase(f, off, off + SECTOR).map_err(|_| OtaError::Flash)?;
    let bytes = entry.to_bytes();
    NorFlash::write(f, off, &bytes).map_err(|_| OtaError::Flash)?;
    let mut back = [0u8; ENTRY_LEN];
    f.read(off, &mut back).map_err(|_| OtaError::Flash)?;
    if back != bytes {
        return Err(OtaError::Verify);
    }
    Ok(())
}

/// Both otadata entries as they are.
pub fn otadata(flash: &Flash) -> Result<OtaData, OtaError> {
    let mut f = flash.borrow_mut();
    let l = read_layout(&mut f)?;
    read_entries(&mut f, &l)
}

/// The slot `otadata` selects for the next boot (`None`: the factory app).
pub fn current_slot(flash: &Flash) -> Result<Option<AppSlot>, OtaError> {
    Ok(otadata(flash)?.selected())
}

/// The state of the selected slot.
pub fn current_state(flash: &Flash) -> Result<Option<ImageState>, OtaError> {
    Ok(otadata(flash)?.state())
}

/// The slot the running image was loaded from (`None`: the factory app).
pub fn booted_slot(flash: &Flash) -> Result<Option<AppSlot>, OtaError> {
    let mut f = flash.borrow_mut();
    Ok(read_layout(&mut f)?.booted)
}

/// The first [`HEAD_LEN`] bytes of a slot: [`crate::image::app_desc`] reads the version.
pub fn slot_head(flash: &Flash, slot: AppSlot) -> Result<[u8; HEAD_LEN], OtaError> {
    let mut f = flash.borrow_mut();
    let l = read_layout(&mut f)?;
    let mut head = [0u8; HEAD_LEN];
    f.read(l.slot(slot).0, &mut head).map_err(|_| OtaError::Flash)?;
    Ok(head)
}

/// Build stamp of a slot's image, `date time`, for ordering installs.
fn build_stamp(head: &[u8; HEAD_LEN]) -> Option<([u8; 10], [u8; 8])> {
    let d = crate::image::app_desc(head)?;
    let (date, time) = (d.date.as_bytes(), d.time.as_bytes());
    Some((date.try_into().ok()?, time.try_into().ok()?))
}

/// The slot an install should go into: the one not running; from the factory app, the
/// one not selected; with nothing selected, the one with the older (or no) build.
pub fn next_slot(flash: &Flash) -> Result<AppSlot, OtaError> {
    let mut f = flash.borrow_mut();
    let l = read_layout(&mut f)?;
    if let Some(b) = l.booted {
        return Ok(b.other());
    }
    if let Some(s) = read_entries(&mut f, &l)?.selected() {
        return Ok(s.other());
    }
    Ok(older_slot(&mut f, &l))
}

fn read_head(f: &mut FlashStorage<'static>, l: &Layout, slot: AppSlot) -> [u8; HEAD_LEN] {
    let mut head = [0u8; HEAD_LEN];
    let _ = f.read(l.slot(slot).0, &mut head);
    head
}

/// The slot with the older build stamp (a slot without an image counts as oldest).
fn older_slot(f: &mut FlashStorage<'static>, l: &Layout) -> AppSlot {
    let a = build_stamp(&read_head(f, l, AppSlot::Ota0));
    let b = build_stamp(&read_head(f, l, AppSlot::Ota1));
    match (a, b) {
        (None, _) => AppSlot::Ota0,
        (Some(_), None) => AppSlot::Ota1,
        (Some(a), Some(b)) => {
            if a <= b {
                AppSlot::Ota0
            } else {
                AppSlot::Ota1
            }
        }
    }
}

/// Walk and verify the image in `slot`, reading it back from flash. `progress` sees the
/// percentage of the slot walked (the image is usually well short of the slot).
pub fn verify_slot(flash: &Flash, slot: AppSlot, progress: &mut dyn FnMut(u32)) -> Result<ImageInfo, OtaError> {
    let mut f = flash.borrow_mut();
    let l = read_layout(&mut f)?;
    let (base, len) = l.slot(slot);
    verify_range(&mut f, base, len, progress)
}

fn verify_range(f: &mut FlashStorage<'static>, base: u32, len: u32, progress: &mut dyn FnMut(u32)) -> Result<ImageInfo, OtaError> {
    let mut v = Verifier::new(len);
    let mut buf = alloc::vec![0u8; SECTOR as usize];
    let mut off = 0u32;
    let mut last = u32::MAX;
    while off < len && !v.done() {
        let n = buf.len().min((len - off) as usize);
        f.read(base + off, &mut buf[..n]).map_err(|_| OtaError::Flash)?;
        v.update(&buf[..n])?;
        off += n as u32;
        let pct = ((off as u64 * 100) / len as u64) as u32;
        if pct != last {
            last = pct;
            progress(pct);
        }
    }
    Ok(v.finish()?)
}

/// Write `slot` into otadata as the next boot, in `state`.
pub fn select(flash: &Flash, slot: AppSlot, state: ImageState) -> Result<(), OtaError> {
    let mut f = flash.borrow_mut();
    let l = read_layout(&mut f)?;
    let (i, e) = read_entries(&mut f, &l)?.select(slot, state);
    write_entry(&mut f, &l, i, e)
}

/// Confirm the running image: the app calls this once it is up. Selects the booted slot
/// if the bootloader fell back to it, then sets its state to `Valid`. Nothing is written
/// when it already is, or when the factory app is running. Returns whether it wrote.
pub fn mark_valid(flash: &Flash) -> Result<bool, OtaError> {
    let mut f = flash.borrow_mut();
    let l = read_layout(&mut f)?;
    let Some(booted) = l.booted else {
        return Ok(false);
    };
    let data = read_entries(&mut f, &l)?;
    if data.selected() != Some(booted) {
        let (i, e) = data.select(booted, ImageState::Valid);
        write_entry(&mut f, &l, i, e)?;
        return Ok(true);
    }
    if data.state() == Some(ImageState::Valid) {
        return Ok(false);
    }
    let (i, e) = data.set_state(ImageState::Valid).ok_or(OtaError::NoSlotSelected)?;
    write_entry(&mut f, &l, i, e)?;
    Ok(true)
}

/// Switch the next boot to the other slot, if its image verifies: the other than the
/// running one, from the factory app the other than the selected one, and with nothing
/// selected the older build. Returns the slot selected.
pub fn rollback(flash: &Flash, progress: &mut dyn FnMut(u32)) -> Result<AppSlot, OtaError> {
    let mut f = flash.borrow_mut();
    let l = read_layout(&mut f)?;
    let data = read_entries(&mut f, &l)?;
    let target = match l.booted.or(data.selected()) {
        Some(s) => s.other(),
        None => older_slot(&mut f, &l),
    };
    let (base, len) = l.slot(target);
    verify_range(&mut f, base, len, progress)?;
    let (i, e) = data.select(target, ImageState::Valid);
    write_entry(&mut f, &l, i, e)?;
    Ok(target)
}

/// Boot the last known firmware again (the recovery app's Retry): the selected slot if
/// its image verifies, else the newest verifiable slot, which is then selected as
/// `Valid`. Returns the slot the next boot will try.
pub fn retry(flash: &Flash, progress: &mut dyn FnMut(u32)) -> Result<AppSlot, OtaError> {
    let mut f = flash.borrow_mut();
    let l = read_layout(&mut f)?;
    let data = read_entries(&mut f, &l)?;
    if let Some(s) = data.selected() {
        let (base, len) = l.slot(s);
        verify_range(&mut f, base, len, progress)?;
        return Ok(s);
    }
    let newest = older_slot(&mut f, &l).other();
    for slot in [newest, newest.other()] {
        let (base, len) = l.slot(slot);
        if verify_range(&mut f, base, len, progress).is_ok() {
            let (i, e) = data.select(slot, ImageState::Valid);
            write_entry(&mut f, &l, i, e)?;
            return Ok(slot);
        }
    }
    Err(OtaError::NoBootableImage)
}

/// Erase otadata so the next boot is the factory (recovery) app, as
/// `esp_ota_set_boot_partition(factory)` does. The slots are untouched.
pub fn boot_recovery(flash: &Flash) -> Result<(), OtaError> {
    let mut f = flash.borrow_mut();
    let l = read_layout(&mut f)?;
    NorFlash::erase(&mut *f, l.otadata, l.otadata + 2 * SECTOR).map_err(|_| OtaError::Flash)
}

/// Streams an image into a slot. Nothing about the boot changes until [`OtaWriter::finish`].
pub struct OtaWriter<'a> {
    flash: &'a Flash,
    layout: Layout,
    slot: AppSlot,
    base: u32,
    capacity: u32,
    /// Bytes waiting for a full sector.
    pending: Vec<u8>,
    /// Bytes committed to flash.
    written: u32,
    /// End of the erased range.
    erased_to: u32,
    verifier: Verifier,
}

impl<'a> OtaWriter<'a> {
    /// Start writing `slot`; the first block is erased when the first sector lands.
    pub fn begin(flash: &'a Flash, slot: AppSlot) -> Result<OtaWriter<'a>, OtaError> {
        let layout = read_layout(&mut flash.borrow_mut())?;
        let (base, capacity) = layout.slot(slot);
        Ok(OtaWriter {
            flash,
            layout,
            slot,
            base,
            capacity,
            pending: Vec::with_capacity(SECTOR as usize),
            written: 0,
            erased_to: 0,
            verifier: Verifier::new(capacity),
        })
    }

    /// The slot being written.
    pub fn slot(&self) -> AppSlot {
        self.slot
    }

    /// Bytes the slot holds.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Bytes accepted so far.
    pub fn len(&self) -> u32 {
        self.written + self.pending.len() as u32
    }

    /// Whether nothing has been written yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Feed the next bytes of the image, any length. The image is checked as it streams,
    /// so a file that is not a firmware image is refused before it fills the slot.
    pub fn write(&mut self, mut data: &[u8]) -> Result<(), OtaError> {
        self.verifier.update(data)?;
        while !data.is_empty() {
            let room = SECTOR as usize - self.pending.len();
            let n = data.len().min(room);
            self.pending.extend_from_slice(&data[..n]);
            data = &data[n..];
            if self.pending.len() == SECTOR as usize {
                self.flush()?;
            }
        }
        Ok(())
    }

    /// Commit the pending bytes (padded to a word) as one sector.
    fn flush(&mut self) -> Result<(), OtaError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        while !self.pending.len().is_multiple_of(4) {
            self.pending.push(0xFF);
        }
        let off = self.written;
        let end = off + self.pending.len() as u32;
        if end > self.capacity {
            return Err(OtaError::TooLarge { len: end as u64, capacity: self.capacity });
        }
        let mut f = self.flash.borrow_mut();
        if end > self.erased_to {
            let to = (self.erased_to + BLOCK).min(self.capacity);
            NorFlash::erase(&mut *f, self.base + self.erased_to, self.base + to).map_err(|_| OtaError::Flash)?;
            self.erased_to = to;
        }
        NorFlash::write(&mut *f, self.base + off, &self.pending).map_err(|_| OtaError::Flash)?;
        self.written = end;
        self.pending.clear();
        Ok(())
    }

    /// Commit the tail, read the slot back and verify it, then select the slot for the
    /// next boot in state `New`. Returns what was installed.
    pub fn finish(mut self, progress: &mut dyn FnMut(u32)) -> Result<ImageInfo, OtaError> {
        self.flush()?;
        let streamed = core::mem::replace(&mut self.verifier, Verifier::new(0)).finish()?;
        let mut f = self.flash.borrow_mut();
        let back = verify_range(&mut f, self.base, streamed.len, progress)?;
        if back.len != streamed.len || back.head != streamed.head {
            return Err(OtaError::Verify);
        }
        let (i, e) = read_entries(&mut f, &self.layout)?.select(self.slot, ImageState::New);
        write_entry(&mut f, &self.layout, i, e)?;
        Ok(back)
    }
}

/// Install the image at `path` on the card into the slot [`next_slot`] picks. `progress`
/// sees the copy in percent (0 to 100), then the read-back verification runs. On success
/// the slot is selected as the next boot in state `New`; the caller resets.
pub fn install_from_card(flash: &Flash, fs: &impl Fs, path: &str, progress: &mut dyn FnMut(u32)) -> Result<(AppSlot, ImageInfo), OtaError> {
    let file = fs.open(path)?;
    let len = file.len();
    let slot = next_slot(flash)?;
    let mut w = OtaWriter::begin(flash, slot)?;
    if len == 0 {
        return Err(OtaError::Image(ImageError::Truncated));
    }
    if len > w.capacity() as u64 {
        return Err(OtaError::TooLarge { len, capacity: w.capacity() });
    }
    let mut buf = alloc::vec![0u8; SECTOR as usize];
    let mut off = 0u64;
    let mut last = u32::MAX;
    while off < len {
        let n = file.read_at(off, &mut buf)?;
        if n == 0 {
            return Err(OtaError::Fs(FsError::Eof));
        }
        w.write(&buf[..n])?;
        off += n as u64;
        let pct = ((off * 100) / len) as u32;
        if pct != last {
            last = pct;
            progress(pct);
        }
    }
    let info = w.finish(&mut |_| {})?;
    Ok((slot, info))
}

/// The first of [`CARD_PATHS`] that exists on the card.
pub fn find_card_update(fs: &impl Fs) -> Option<&'static str> {
    CARD_PATHS.into_iter().find(|p| fs.exists(p))
}

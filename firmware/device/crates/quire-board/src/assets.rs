//! The `assets` flash partition: read-only data the firmware image cannot carry because
//! the ESP32-C3 maps at most 4 MB of flash for code and constants. Today it holds the
//! built-in dictionary (`en.qdict`, flashed at the partition's start); reads go through
//! the SPI flash driver, so nothing here is memory-mapped and nothing is cached in RAM.
//! Opening the region registers the flash handle with [`crate::flash`], so the OTA
//! writer shares it.

use embedded_storage::ReadStorage;
use esp_bootloader_esp_idf::partitions::{self, DataPartitionSubType, PartitionType};
use esp_storage::FlashStorage;
use quire_ui::dict::builtin::DictSource;

/// Label of the assets partition in `partitions.csv`.
pub const ASSETS_LABEL: &str = "assets";

/// A window onto the flash, addressed from the partition's start.
pub struct FlashRegion {
    flash: &'static crate::flash::Flash,
    base: u32,
    len: u32,
}

impl FlashRegion {
    /// Open the assets partition. The partition table is read from the flash, so the
    /// offset follows whatever table was flashed, not a constant in the code. The flash
    /// handle is shared through [`crate::flash::shared`] whether or not the partition
    /// is found.
    pub fn assets(flash: FlashStorage<'static>) -> Option<FlashRegion> {
        let flash = crate::flash::share(flash);
        let mut table = [0u8; partitions::PARTITION_TABLE_MAX_LEN];
        let (base, len) = {
            let mut f = flash.borrow_mut();
            let pt = partitions::read_partition_table(&mut *f, &mut table).ok()?;
            let entry = pt
                .iter()
                .find(|p| p.label_as_str() == ASSETS_LABEL)
                .or_else(|| pt.iter().find(|p| p.partition_type() == PartitionType::Data(DataPartitionSubType::Spiffs)))?;
            (entry.offset(), entry.len())
        };
        Some(FlashRegion { flash, base, len })
    }

    /// Absolute flash offset of the region.
    pub fn base(&self) -> u32 {
        self.base
    }

    /// The shared flash handle (the OTA writer borrows the same one).
    pub fn flash(&self) -> &'static crate::flash::Flash {
        self.flash
    }
}

impl DictSource for FlashRegion {
    fn len(&self) -> usize {
        self.len as usize
    }
    fn read(&self, off: usize, buf: &mut [u8]) -> bool {
        let end = off.checked_add(buf.len());
        if end.is_none_or(|e| e > self.len as usize) {
            return false;
        }
        self.flash.borrow_mut().read(self.base + off as u32, buf).is_ok()
    }
}

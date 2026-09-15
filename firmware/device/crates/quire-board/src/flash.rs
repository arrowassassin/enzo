//! The one SPI-flash handle. `esp_storage::FlashStorage::new` may run once per boot, so
//! the binary registers it here and every user (the assets partition, the OTA writer)
//! borrows the same `RefCell` for the duration of one operation.

use core::cell::RefCell;
use core::sync::atomic::{AtomicPtr, Ordering};

use esp_storage::FlashStorage;
use static_cell::StaticCell;

/// The shared flash: borrow it mutably for one read, write or erase at a time.
pub type Flash = RefCell<FlashStorage<'static>>;

static FLASH: StaticCell<Flash> = StaticCell::new();
static SHARED: AtomicPtr<Flash> = AtomicPtr::new(core::ptr::null_mut());

/// Register the flash handle for the rest of the boot. Panics if called twice, as
/// [`FlashStorage::new`] itself does.
pub fn share(flash: FlashStorage<'static>) -> &'static Flash {
    let f: &'static Flash = FLASH.init(RefCell::new(flash));
    SHARED.store(f as *const Flash as *mut Flash, Ordering::Release);
    f
}

/// The registered handle, once [`share`] has run.
pub fn shared() -> Option<&'static Flash> {
    let p = SHARED.load(Ordering::Acquire);
    // SAFETY: the pointer is null or was made from the `&'static` that `share` returned.
    unsafe { p.cast_const().as_ref() }
}

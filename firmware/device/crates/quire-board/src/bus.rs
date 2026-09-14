//! The shared SPI bus (SCLK 8, MOSI 10) between the panel and the card. Each device
//! borrows the bus for one transaction at a time and the clock is switched to the
//! device's rate when it differs from the last one used.

use core::cell::{Cell, RefCell};

use embedded_hal::spi::{ErrorType, SpiBus};
use esp_hal::spi::master::{Config, Spi};
use esp_hal::time::Rate;
use esp_hal::Blocking;

/// Who is using the bus; each role has its own clock rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The e-paper controller (10 MHz UC8253, 20 MHz UC8279).
    Panel,
    /// The SD card (400 kHz while it initialises, then 20 MHz).
    Card,
}

/// The bus plus the rate it is currently configured for and each role's rate.
pub struct SharedBus {
    bus: RefCell<Spi<'static, Blocking>>,
    hz: Cell<u32>,
    panel_hz: Cell<u32>,
    card_hz: Cell<u32>,
}

impl SharedBus {
    /// Wrap a configured bus (currently running at `hz`).
    pub fn new(bus: Spi<'static, Blocking>, hz: u32) -> Self {
        SharedBus { bus: RefCell::new(bus), hz: Cell::new(hz), panel_hz: Cell::new(10_000_000), card_hz: Cell::new(400_000) }
    }

    /// Set a role's clock rate (takes effect on its next transaction).
    pub fn set_rate(&self, role: Role, hz: u32) {
        match role {
            Role::Panel => self.panel_hz.set(hz),
            Role::Card => self.card_hz.set(hz),
        }
    }

    /// A handle for one role.
    pub fn handle(&self, role: Role) -> BusHandle<'_> {
        BusHandle { shared: self, role }
    }

    fn rate(&self, role: Role) -> u32 {
        match role {
            Role::Panel => self.panel_hz.get(),
            Role::Card => self.card_hz.get(),
        }
    }

    fn with<R>(&self, hz: u32, f: impl FnOnce(&mut Spi<'static, Blocking>) -> R) -> R {
        let mut bus = self.bus.borrow_mut();
        if self.hz.get() != hz {
            let cfg = Config::default().with_frequency(Rate::from_hz(hz)).with_mode(esp_hal::spi::Mode::_0);
            if bus.apply_config(&cfg).is_ok() {
                self.hz.set(hz);
            }
        }
        f(&mut bus)
    }
}

/// A per-role view of the shared bus.
pub struct BusHandle<'a> {
    shared: &'a SharedBus,
    role: Role,
}

impl ErrorType for BusHandle<'_> {
    type Error = esp_hal::spi::Error;
}

impl SpiBus<u8> for BusHandle<'_> {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.shared.with(self.shared.rate(self.role), |b| b.read(words))
    }
    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.shared.with(self.shared.rate(self.role), |b| b.write(words))
    }
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        self.shared.with(self.shared.rate(self.role), |b| SpiBus::transfer(b, read, write))
    }
    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.shared.with(self.shared.rate(self.role), |b| SpiBus::transfer_in_place(b, words))
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.shared.with(self.shared.rate(self.role), SpiBus::flush)
    }
}

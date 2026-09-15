//! The panel for the recovery screen: the same probe and init as quire-x3, a full (GC)
//! refresh for the screen and DU refreshes for the progress line.

use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, Output};
use quire_board::bus::{BusHandle, Role, SharedBus};
use quire_epd::{Epd, Mode, ProbeResult, Rotation, PLANE_BYTES};
use quire_gfx::Frame;

/// The plane buffer the driver streams to the panel (static: 52 KB is too much stack).
static mut PLANE: [u8; PLANE_BYTES] = [0; PLANE_BYTES];

/// The driver plus a little refresh policy.
pub struct Display {
    epd: Epd<BusHandle<'static>, Output<'static>, Output<'static>, Input<'static>, Output<'static>, Delay>,
    /// Controller name, for the screen.
    pub name: &'static str,
    asleep: bool,
}

impl Display {
    /// Initialise the controller the probe found.
    pub fn new(
        bus: &'static SharedBus,
        dc: Output<'static>,
        rst: Output<'static>,
        busy: Input<'static>,
        cs: Output<'static>,
        probe: ProbeResult,
    ) -> Display {
        let controller = probe.controller();
        bus.set_rate(Role::Panel, quire_epd::spi_hz(controller));
        let mut epd = Epd::new(bus.handle(Role::Panel), dc, rst, busy, cs, Delay::new(), controller);
        if let Err(e) = epd.init() {
            esp_println::println!("panel init: {e:?}");
        }
        let name = match controller {
            quire_epd::Controller::Uc8253 => "UC8253",
            quire_epd::Controller::Uc8279 => "UC8279d",
        };
        Display { epd, name, asleep: false }
    }

    /// Show `frame`; `full` asks for a GC refresh, otherwise a DU page turn. Blocks
    /// until the panel is idle (about half a second).
    pub fn show(&mut self, frame: &Frame, full: bool) {
        if self.asleep {
            let _ = self.epd.wake();
            self.asleep = false;
        }
        // SAFETY: the plane is only touched here, one refresh at a time.
        let plane = unsafe { &mut *core::ptr::addr_of_mut!(PLANE) };
        quire_epd::rotate_frame_to_plane(frame.bits(), plane, Rotation::Portrait);
        let mode = if full || self.epd.needs_gc() { Mode::Gc } else { Mode::Du };
        if let Err(e) = self.epd.refresh(plane, mode) {
            esp_println::println!("refresh: {e:?}");
        }
    }

    /// Put the controller to sleep (the image stays).
    pub fn sleep(&mut self) {
        if !self.asleep {
            let _ = self.epd.sleep();
            self.asleep = true;
        }
    }
}

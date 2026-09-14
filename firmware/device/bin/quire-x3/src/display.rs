//! The panel behind the UI's frame: probe, init, frame-to-plane conversion, the refresh
//! policy (DU for page turns, GC for dialogs and every N pages), and sleep.

use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, Output};
use quire_board::bus::{BusHandle, Role, SharedBus};
use quire_epd::{Controller, Epd, Mode, ProbeResult, Rotation, PLANE_BYTES};
use quire_gfx::Frame;
use quire_ui::Refresh;

/// The plane buffer the driver streams to the panel (static: 52 KB is too much stack).
static mut PLANE: [u8; PLANE_BYTES] = [0xFF; PLANE_BYTES];

/// The driver plus the refresh policy.
pub struct Display {
    epd: Epd<BusHandle<'static>, Output<'static>, Output<'static>, Input<'static>, Output<'static>, Delay>,
    /// Which controller the probe found.
    pub controller: Controller,
    du_since_gc: u8,
    rotation: Rotation,
    asleep: bool,
}

impl Display {
    /// Probe the controller, initialise it and clear the panel.
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
            log::error!("panel init: {e:?}");
        }
        Display { epd, controller, du_since_gc: 0, rotation: Rotation::Portrait, asleep: false }
    }

    /// Controller name for About.
    pub fn name(&self) -> &'static str {
        match self.controller {
            Controller::Uc8253 => "UC8253",
            Controller::Uc8279 => "UC8279d",
        }
    }

    /// Flip the mounting orientation (left-handed mode).
    pub fn set_upside_down(&mut self, flip: bool) {
        self.rotation = if flip { Rotation::Flip180 } else { Rotation::Portrait };
    }

    /// Show `frame` with the refresh the UI asked for. `gc_every` page turns force a GC
    /// so ghosting never builds up. Returns once the panel is idle again; `poll` is
    /// called every few milliseconds during the wait so keys keep being sampled.
    pub fn show(&mut self, frame: &Frame, refresh: Refresh, gc_every: u8, mut poll: impl FnMut()) {
        if refresh == Refresh::None {
            return;
        }
        if self.asleep {
            if let Err(e) = self.epd.wake() {
                log::error!("panel wake: {e:?}");
            }
            self.asleep = false;
            self.du_since_gc = gc_every; // first paint after a sleep is a GC
        }
        // SAFETY: the plane is only touched from the main task, one refresh at a time.
        let plane = unsafe { &mut *core::ptr::addr_of_mut!(PLANE) };
        quire_epd::rotate_frame_to_plane(frame.bits(), plane, self.rotation);
        let mode = if refresh == Refresh::Gc || self.du_since_gc >= gc_every.max(1) || self.epd.needs_gc() {
            self.du_since_gc = 0;
            Mode::Gc
        } else {
            self.du_since_gc += 1;
            Mode::Du
        };
        if let Err(e) = self.epd.begin_refresh(plane, mode) {
            log::error!("refresh: {e:?}");
            return;
        }
        let mut elapsed_ms = 0u32;
        loop {
            match self.epd.poll_refresh(elapsed_ms) {
                Ok(true) => break,
                Ok(false) => {
                    poll();
                    Delay::new().delay_millis(5);
                    elapsed_ms += 5;
                }
                Err(e) => {
                    log::error!("refresh wait: {e:?}");
                    return;
                }
            }
        }
        if let Err(e) = self.epd.finish_refresh(plane) {
            log::error!("refresh finish: {e:?}");
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

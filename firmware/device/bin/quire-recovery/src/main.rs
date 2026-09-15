//! Quire recovery for the Xteink X3: the factory app in the 512 KB `recovery` partition.
//! The bootloader lands here when no OTA slot is bootable (or the app asked for it).
//! It brings up the panel and the keys, says why it is running, and offers three
//! things: Retry the selected firmware, install an update from the card, or roll back
//! to the other slot. It stays small: no UI crate, two font strikes, no radio.
#![no_std]
#![no_main]

extern crate alloc;

mod display;
mod screen;

use alloc::format;
use alloc::string::String;

use esp_hal::analog::adc::{Adc, AdcCalLine, AdcConfig, AdcPin, Attenuation};
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull, RtcPinWithResistors};
use esp_hal::peripherals::ADC1;
use esp_hal::rtc_cntl::sleep::{RtcioWakeupSource, WakeupLevel};
use esp_hal::rtc_cntl::Rtc;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::{Instant, Rate};
use esp_hal::Blocking;
use esp_println::println;
use quire_board::bus::SharedBus;
use quire_board::flash::Flash;
use quire_board::keys::{Key, KeyKind, KeyMachine};
use quire_board::ota;
use quire_board::sdfs::{SdFs, Vm};
use quire_gfx::Frame;
use static_cell::StaticCell;

use crate::display::Display;
use crate::screen::State;

esp_bootloader_esp_idf::esp_app_desc!();

/// Build stamp shown in the head.
const BUILD: &str = env!("CARGO_PKG_VERSION");

static BUS: StaticCell<SharedBus> = StaticCell::new();
static VM: StaticCell<Vm> = StaticCell::new();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("recovery panic: {info}");
    Delay::new().delay_millis(500);
    esp_hal::system::software_reset()
}

type KeyPin1 = AdcPin<esp_hal::peripherals::GPIO1<'static>, ADC1<'static>, AdcCalLine<ADC1<'static>>>;
type KeyPin2 = AdcPin<esp_hal::peripherals::GPIO2<'static>, ADC1<'static>, AdcCalLine<ADC1<'static>>>;

/// The key inputs, read exactly as quire-x3 reads them.
struct Keys {
    adc: Adc<'static, ADC1<'static>, Blocking>,
    g1: KeyPin1,
    g2: KeyPin2,
    power: Input<'static>,
    machine: KeyMachine,
}

impl Keys {
    fn read_mv(&mut self, which: u8) -> u16 {
        let r = if which == 1 { nb::block!(self.adc.read_oneshot(&mut self.g1)) } else { nb::block!(self.adc.read_oneshot(&mut self.g2)) };
        r.unwrap_or(4095)
    }
    fn sample(&mut self, now_ms: u32) -> heapless::Vec<quire_board::keys::KeyEvent, 6> {
        let g1 = self.read_mv(1);
        let g2 = self.read_mv(2);
        let power = self.power.is_low();
        self.machine.sample(g1, g2, power, now_ms)
    }
}

/// The pins the boot-time controller probe bit-bangs.
struct ProbePins<'a> {
    sclk: Output<'a>,
    mosi: esp_hal::gpio::Flex<'a>,
    rst: Output<'static>,
    cs: Output<'static>,
    dc: Output<'static>,
    delay: Delay,
}

impl quire_epd::ProbeBus for ProbePins<'_> {
    fn rst(&mut self, high: bool) {
        self.rst.set_level(if high { Level::High } else { Level::Low });
    }
    fn cs(&mut self, high: bool) {
        self.cs.set_level(if high { Level::High } else { Level::Low });
    }
    fn dc(&mut self, high: bool) {
        self.dc.set_level(if high { Level::High } else { Level::Low });
    }
    fn sclk(&mut self, high: bool) {
        self.sclk.set_level(if high { Level::High } else { Level::Low });
    }
    fn mosi_drive(&mut self, high: bool) {
        self.mosi.set_output_enable(true);
        self.mosi.set_level(if high { Level::High } else { Level::Low });
    }
    fn mosi_release(&mut self) {
        self.mosi.set_output_enable(false);
        self.mosi.apply_input_config(&InputConfig::default().with_pull(Pull::Up));
        self.mosi.set_input_enable(true);
    }
    fn mosi_read(&mut self) -> bool {
        self.mosi.is_high()
    }
    fn delay_us(&mut self, us: u32) {
        self.delay.delay_micros(us);
    }
}

fn uptime_ms() -> u32 {
    Instant::now().duration_since_epoch().as_millis() as u32
}

/// Mount the card, power-cycling its rail first when asked. GPIO12 is re-created per
/// attempt because a failed mount consumes it with the discarded card object.
fn mount_card(bus: &'static SharedBus, sd_power: &mut Output<'static>, cycle: bool) -> Option<SdFs> {
    let delay = Delay::new();
    if cycle {
        sd_power.set_low();
        delay.delay_millis(200);
        sd_power.set_high();
        delay.delay_millis(50);
    }
    // SAFETY: GPIO12 is used by nothing else; a previous attempt's pin was dropped with it.
    let cs = Output::new(unsafe { esp_hal::peripherals::GPIO12::steal() }, Level::High, OutputConfig::default());
    match SdFs::mount(bus, cs, &VM) {
        Ok(fs) => Some(fs),
        Err(e) => {
            println!("card: {e}");
            None
        }
    }
}

/// Power everything down; only the Power key wakes the device.
fn deep_sleep(display: &mut Display, sd_power: &mut Output<'static>, rtc: &mut Rtc<'static>) -> ! {
    display.sleep();
    sd_power.set_low();
    quire_board::power::hold_sd_rail(true);
    // SAFETY: GPIO3 is also held as the power-key `Input`; the wake source only programs
    // the RTC wake bits of the same pad and does not change its mode.
    let mut wake_pin = unsafe { esp_hal::peripherals::GPIO3::steal() };
    let mut pins: [(&mut dyn RtcPinWithResistors, WakeupLevel); 1] = [(&mut wake_pin, WakeupLevel::Low)];
    let gpio = RtcioWakeupSource::new(&mut pins);
    rtc.sleep_deep(&[&gpio])
}

/// Which way an action went: restart into a slot, or an error to show.
enum Outcome {
    Restart(String),
    Failed(String),
}

fn describe(flash: &Flash, slot: ota::AppSlot) -> String {
    match ota::slot_head(flash, slot).ok().as_ref().and_then(|h| quire_board::image::app_desc(h)) {
        Some(d) => format!("{} {} in slot {}", d.project, d.version, slot.letter()),
        None => format!("slot {}", slot.letter()),
    }
}

#[esp_hal::main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let mut peripherals = esp_hal::init(config);
    // The frame, the card driver's buffers and the 4 KB copy buffers live here.
    esp_alloc::heap_allocator!(size: 96 * 1024);
    let delay = Delay::new();

    println!("quire-recovery {BUILD} boot: reset {:?}", esp_hal::system::reset_reason());
    quire_board::power::release_holds();

    // Rails and the shared SPI bus, then the panel (probe first, bit-banged on the MOSI pad).
    let mut sd_power = Output::new(peripherals.GPIO13, Level::High, OutputConfig::default());
    let sd_cs = Output::new(peripherals.GPIO12, Level::High, OutputConfig::default());
    let epd_cs = Output::new(peripherals.GPIO21, Level::High, OutputConfig::default());
    let epd_dc = Output::new(peripherals.GPIO4, Level::High, OutputConfig::default());
    let epd_rst = Output::new(peripherals.GPIO5, Level::High, OutputConfig::default());
    let epd_busy = Input::new(peripherals.GPIO6, InputConfig::default().with_pull(Pull::Up));
    delay.delay_millis(20);
    let (probe, epd_dc, epd_rst, epd_cs) = {
        let sclk = Output::new(peripherals.GPIO8.reborrow(), Level::Low, OutputConfig::default());
        let mut mosi = esp_hal::gpio::Flex::new(peripherals.GPIO10.reborrow());
        mosi.apply_output_config(&OutputConfig::default());
        mosi.set_output_enable(true);
        mosi.set_low();
        let mut pins = ProbePins { sclk, mosi, rst: epd_rst, cs: epd_cs, dc: epd_dc, delay: Delay::new() };
        let r = quire_epd::probe(&mut pins);
        pins.mosi.set_output_enable(false);
        (r, pins.dc, pins.rst, pins.cs)
    };
    println!("panel probe: {:?}", probe.verdict);
    let spi = Spi::new(peripherals.SPI2, SpiConfig::default().with_frequency(Rate::from_mhz(10)).with_mode(esp_hal::spi::Mode::_0))
        .expect("spi")
        .with_sck(peripherals.GPIO8)
        .with_mosi(peripherals.GPIO10)
        .with_miso(peripherals.GPIO7);
    let bus: &'static SharedBus = BUS.init(SharedBus::new(spi, 10_000_000));
    let mut display = Display::new(bus, epd_dc, epd_rst, epd_busy, epd_cs, probe);

    // Keys.
    let mut adc_cfg = AdcConfig::new();
    let g1: KeyPin1 = adc_cfg.enable_pin_with_cal(peripherals.GPIO1, Attenuation::_11dB);
    let g2: KeyPin2 = adc_cfg.enable_pin_with_cal(peripherals.GPIO2, Attenuation::_11dB);
    let adc = Adc::new(peripherals.ADC1, adc_cfg);
    let power_key = Input::new(peripherals.GPIO3, InputConfig::default().with_pull(Pull::Up));
    let mut keys = Keys { adc, g1, g2, power: power_key, machine: KeyMachine::new() };

    // The flash (otadata and the slots) and the card.
    let flash: &'static Flash = quire_board::flash::share(esp_storage::FlashStorage::new(peripherals.FLASH));
    #[allow(clippy::drop_non_drop)]
    drop(sd_cs);
    let mut fs = mount_card(bus, &mut sd_power, false);
    let mut update = fs.as_ref().and_then(ota::find_card_update);

    let mut state = State::gather(flash, fs.as_ref().map(|_| "card"), update, display.name);
    println!("recovery: {} | {} | {}", state.reason, state.slots.join(" | "), state.card);
    let mut frame = Frame::panel();
    state.draw(&mut frame);
    display.show(&frame, true);

    let mut rtc = Rtc::new(peripherals.LPWR);
    loop {
        delay.delay_millis(10);
        let now = uptime_ms();
        for ev in keys.sample(now) {
            let outcome = match (ev.key, ev.kind) {
                (Key::Power, KeyKind::Long) => deep_sleep(&mut display, &mut sd_power, &mut rtc),
                (Key::Back, KeyKind::Press) => {
                    state.status = String::from("Checking the selected firmware…");
                    state.draw(&mut frame);
                    display.show(&frame, false);
                    match ota::retry(flash, &mut |_| {}) {
                        Ok(slot) => Outcome::Restart(format!("Restarting {}…", describe(flash, slot))),
                        Err(e) => Outcome::Failed(format!("Retry failed: {e}")),
                    }
                }
                (Key::Confirm, KeyKind::Press) => {
                    if fs.is_none() {
                        fs = mount_card(bus, &mut sd_power, true);
                        update = fs.as_ref().and_then(ota::find_card_update);
                    }
                    match (fs.as_ref(), update) {
                        (None, _) => Outcome::Failed(String::from("No card. Insert a microSD card with the update file and try again.")),
                        (Some(_), None) => {
                            Outcome::Failed(format!("No update file on the card. Looked for {}.", ota::CARD_PATHS.join(", ")))
                        }
                        (Some(fs), Some(path)) => {
                            let mut shown = u32::MAX;
                            let r = ota::install_from_card(flash, fs, path, &mut |pct| {
                                // A DU refresh every 5 % (and at the end).
                                if pct / 5 != shown / 5 || pct == 100 {
                                    shown = pct;
                                    state.status = format!("Installing… {pct} %");
                                    state.draw(&mut frame);
                                    display.show(&frame, false);
                                }
                            });
                            match r {
                                Ok((slot, info)) => {
                                    let what = match info.desc() {
                                        Some(d) => format!("{} {}", d.project, d.version),
                                        None => String::from("the update"),
                                    };
                                    Outcome::Restart(format!("Installed {what} into slot {}. Restarting…", slot.letter()))
                                }
                                Err(e) => Outcome::Failed(format!("Install failed: {e}")),
                            }
                        }
                    }
                }
                (Key::Right, KeyKind::Press) => {
                    state.status = String::from("Checking the other slot…");
                    state.draw(&mut frame);
                    display.show(&frame, false);
                    match ota::rollback(flash, &mut |_| {}) {
                        Ok(slot) => Outcome::Restart(format!("Switched to {}. Restarting…", describe(flash, slot))),
                        Err(e) => Outcome::Failed(format!("Rollback failed: {e}")),
                    }
                }
                _ => continue,
            };
            match outcome {
                Outcome::Restart(msg) => {
                    println!("{msg}");
                    state.status = msg;
                    state.draw(&mut frame);
                    display.show(&frame, true);
                    display.sleep();
                    delay.delay_millis(300);
                    esp_hal::system::software_reset();
                }
                Outcome::Failed(msg) => {
                    println!("{msg}");
                    state = State::gather(flash, fs.as_ref().map(|_| "card"), update, display.name);
                    state.status = msg;
                    state.draw(&mut frame);
                    display.show(&frame, true);
                }
            }
        }
    }
}

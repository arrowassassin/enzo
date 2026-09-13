//! Segment 0 toolchain proof: boots, allocates, starts the scheduler and the Wi-Fi
//! driver, reports free heap. Everything else arrives in later segments.
#![no_std]
#![no_main]

extern crate alloc;

use embassy_time::{Duration, Timer};
use esp_hal::{clock::CpuClock, interrupt::software::SoftwareInterruptControl, ram, timer::timg::TimerGroup};
use esp_backtrace as _;
use esp_println::println;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 96 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let (_wifi_controller, _interfaces) =
        esp_radio::wifi::new(peripherals.WIFI, Default::default()).expect("wifi");

    println!("quire-x3 segment-0 boot ok; free heap {} B", esp_alloc::HEAP.free());
    loop {
        Timer::after(Duration::from_secs(5)).await;
        println!("alive; free heap {} B", esp_alloc::HEAP.free());
    }
}

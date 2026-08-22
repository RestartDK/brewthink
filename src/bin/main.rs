#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use esp_println as _;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // First X4-safe firmware shape: logging-only heartbeat.
    // Deliberately do not initialize Wi-Fi, BLE, display, SD, GPIO13, or any
    // flash-writing/OTA path here.

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    let _ = spawner;

    info!(
        "Brewthink logging-only firmware booted: version={=str}",
        env!("CARGO_PKG_VERSION")
    );
    info!("No Wi-Fi, BLE, display, SD, GPIO13, or flash-writing path initialized");

    let mut heartbeat: u32 = 0;
    loop {
        info!("Brewthink heartbeat {}", heartbeat);
        heartbeat = heartbeat.wrapping_add(1);
        Timer::after(Duration::from_secs(1)).await;
    }
}

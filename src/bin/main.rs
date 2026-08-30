#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use brewthink::{
    display::{
        diagnostic::{fill_checkerboard, fill_rotated_orientation},
        framebuffer::{FRAME_BYTES, Frame, Rotation},
        ssd1677::{DisplayBus, InitializedSsd1677, Ssd1677},
    },
    x4::{SharedSpiChipSelects, X4DisplayHardware},
};
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use esp_println as _;

esp_bootloader_esp_idf::esp_app_desc!();

const DISPLAY_STAGE: &str = match option_env!("BREWTHINK_DISPLAY_STAGE") {
    Some(stage) => stage,
    None => "none",
};
const DISPLAY_ROTATION: &str = match option_env!("BREWTHINK_DISPLAY_ROTATION") {
    Some(rotation) => rotation,
    None => "270",
};
const BUILT_IMAGE: &[u8; FRAME_BYTES] =
    include_bytes!(concat!(env!("OUT_DIR"), "/brewthink-image.bin"));

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let chip_selects = SharedSpiChipSelects::deselected(peripherals.GPIO21, peripherals.GPIO12);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    let _ = spawner;

    info!(
        "Brewthink X4 firmware booted: version={=str}",
        env!("CARGO_PKG_VERSION")
    );
    info!(
        "X4 display CS GPIO21 high={}",
        chip_selects.display_is_deselected()
    );
    info!("X4 SD CS GPIO12 high={}", chip_selects.sd_is_deselected());
    assert!(chip_selects.both_are_deselected());
    info!("Display diagnostic stage={=str}", DISPLAY_STAGE);
    info!("Display rotation degrees={=str}", DISPLAY_ROTATION);

    if DISPLAY_STAGE == "none" {
        info!("No SPI, display reset, SD, GPIO13, radio, or flash-writing path initialized");
        heartbeat().await;
    }

    if !matches!(
        DISPLAY_STAGE,
        "reset"
            | "initialize"
            | "write"
            | "refresh"
            | "black"
            | "checkerboard"
            | "orientation"
            | "image"
    ) {
        hold(chip_selects, "unknown display diagnostic stage");
    }

    let rotation = if matches!(DISPLAY_STAGE, "orientation" | "image") {
        match parse_rotation(DISPLAY_ROTATION) {
            Some(rotation) => rotation,
            None => hold(chip_selects, "unknown display rotation"),
        }
    } else {
        Rotation::Degrees0
    };

    let hardware = match X4DisplayHardware::new(
        peripherals.SPI2,
        peripherals.GPIO8,
        peripherals.GPIO10,
        peripherals.GPIO4,
        peripherals.GPIO5,
        peripherals.GPIO6,
        chip_selects,
    ) {
        Ok(hardware) => hardware,
        Err(_) => hold((), "SPI2 configuration failed"),
    };
    run_display_diagnostic(hardware, DISPLAY_STAGE, rotation);
}

async fn heartbeat() -> ! {
    let mut heartbeat: u32 = 0;
    loop {
        info!("Brewthink heartbeat {}", heartbeat);
        heartbeat = heartbeat.wrapping_add(1);
        Timer::after(Duration::from_secs(1)).await;
    }
}

fn run_display_diagnostic(hardware: X4DisplayHardware<'_>, stage: &str, rotation: Rotation) -> ! {
    let (mut epd_bus, sd_chip_select) = hardware.into_parts();
    info!("SPI2 ready: mode=0 frequency_mhz=20 SD_CS_high=true");

    if stage == "reset" {
        epd_bus.reset();
        hold((epd_bus, sd_chip_select), "display reset complete");
    }

    let mut controller = Ssd1677::new(epd_bus);
    let mut display = match controller.initialize() {
        Ok(display) => display,
        Err(_) => hold(
            (controller, sd_chip_select),
            "display initialization failed",
        ),
    };
    info!("SSD1677 initialization and BUSY waits complete");

    if stage == "initialize" {
        hold(
            (display, sd_chip_select),
            "display initialization stage complete",
        );
    }

    if stage == "write" {
        if display.write_white_frame().is_err() {
            hold((display, sd_chip_select), "white RAM write failed");
        }
        info!("White frame written to both RAM planes without refresh");
        hold(
            (display, sd_chip_select),
            "display RAM write stage complete",
        );
    }

    if stage == "black" {
        if display.refresh_solid(0x00).is_err() {
            hold((display, sd_chip_select), "black full refresh failed");
        }
        info!("Black full refresh complete");
        hold((display, sd_chip_select), "black display stage complete");
    }

    if stage == "checkerboard" {
        if display.refresh_generated_frame(fill_checkerboard).is_err() {
            hold((display, sd_chip_select), "checkerboard refresh failed");
        }
        info!("Checkerboard full refresh complete");
        hold(
            (display, sd_chip_select),
            "checkerboard display stage complete",
        );
    }

    if stage == "orientation" {
        run_orientation(display, sd_chip_select, rotation);
    }

    if stage == "image" {
        run_image(display, sd_chip_select, rotation);
    }

    if display.refresh_white().is_err() {
        hold((display, sd_chip_select), "white full refresh failed");
    }
    info!("White full refresh complete");
    hold((display, sd_chip_select), "white display stage complete");
}

fn run_orientation<B, S>(
    mut display: InitializedSsd1677<'_, B>,
    retained: S,
    rotation: Rotation,
) -> !
where
    B: DisplayBus,
{
    if display
        .refresh_generated_frame(|offset, output| {
            fill_rotated_orientation(rotation, offset, output);
        })
        .is_err()
    {
        hold((display, retained), "orientation refresh failed");
    }
    info!(
        "Rotated orientation full refresh complete: degrees={}",
        rotation.degrees()
    );
    hold((display, retained), "orientation display stage complete");
}

fn run_image<B, S>(mut display: InitializedSsd1677<'_, B>, retained: S, rotation: Rotation) -> !
where
    B: DisplayBus,
{
    let frame = match Frame::new(BUILT_IMAGE, rotation) {
        Ok(frame) => frame,
        Err(_) => hold((display, retained), "built image frame is invalid"),
    };
    if display.refresh_logical_frame(frame).is_err() {
        hold((display, retained), "image refresh failed");
    }
    info!(
        "Image full refresh complete: width={} height={} rotation={}",
        frame.width(),
        frame.height(),
        frame.rotation().degrees()
    );
    hold((display, retained), "image display stage complete");
}

fn parse_rotation(value: &str) -> Option<Rotation> {
    match value {
        "0" => Some(Rotation::Degrees0),
        "90" => Some(Rotation::Degrees90),
        "180" => Some(Rotation::Degrees180),
        "270" => Some(Rotation::Degrees270),
        _ => None,
    }
}

fn hold<T>(_hardware: T, status: &str) -> ! {
    info!("{=str}; holding without retry", status);
    let delay = esp_hal::delay::Delay::new();
    loop {
        delay.delay_millis(5_000);
        info!("{=str}; still holding without retry", status);
    }
}

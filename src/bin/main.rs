#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use brewthink::{
    diagnostics::{DiagnosticStage, DisplayDiagnosticStage},
    display::{
        diagnostic::{fill_checkerboard, fill_rotated_orientation},
        framebuffer::{FRAME_BYTES, Frame, Rotation},
        ssd1677::{DisplayBus, InitializedSsd1677, Ssd1677},
    },
    input::{ButtonChanges, ButtonDebouncer, RawInputSample},
    power::{DisconnectedPowerCapture, PowerUsbEvent, PowerUsbTracker},
    storage::{
        CardInfo, DiskLayout, ReadOnlySdCard, SdError, Sector, inspect_filesystem,
        inspect_sector_zero, sector_fingerprint,
    },
    x4::{
        InputReadError, SharedSpiChipSelects, X4DisplayHardware, X4InputHardware,
        X4InputPeripherals, X4SharedSpiPeripherals, X4StorageError, X4StorageHardware,
        decode_buttons,
    },
};
#[cfg(feature = "sd-write-diagnostic")]
use brewthink::{
    storage::{TemporaryFileStore, create_verify_delete},
    x4::{X4FatBlockDevice, X4FatBlockDeviceError},
};
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
#[cfg(feature = "sd-write-diagnostic")]
use embedded_sdmmc::{
    Error as FilesystemError, Mode, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    delay::Delay,
    gpio::{Input, InputConfig, Pull, RtcPinWithResistors},
    peripherals::{GPIO3, LPWR},
    rtc_cntl::{
        Rtc,
        sleep::{RtcioWakeupSource, WakeupLevel},
    },
    system::SleepSource,
    timer::timg::TimerGroup,
};
use esp_println as _;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const DIAGNOSTIC_STAGE: &str = match option_env!("BREWTHINK_DIAGNOSTIC_STAGE") {
    Some(stage) => stage,
    None => "heartbeat",
};
const DISPLAY_ROTATION: &str = match option_env!("BREWTHINK_DISPLAY_ROTATION") {
    Some(rotation) => rotation,
    None => "270",
};
const BUILT_IMAGE: &[u8; FRAME_BYTES] =
    include_bytes!(concat!(env!("OUT_DIR"), "/brewthink-image.bin"));

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    initialize(spawner);
    core::future::pending().await
}

fn initialize(spawner: Spawner) {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let chip_selects = SharedSpiChipSelects::deselected(peripherals.GPIO21, peripherals.GPIO12);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

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

    let stage = match DIAGNOSTIC_STAGE.parse::<DiagnosticStage>() {
        Ok(stage) => stage,
        Err(_) => hold(chip_selects, "unknown diagnostic stage"),
    };
    info!("Diagnostic stage={=str}", stage.name());

    match stage {
        DiagnosticStage::Heartbeat => {
            info!(
                "No SPI, display reset, ADC, SD, GPIO13, radio, or flash-writing path initialized"
            );
            let task = match heartbeat(chip_selects) {
                Ok(task) => task,
                Err(_) => hold((), "heartbeat task allocation failed"),
            };
            spawner.spawn(task);
        }
        DiagnosticStage::InputsRaw | DiagnosticStage::InputsEvents | DiagnosticStage::PowerUsb => {
            start_input_diagnostic(
                spawner,
                stage,
                chip_selects,
                X4InputPeripherals::new(
                    peripherals.ADC1,
                    peripherals.GPIO0,
                    peripherals.GPIO1,
                    peripherals.GPIO2,
                    peripherals.GPIO3,
                    peripherals.GPIO20,
                ),
            )
        }
        DiagnosticStage::StorageReadOnly => {
            let hardware = match X4StorageHardware::new(
                X4SharedSpiPeripherals::new(
                    peripherals.SPI2,
                    peripherals.GPIO8,
                    peripherals.GPIO10,
                    peripherals.GPIO7,
                    peripherals.GPIO4,
                    peripherals.GPIO5,
                    peripherals.GPIO6,
                ),
                chip_selects,
            ) {
                Ok(hardware) => hardware,
                Err(_) => hold((), "SPI2 storage configuration failed"),
            };
            run_storage_readonly_diagnostic(hardware);
        }
        DiagnosticStage::StorageWriteTest => {
            #[cfg(not(feature = "sd-write-diagnostic"))]
            hold(chip_selects, "SD write diagnostic feature is disabled");

            #[cfg(feature = "sd-write-diagnostic")]
            {
                let hardware = match X4StorageHardware::new(
                    X4SharedSpiPeripherals::new(
                        peripherals.SPI2,
                        peripherals.GPIO8,
                        peripherals.GPIO10,
                        peripherals.GPIO7,
                        peripherals.GPIO4,
                        peripherals.GPIO5,
                        peripherals.GPIO6,
                    ),
                    chip_selects,
                ) {
                    Ok(hardware) => hardware,
                    Err(_) => hold((), "SPI2 storage configuration failed"),
                };
                run_storage_write_diagnostic(hardware);
            }
        }
        DiagnosticStage::IntegratedDevice => {
            let hardware = match X4StorageHardware::new(
                X4SharedSpiPeripherals::new(
                    peripherals.SPI2,
                    peripherals.GPIO8,
                    peripherals.GPIO10,
                    peripherals.GPIO7,
                    peripherals.GPIO4,
                    peripherals.GPIO5,
                    peripherals.GPIO6,
                ),
                chip_selects,
            ) {
                Ok(hardware) => hardware,
                Err(_) => hold((), "SPI2 integrated configuration failed"),
            };
            static INTEGRATED_HARDWARE: StaticCell<X4StorageHardware<'static>> = StaticCell::new();
            let hardware = INTEGRATED_HARDWARE.init(integrated_device_startup(hardware));
            static INTEGRATED_INPUTS: StaticCell<X4InputHardware> = StaticCell::new();
            let inputs = INTEGRATED_INPUTS.init_with(|| {
                X4InputPeripherals::new(
                    peripherals.ADC1,
                    peripherals.GPIO0,
                    peripherals.GPIO1,
                    peripherals.GPIO2,
                    peripherals.GPIO3,
                    peripherals.GPIO20,
                )
                .initialize()
            });
            let task = match integrated_device_task(hardware, inputs) {
                Ok(task) => task,
                Err(_) => hold((), "integrated-device task allocation failed"),
            };
            spawner.spawn(task);
        }
        DiagnosticStage::SleepWake => {
            let hardware = match X4StorageHardware::new(
                X4SharedSpiPeripherals::new(
                    peripherals.SPI2,
                    peripherals.GPIO8,
                    peripherals.GPIO10,
                    peripherals.GPIO7,
                    peripherals.GPIO4,
                    peripherals.GPIO5,
                    peripherals.GPIO6,
                ),
                chip_selects,
            ) {
                Ok(hardware) => hardware,
                Err(_) => hold((), "SPI2 sleep/wake configuration failed"),
            };
            run_sleep_wake_diagnostic(
                hardware,
                peripherals.GPIO3,
                peripherals.LPWR,
                esp_hal::system::wakeup_cause(),
            );
        }
        DiagnosticStage::Display(display_stage) => {
            let rotation = if display_stage.uses_rotation() {
                match parse_rotation(DISPLAY_ROTATION) {
                    Some(rotation) => rotation,
                    None => hold(chip_selects, "unknown display rotation"),
                }
            } else {
                Rotation::Degrees0
            };
            info!("Display rotation degrees={}", rotation.degrees());

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
            run_display_diagnostic(hardware, display_stage, rotation);
        }
    }
}

fn start_input_diagnostic(
    spawner: Spawner,
    stage: DiagnosticStage,
    chip_selects: SharedSpiChipSelects<'static>,
    input_peripherals: X4InputPeripherals,
) {
    let inputs = input_peripherals.initialize();

    match stage {
        DiagnosticStage::InputsRaw | DiagnosticStage::InputsEvents => {
            let heartbeat_task = match heartbeat(chip_selects) {
                Ok(task) => task,
                Err(_) => hold((), "heartbeat task allocation failed"),
            };
            spawner.spawn(heartbeat_task);

            let spawned = match stage {
                DiagnosticStage::InputsRaw => raw_input_task(inputs)
                    .map(|task| spawner.spawn(task))
                    .is_ok(),
                DiagnosticStage::InputsEvents => input_event_task(inputs)
                    .map(|task| spawner.spawn(task))
                    .is_ok(),
                _ => unreachable!(),
            };
            if !spawned {
                hold((), "input task allocation failed");
            }
        }
        DiagnosticStage::PowerUsb => {
            let retained = match retain_chip_selects(chip_selects) {
                Ok(task) => task,
                Err(_) => hold((), "chip-select retention task allocation failed"),
            };
            spawner.spawn(retained);

            let task = match power_usb_task(inputs) {
                Ok(task) => task,
                Err(_) => hold((), "power/USB task allocation failed"),
            };
            spawner.spawn(task);
        }
        DiagnosticStage::Heartbeat
        | DiagnosticStage::StorageReadOnly
        | DiagnosticStage::StorageWriteTest
        | DiagnosticStage::IntegratedDevice
        | DiagnosticStage::SleepWake
        | DiagnosticStage::Display(_) => unreachable!(),
    }
}

#[embassy_executor::task]
async fn retain_chip_selects(_chip_selects: SharedSpiChipSelects<'static>) -> ! {
    core::future::pending().await
}

#[embassy_executor::task]
async fn heartbeat(_chip_selects: SharedSpiChipSelects<'static>) -> ! {
    let mut heartbeat: u32 = 0;
    loop {
        info!("Brewthink heartbeat {}", heartbeat);
        heartbeat = heartbeat.wrapping_add(1);
        Timer::after(Duration::from_secs(1)).await;
    }
}

#[embassy_executor::task]
async fn raw_input_task(mut inputs: X4InputHardware) -> ! {
    info!("Raw input capture ready: ADC GPIO0/GPIO1/GPIO2, power GPIO3, USB detect GPIO20");
    info!("No SPI, display reset, SD protocol, GPIO13, radio, or flash-writing path initialized");

    let mut sequence = 0u32;
    loop {
        match inputs.sample().await {
            Ok(sample) => {
                info!(
                    "bench: input_raw seq={} battery_pin_mv={} battery_mv={} navigation_mv={} page_mv={} power_pressed={} usb_connected={}",
                    sequence,
                    sample.battery_pin_voltage.get(),
                    sample.battery_voltage().millivolts().get(),
                    sample.navigation_voltage.get(),
                    sample.page_voltage.get(),
                    sample.power_pressed,
                    sample.usb_state.is_connected(),
                );
            }
            Err(error) => {
                info!("bench: input_error source={=str}", error.name());
            }
        }
        sequence = sequence.wrapping_add(1);
        Timer::after(Duration::from_millis(100)).await;
    }
}

#[embassy_executor::task]
async fn input_event_task(mut inputs: X4InputHardware) -> ! {
    info!("Input event capture ready: sample_period_ms=20 debounce_samples=3");
    info!("No SPI, display reset, SD protocol, GPIO13, radio, or flash-writing path initialized");

    let mut debouncer = ButtonDebouncer::new();
    let mut sequence = 0u32;
    loop {
        match inputs.sample().await {
            Ok(sample) => match decode_buttons(sample) {
                Ok(observed) => {
                    if let Some(changes) = debouncer.update(observed) {
                        log_button_changes(sequence, changes);
                    }
                }
                Err(error) => {
                    debouncer.reject_sample();
                    info!(
                        "bench: button_unrecognized seq={} channel={=str} millivolts={}",
                        sequence,
                        error.channel_name(),
                        error.voltage().get()
                    );
                }
            },
            Err(error) => {
                debouncer.reject_sample();
                info!("bench: input_error source={=str}", error.name());
            }
        }
        sequence = sequence.wrapping_add(1);
        Timer::after(Duration::from_millis(20)).await;
    }
}

fn log_button_changes(sequence: u32, changes: ButtonChanges) {
    for event in changes.events() {
        info!(
            "bench: button_event seq={} button={=str} action={=str}",
            sequence,
            event.button().name(),
            event.transition().name()
        );
    }
}

#[embassy_executor::task]
async fn power_usb_task(mut inputs: X4InputHardware) -> ! {
    let mut tracker = PowerUsbTracker::new();
    let mut sequence = 0u32;
    loop {
        match inputs.sample().await {
            Ok(sample) => {
                if let Some(PowerUsbEvent::Reconnected(capture)) =
                    tracker.observe(sample.usb_state, sample.battery_voltage())
                {
                    log_disconnected_power_capture(capture);
                }
                if sample.usb_state.is_connected() && sequence.is_multiple_of(10) {
                    log_connected_power(sequence, sample, tracker.last_disconnected_capture());
                }
            }
            Err(error) => {
                esp_println::println!("bench: power_usb error={}", error.name());
            }
        }
        sequence = sequence.wrapping_add(1);
        Timer::after(Duration::from_millis(100)).await;
    }
}

fn log_connected_power(
    sequence: u32,
    sample: RawInputSample,
    disconnected: Option<DisconnectedPowerCapture>,
) {
    match disconnected {
        Some(capture) => esp_println::println!(
            "bench: power_usb state=connected seq={} battery_pin_mv={} battery_mv={} disconnected_samples={} battery_disconnected_min_mv={} battery_disconnected_max_mv={}",
            sequence,
            sample.battery_pin_voltage.get(),
            sample.battery_voltage().millivolts().get(),
            capture.samples(),
            capture.minimum().millivolts().get(),
            capture.maximum().millivolts().get()
        ),
        None => esp_println::println!(
            "bench: power_usb state=connected seq={} battery_pin_mv={} battery_mv={} disconnected_samples=0",
            sequence,
            sample.battery_pin_voltage.get(),
            sample.battery_voltage().millivolts().get()
        ),
    }
}

fn log_disconnected_power_capture(capture: DisconnectedPowerCapture) {
    esp_println::println!(
        "bench: power_usb event=reconnected disconnected_samples={} battery_min_mv={} battery_max_mv={}",
        capture.samples(),
        capture.minimum().millivolts().get(),
        capture.maximum().millivolts().get()
    );
}

#[embassy_executor::task]
async fn integrated_device_task(
    hardware: &'static mut X4StorageHardware<'static>,
    inputs: &'static mut X4InputHardware,
) -> ! {
    let mut debouncer = ButtonDebouncer::new();
    let mut sequence = 0_u32;
    loop {
        let sample = inputs.sample().await;
        process_integrated_input(hardware, &mut debouncer, sequence, sample);
        sequence = sequence.wrapping_add(1);
        Timer::after(Duration::from_millis(20)).await;
    }
}

fn process_integrated_input(
    hardware: &mut X4StorageHardware<'_>,
    debouncer: &mut ButtonDebouncer,
    sequence: u32,
    sample: Result<RawInputSample, InputReadError>,
) {
    match sample {
        Ok(sample) => match decode_buttons(sample) {
            Ok(buttons) => {
                if let Some(changes) = debouncer.update(buttons) {
                    for event in changes.events() {
                        esp_println::println!(
                            "bench: integrated_device event=button seq={} button={} action={}",
                            sequence,
                            event.button().name(),
                            event.transition().name()
                        );
                    }
                }
                if sequence.is_multiple_of(50) {
                    let chip_selects_high = hardware.both_are_deselected();
                    esp_println::println!(
                        "bench: integrated_device state=live seq={} battery_mv={} usb={} display_cs_high={} sd_cs_high={}",
                        sequence,
                        sample.battery_voltage().millivolts().get(),
                        sample.usb_state.name(),
                        chip_selects_high && hardware.display_is_deselected(),
                        chip_selects_high && hardware.sd_is_deselected()
                    );
                }
            }
            Err(error) => {
                debouncer.reject_sample();
                esp_println::println!(
                    "bench: integrated_device event=button_unrecognized seq={} channel={} millivolts={}",
                    sequence,
                    error.channel_name(),
                    error.voltage().get()
                );
            }
        },
        Err(error) => {
            debouncer.reject_sample();
            esp_println::println!(
                "bench: integrated_device status=failed phase=input_sample error={}",
                error.name()
            );
        }
    }
}

fn integrated_device_startup(hardware: X4StorageHardware<'static>) -> X4StorageHardware<'static> {
    static SECTOR: StaticCell<Sector> = StaticCell::new();
    let sector = SECTOR.init_with(Sector::zeroed);

    let (hardware, before_info, before_fingerprint) =
        integrated_sd_read(hardware, sector, "sd_before");
    esp_println::println!(
        "bench: integrated_device phase=sd_before_display capacity_bytes={} sector0_fingerprint=0x{:08x}",
        before_info.capacity_bytes(),
        before_fingerprint
    );

    let hardware = integrated_display_refresh(hardware);
    esp_println::println!("bench: integrated_device phase=display checkerboard_refresh=true");

    let (mut hardware, after_info, after_fingerprint) =
        integrated_sd_read(hardware, sector, "sd_after");
    if before_info != after_info || before_fingerprint != after_fingerprint {
        esp_println::println!(
            "bench: integrated_device status=failed phase=sd_compare before=0x{:08x} after=0x{:08x}",
            before_fingerprint,
            after_fingerprint
        );
        hold(hardware, "integrated SD comparison failed");
    }
    if !hardware.both_are_deselected() {
        hold(hardware, "integrated SPI chip selects remained active");
    }
    esp_println::println!(
        "bench: integrated_device phase=sd_after_display sector0_fingerprint=0x{:08x} unchanged=true",
        after_fingerprint
    );
    esp_println::println!(
        "bench: integrated_device status=ready display=checkerboard storage=verified sectors_written=0"
    );
    hardware
}

fn integrated_sd_read<'d>(
    hardware: X4StorageHardware<'d>,
    sector: &mut Sector,
    phase: &'static str,
) -> (X4StorageHardware<'d>, CardInfo, u32) {
    let mut card = ReadOnlySdCard::new(hardware);
    let info = match card.initialize() {
        Ok(info) => info,
        Err(error) => {
            esp_println::println!(
                "bench: integrated_device status=failed phase={}_initialize error={}",
                phase,
                error.name()
            );
            hold(card.into_bus(), "integrated SD initialization failed");
        }
    };
    if let Err(error) = card.read_sector(0, sector) {
        esp_println::println!(
            "bench: integrated_device status=failed phase={}_read error={}",
            phase,
            error.name()
        );
        hold(card.into_bus(), "integrated SD read failed");
    }
    let fingerprint = sector_fingerprint(sector);
    (card.into_bus(), info, fingerprint)
}

fn integrated_display_refresh<'d>(mut hardware: X4StorageHardware<'d>) -> X4StorageHardware<'d> {
    {
        let display_bus = match hardware.display_bus() {
            Ok(bus) => bus,
            Err(_) => hold((), "integrated display bus configuration failed"),
        };
        let mut controller = Ssd1677::new(display_bus);
        {
            let mut display = match controller.initialize() {
                Ok(display) => display,
                Err(_) => hold(controller, "integrated display initialization failed"),
            };
            if display.refresh_generated_frame(fill_checkerboard).is_err() {
                hold(display, "integrated checkerboard refresh failed");
            }
        }
        let _released_bus = controller.into_bus();
    }
    hardware
}

fn run_sleep_wake_diagnostic(
    hardware: X4StorageHardware<'static>,
    power: GPIO3<'static>,
    low_power: LPWR<'static>,
    wakeup_cause: SleepSource,
) -> ! {
    match wakeup_cause {
        SleepSource::Gpio => run_after_power_wake(hardware),
        _ => enter_power_button_sleep(hardware, power, low_power),
    }
}

fn enter_power_button_sleep(
    hardware: X4StorageHardware<'static>,
    mut power: GPIO3<'static>,
    low_power: LPWR<'static>,
) -> ! {
    let mut hardware = prepare_display_for_sleep(hardware);
    if !hardware.both_are_deselected() {
        hold(hardware, "sleep/wake SPI chip selects remained active");
    }

    {
        let power_input = Input::new(power.reborrow(), InputConfig::default().with_pull(Pull::Up));
        if power_input.is_low() {
            esp_println::println!(
                "bench: sleep_wake status=failed phase=pre_sleep power_released=false"
            );
            hold(
                power_input,
                "power button must be released before deep sleep",
            );
        }
    }

    esp_println::println!(
        "bench: sleep_wake state=entering_deep_sleep display=orientation display_sleep=true wake_gpio=3 wake_level=low display_cs_high=true sd_cs_high=true"
    );
    embedded_hal::delay::DelayNs::delay_ms(&mut Delay::new(), 500);

    let mut rtc = Rtc::new(low_power);
    let wakeup_pins: &mut [(&mut dyn RtcPinWithResistors, WakeupLevel)] =
        &mut [(&mut power, WakeupLevel::Low)];
    let power_wakeup = RtcioWakeupSource::new(wakeup_pins);
    rtc.sleep_deep(&[&power_wakeup]);
}

fn prepare_display_for_sleep<'d>(mut hardware: X4StorageHardware<'d>) -> X4StorageHardware<'d> {
    {
        let display_bus = match hardware.display_bus() {
            Ok(bus) => bus,
            Err(_) => hold((), "sleep/wake display bus configuration failed"),
        };
        let mut controller = Ssd1677::new(display_bus);
        let mut display = match controller.initialize() {
            Ok(display) => display,
            Err(_) => hold(controller, "sleep/wake display initialization failed"),
        };
        if display
            .refresh_generated_frame(|offset, output| {
                fill_rotated_orientation(Rotation::Degrees270, offset, output);
            })
            .is_err()
        {
            hold(display, "sleep/wake orientation refresh failed");
        }
        if display.enter_deep_sleep().is_err() {
            hold(controller, "SSD1677 deep sleep command failed");
        }
        let _released_bus = controller.into_bus();
    }
    hardware
}

fn run_after_power_wake(mut hardware: X4StorageHardware<'static>) -> ! {
    {
        let display_bus = match hardware.display_bus() {
            Ok(bus) => bus,
            Err(_) => hold((), "wake display bus configuration failed"),
        };
        let mut controller = Ssd1677::new(display_bus);
        {
            let mut display = match controller.initialize() {
                Ok(display) => display,
                Err(_) => hold(controller, "wake display initialization failed"),
            };
            if display.refresh_white().is_err() {
                hold(display, "wake white refresh failed");
            }
        }
        let _released_bus = controller.into_bus();
    }
    if !hardware.both_are_deselected() {
        hold(hardware, "wake SPI chip selects remained active");
    }
    esp_println::println!(
        "bench: sleep_wake state=awake cause=gpio3 display=white display_cs_high=true sd_cs_high=true"
    );
    hold(hardware, "sleep/wake diagnostic complete");
}

fn run_storage_readonly_diagnostic(hardware: X4StorageHardware<'_>) -> ! {
    let mut card = ReadOnlySdCard::new(hardware);
    let card_info = match card.initialize() {
        Ok(info) => info,
        Err(error) => {
            esp_println::println!(
                "bench: storage_readonly status=failed phase=initialize error={}",
                error.name()
            );
            hold(card.into_bus(), "read-only SD initialization failed");
        }
    };
    esp_println::println!(
        "bench: storage_readonly card_version={} card_type={} blocks={} capacity_bytes={}",
        card_info.version.name(),
        card_info.card_type.name(),
        card_info.block_count,
        card_info.capacity_bytes()
    );

    let sectors_read = match inspect_storage_layout(&mut card) {
        Ok(sectors_read) => sectors_read,
        Err((phase, error)) => {
            esp_println::println!(
                "bench: storage_readonly status=failed phase={} error={}",
                phase,
                error.name()
            );
            hold(card.into_bus(), "read-only storage inspection failed");
        }
    };

    let mut hardware = card.into_bus();
    let display_cs_high = hardware.display_is_deselected();
    let sd_cs_high = hardware.sd_is_deselected();
    let both_high = hardware.both_are_deselected();
    esp_println::println!(
        "bench: storage_readonly status=complete sectors_read={} sectors_written=0 display_cs_high={} sd_cs_high={}",
        sectors_read,
        display_cs_high && both_high,
        sd_cs_high && both_high
    );
    hold(hardware, "read-only storage diagnostic complete");
}

#[cfg(feature = "sd-write-diagnostic")]
const WRITE_TEST_FILE: &str = "BWTST001.TMP";
#[cfg(feature = "sd-write-diagnostic")]
const WRITE_TEST_PAYLOAD: &[u8] = b"Brewthink X4 microSD write verification\r\nversion=1\r\n";

#[cfg(feature = "sd-write-diagnostic")]
struct DiagnosticTimeSource;

#[cfg(feature = "sd-write-diagnostic")]
impl TimeSource for DiagnosticTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 7,
            zero_indexed_day: 29,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[cfg(feature = "sd-write-diagnostic")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriteTestFailure {
    Filesystem(&'static str),
    TemporaryFile(&'static str),
}

#[cfg(feature = "sd-write-diagnostic")]
impl WriteTestFailure {
    const fn name(self) -> &'static str {
        match self {
            Self::Filesystem(phase) | Self::TemporaryFile(phase) => phase,
        }
    }
}

#[cfg(feature = "sd-write-diagnostic")]
type WriteVolumeManager<'d> = VolumeManager<X4FatBlockDevice<'d>, DiagnosticTimeSource, 1, 1, 1>;

#[cfg(feature = "sd-write-diagnostic")]
fn run_storage_write_diagnostic(hardware: X4StorageHardware<'static>) -> ! {
    let mut card = ReadOnlySdCard::new(hardware);
    if let Err(error) = card.initialize() {
        esp_println::println!(
            "bench: storage_write_test status=failed phase=initialize error={}",
            error.name()
        );
        hold(card.into_bus(), "SD write-test initialization failed");
    }

    let device = X4FatBlockDevice::new(card.enable_write_diagnostic());
    static VOLUME_MANAGER: StaticCell<WriteVolumeManager<'static>> = StaticCell::new();
    let volume_manager = VOLUME_MANAGER
        .init_with(|| VolumeManager::new_with_limits(device, DiagnosticTimeSource, 7_000));
    let result = write_verify_delete(volume_manager);
    let (sectors_read, sectors_written, display_cs_high, sd_cs_high) =
        volume_manager.device(|device| {
            let (display_high, sd_high, both_high) = device.chip_select_states();
            (
                device.sectors_read(),
                device.sectors_written(),
                display_high && both_high,
                sd_high && both_high,
            )
        });

    match result {
        Ok(()) => esp_println::println!(
            "bench: storage_write_test status=complete file={} payload_bytes={} sectors_read={} sectors_written={} deleted=true display_cs_high={} sd_cs_high={}",
            WRITE_TEST_FILE,
            WRITE_TEST_PAYLOAD.len(),
            sectors_read,
            sectors_written,
            display_cs_high,
            sd_cs_high
        ),
        Err(error) => esp_println::println!(
            "bench: storage_write_test status=failed phase={} file={} sectors_read={} sectors_written={} display_cs_high={} sd_cs_high={}",
            error.name(),
            WRITE_TEST_FILE,
            sectors_read,
            sectors_written,
            display_cs_high,
            sd_cs_high
        ),
    }
    hold(volume_manager, "storage write diagnostic complete");
}

#[cfg(feature = "sd-write-diagnostic")]
#[cfg(feature = "sd-write-diagnostic")]
struct FatRootStore<'root, 'manager, 'device> {
    root: &'root embedded_sdmmc::Directory<
        'manager,
        X4FatBlockDevice<'device>,
        DiagnosticTimeSource,
        1,
        1,
        1,
    >,
}

#[cfg(feature = "sd-write-diagnostic")]
impl TemporaryFileStore for FatRootStore<'_, '_, '_> {
    type Error = FilesystemError<X4FatBlockDeviceError>;

    fn exists(&mut self, name: &str) -> Result<bool, Self::Error> {
        match self.root.find_directory_entry(name) {
            Ok(_) => Ok(true),
            Err(FilesystemError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn create(&mut self, name: &str, contents: &[u8]) -> Result<(), Self::Error> {
        let file = self.root.open_file_in_dir(name, Mode::ReadWriteCreate)?;
        file.write(contents)?;
        file.flush()?;
        file.close()
    }

    fn read(&mut self, name: &str, output: &mut [u8]) -> Result<usize, Self::Error> {
        let file = self.root.open_file_in_dir(name, Mode::ReadOnly)?;
        let length = file.length() as usize;
        if length != output.len() {
            file.close()?;
            return Ok(length);
        }
        let read = file.read(output);
        let close = file.close();
        close?;
        read
    }

    fn delete(&mut self, name: &str) -> Result<(), Self::Error> {
        self.root.delete_entry_in_dir(name)
    }
}

#[cfg(feature = "sd-write-diagnostic")]
fn write_verify_delete(volume_manager: &WriteVolumeManager<'_>) -> Result<(), WriteTestFailure> {
    let volume = volume_manager
        .open_volume(VolumeIdx(0))
        .map_err(|_| WriteTestFailure::Filesystem("open_volume"))?;
    let root = volume
        .open_root_dir()
        .map_err(|_| WriteTestFailure::Filesystem("open_root"))?;
    let mut store = FatRootStore { root: &root };
    let mut readback = [0_u8; WRITE_TEST_PAYLOAD.len()];
    let operation = create_verify_delete(
        &mut store,
        WRITE_TEST_FILE,
        WRITE_TEST_PAYLOAD,
        &mut readback,
    )
    .map_err(|error| WriteTestFailure::TemporaryFile(error.name()));
    let root_close = root
        .close()
        .map_err(|_| WriteTestFailure::Filesystem("close_root"));
    let volume_close = volume
        .close()
        .map_err(|_| WriteTestFailure::Filesystem("close_volume"));

    root_close?;
    volume_close?;
    operation
}

fn inspect_storage_layout(
    card: &mut ReadOnlySdCard<X4StorageHardware<'_>>,
) -> Result<u8, (&'static str, SdError<X4StorageError>)> {
    static SECTOR: StaticCell<Sector> = StaticCell::new();
    let sector = SECTOR.init_with(Sector::zeroed);
    card.read_sector(0, sector)
        .map_err(|error| ("read_sector_0", error))?;

    let layout = inspect_sector_zero(sector);
    log_storage_layout(layout, sector.as_bytes()[510..512] == [0x55, 0xAA]);
    let Some(partition) = layout.first_partition() else {
        return Ok(1);
    };
    if partition.type_code == 0xEE {
        esp_println::println!(
            "bench: storage_readonly volume_lba={} filesystem=protective_gpt",
            partition.first_lba
        );
        return Ok(1);
    }

    card.read_sector(partition.first_lba, sector)
        .map_err(|error| ("read_volume_boot", error))?;
    esp_println::println!(
        "bench: storage_readonly volume_lba={} filesystem={}",
        partition.first_lba,
        inspect_filesystem(sector).name()
    );
    Ok(2)
}

fn log_storage_layout(layout: DiskLayout, signature_valid: bool) {
    match layout {
        DiskLayout::MasterBootRecord {
            partitions,
            partition_count,
        } => {
            esp_println::println!(
                "bench: storage_readonly sector=0 signature_valid={} layout=mbr partitions={}",
                signature_valid,
                partition_count
            );
            log_storage_partitions(partitions);
        }
        DiskLayout::SuperFloppy(filesystem) => esp_println::println!(
            "bench: storage_readonly sector=0 signature_valid={} layout=superfloppy filesystem={}",
            signature_valid,
            filesystem.name()
        ),
        DiskLayout::Unknown => esp_println::println!(
            "bench: storage_readonly sector=0 signature_valid={} layout=unknown",
            signature_valid
        ),
    }
}

fn log_storage_partitions(partitions: [Option<brewthink::storage::Partition>; 4]) {
    for (index, partition) in partitions.into_iter().enumerate() {
        if let Some(partition) = partition {
            esp_println::println!(
                "bench: storage_readonly partition={} bootable={} type=0x{:02x} first_lba={} sectors={}",
                index,
                partition.bootable,
                partition.type_code,
                partition.first_lba,
                partition.sector_count
            );
        }
    }
}

fn run_display_diagnostic(
    hardware: X4DisplayHardware<'_>,
    stage: DisplayDiagnosticStage,
    rotation: Rotation,
) -> ! {
    let (mut epd_bus, sd_chip_select) = hardware.into_parts();
    info!("SPI2 ready: mode=0 frequency_mhz=20 SD_CS_high=true");

    if stage == DisplayDiagnosticStage::Reset {
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

    match stage {
        DisplayDiagnosticStage::Reset => unreachable!(),
        DisplayDiagnosticStage::Initialize => hold(
            (display, sd_chip_select),
            "display initialization stage complete",
        ),
        DisplayDiagnosticStage::Write => {
            if display.write_white_frame().is_err() {
                hold((display, sd_chip_select), "white RAM write failed");
            }
            info!("White frame written to both RAM planes without refresh");
            hold(
                (display, sd_chip_select),
                "display RAM write stage complete",
            );
        }
        DisplayDiagnosticStage::Refresh => {
            if display.refresh_white().is_err() {
                hold((display, sd_chip_select), "white full refresh failed");
            }
            info!("White full refresh complete");
            hold((display, sd_chip_select), "white display stage complete");
        }
        DisplayDiagnosticStage::Black => {
            if display.refresh_solid(0x00).is_err() {
                hold((display, sd_chip_select), "black full refresh failed");
            }
            info!("Black full refresh complete");
            hold((display, sd_chip_select), "black display stage complete");
        }
        DisplayDiagnosticStage::Checkerboard => {
            if display.refresh_generated_frame(fill_checkerboard).is_err() {
                hold((display, sd_chip_select), "checkerboard refresh failed");
            }
            info!("Checkerboard full refresh complete");
            hold(
                (display, sd_chip_select),
                "checkerboard display stage complete",
            );
        }
        DisplayDiagnosticStage::Orientation => run_orientation(display, sd_chip_select, rotation),
        DisplayDiagnosticStage::Image => run_image(display, sd_chip_select, rotation),
    }
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

use embassy_time::{Duration, Instant, Timer};
use esp_hal::{Blocking, delay::Delay, time::Rate, usb_serial_jtag::UsbSerialJtagRx};

use crate::display::grayscale_bench::{Command, CommandLines, paint};

use super::X4StorageHardware;

#[embassy_executor::task]
pub async fn grayscale_bench_task(
    mut hardware: X4StorageHardware<'static>,
    mut control: UsbSerialJtagRx<'static, Blocking>,
) {
    let mut lines = CommandLines::default();
    let mut attempts = 0u8;
    let mut faulted = false;
    let mut last_completed = None;
    let mut probe_attempt = None;
    esp_println::println!("BREWGRAY/1 READY version=3 spi_mhz=20 auto_refresh=0 sd_writes=0");

    loop {
        for _ in 0..128 {
            let Ok(byte) = control.read_byte() else {
                break;
            };
            let Some(command) = lines.push(byte) else {
                continue;
            };
            match command {
                Some(Command::Status) => {
                    esp_println::println!(
                        "BREWGRAY/1 STATUS version=3 attempts={} faulted={} probe_attempt={} spi_mhz=20 sd_writes=0",
                        attempts,
                        faulted,
                        probe_attempt.unwrap_or(0)
                    );
                    esp_println::println!("BREWGRAY/1 DONE command=status status=ok");
                }
                Some(Command::Paint(pattern)) => {
                    let cooling = last_completed
                        .is_some_and(|last| Instant::now() - last < Duration::from_secs(5));
                    let probe_used = pattern.is_probe() && probe_attempt.is_some();
                    if faulted || attempts >= 32 || cooling || probe_used {
                        esp_println::println!(
                            "BREWGRAY/1 DONE command={} status=blocked faulted={} limit={} cooling={} probe_used={}",
                            pattern.name(),
                            faulted,
                            attempts >= 32,
                            cooling,
                            probe_used
                        );
                        continue;
                    }
                    attempts += 1;
                    if pattern.is_probe() {
                        probe_attempt = Some(attempts);
                    }
                    esp_println::println!(
                        "BREWGRAY/1 START command={} attempt={}",
                        pattern.name(),
                        attempts
                    );
                    let start = Instant::now();
                    let result = match hardware.display_bus_at(Rate::from_mhz(20)) {
                        Ok(mut bus) => paint(&mut bus, &mut Delay::new(), pattern).map_err(|_| ()),
                        Err(_) => Err(()),
                    };
                    last_completed = Some(Instant::now());
                    faulted = result.is_err();
                    esp_println::println!(
                        "BREWGRAY/1 DONE command={} status={} attempt={} elapsed_ms={} cs_high={} optical=unmeasured",
                        pattern.name(),
                        if faulted { "error" } else { "ok" },
                        attempts,
                        start.elapsed().as_millis(),
                        hardware.both_are_deselected()
                    );
                }
                None => esp_println::println!("BREWGRAY/1 DONE command=parse status=invalid"),
            }
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

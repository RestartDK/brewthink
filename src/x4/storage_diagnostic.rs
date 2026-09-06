use embassy_time::{Duration, Timer};
use esp_hal::{Blocking, usb_serial_jtag::UsbSerialJtagRx};
use static_cell::ConstStaticCell;

use crate::storage::{
    ReadOnlySdCard, Sector,
    diagnostic::{MAX_READ_BYTES, SdDiagnosticCommand, SdDiagnosticLines, SectorRange},
};

use super::X4StorageHardware;

#[embassy_executor::task]
pub async fn storage_diagnostic_task(
    hardware: X4StorageHardware<'static>,
    mut control: UsbSerialJtagRx<'static, Blocking>,
) {
    static BUFFER: ConstStaticCell<[u8; MAX_READ_BYTES]> =
        ConstStaticCell::new([0; MAX_READ_BYTES]);
    let buffer = BUFFER.take();
    let mut card = ReadOnlySdCard::new(hardware);
    let mut lines = SdDiagnosticLines::default();
    esp_println::println!("BREWCTL/1 READY mode=sd-readonly version=1");

    loop {
        for _ in 0..128 {
            let Ok(byte) = control.read_byte() else {
                break;
            };
            let Some(command) = lines.push(byte) else {
                continue;
            };
            match command {
                Ok(SdDiagnosticCommand::Info) => {
                    let info = match card.card_info() {
                        Some(info) => Ok(info),
                        None => card.initialize(),
                    };
                    match info {
                        Ok(info) => {
                            esp_println::println!(
                                "BREWCTL/1 SDINFO blocks={} block_bytes=512 readonly=1",
                                info.block_count
                            );
                            esp_println::println!("BREWCTL/1 DONE command=sd-info status=ok");
                        }
                        Err(error) => report_error("sd-info", error.name()),
                    }
                }
                Ok(SdDiagnosticCommand::Read(range)) => {
                    match read_sectors(&mut card, range, buffer) {
                        Ok(()) => {
                            let bytes = &buffer[..range.byte_length()];
                            esp_println::println!(
                                "BREWCTL/1 SDREAD start={} count={} bytes={} crc32={:08x}",
                                range.start(),
                                range.count(),
                                bytes.len(),
                                crc32fast::hash(bytes)
                            );
                            esp_println::Printer::write_bytes(bytes);
                            esp_println::Printer::write_bytes(b"\n");
                            esp_println::println!("BREWCTL/1 DONE command=sd-read status=ok");
                        }
                        Err(reason) => report_error("sd-read", reason),
                    }
                }
                Err(_) => report_error("parse", "invalid-readonly-command"),
            }
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

fn read_sectors(
    card: &mut ReadOnlySdCard<X4StorageHardware<'static>>,
    range: SectorRange,
    buffer: &mut [u8; MAX_READ_BYTES],
) -> Result<(), &'static str> {
    let info = card.card_info().ok_or("not-initialized")?;
    if range.end() > info.block_count {
        return Err("out-of-range");
    }
    for (offset, block) in buffer[..range.byte_length()]
        .chunks_exact_mut(Sector::LEN)
        .enumerate()
    {
        let block: &mut [u8; Sector::LEN] = block.try_into().expect("chunks are one sector");
        card.read_block(range.start() + offset as u32, block)
            .map_err(|error| error.name())?;
    }
    Ok(())
}

fn report_error(command: &str, reason: &str) {
    esp_println::println!("BREWCTL/1 ERROR command={} reason={}", command, reason);
    esp_println::println!("BREWCTL/1 DONE command={} status=error", command);
}

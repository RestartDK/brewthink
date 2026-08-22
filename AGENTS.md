# AGENTS.md — Brewthink / Xteink X4 context

This repository is for learning embedded Rust by building custom firmware for a physical **Xteink X4 e-reader**. Treat the device as real hardware with a recoverable stock firmware that must not be destroyed accidentally.

## Non-negotiable safety rules

- Do **not** erase flash.
- Do **not** write to flash unless the user explicitly asks and the exact offset/size/target are reviewed.
- Do **not** burn eFuses or run any `espefuse burn...` command.
- Do **not** overwrite the bootloader, partition table, NVS, filesystem, or stock `app0` during early development.
- Do **not** use `cargo run` as a flashing shortcut. The generated runner has been disabled intentionally.
- Do **not** commit files under `backup/`, `.direnv/`, or `target/`.
- Do **not** commit raw flash dumps, NVS, filesystem extracts, MAC addresses, credentials, or private device identifiers.
- Explain hardware-changing commands before suggesting them.

## Physical device facts

Observed physical unit:

- Product: Xteink X4 e-reader
- SoC: ESP32-C3, QFN32, revision v0.4
- CPU target: `riscv32imc-unknown-none-elf`
- Crystal: 40 MHz
- SRAM: 400 KB total, less available to app after runtime/radio use
- PSRAM: none
- Wireless: Wi-Fi + BLE
- Display: Good Display GDEQ0426T82, 800 × 480 e-paper
- Display controller: SSD1677
- Storage: microSD over shared SPI
- USB: native ESP32-C3 USB Serial/JTAG

Security state:

- Secure Boot: disabled
- Flash Encryption: disabled
- SPI_BOOT_CRYPT_CNT: `0x0`
- USB recovery/download access works

External SPI NOR flash:

- JEDEC ID: `85 20 18`
- Manufacturer: Puya Semiconductor (`0x85`)
- Device: `0x2018`
- Capacity: 128 Mbit / 16 MiB
- Likely family: PY25Q128HA-compatible
- SFDP header: `53 46 44 50` (`SFDP`)
- Stock bootloader/app image headers use DIO at 80 MHz

The community schematic labels the flash as Winbond `W25Q128JVSIQTR`, but this physical device identifies as Puya-compatible. Firmware should rely on standard SPI NOR/SFDP behavior, not a hardcoded schematic vendor.

## Verified stock flash layout

A full 16 MiB stock dump exists privately under ignored `backup/` and has a verified SHA-256 checksum. A second private copy exists under `$HOME/X4-backups/brewthink-stock/`.

The partition table was extracted from flash offset `0x8000` and decoded. Its internal MD5 matches.

| Name       | Type/subtype          |     Offset |       Size | Range               |
| ---------- | --------------------- | ---------: | ---------: | ------------------- |
| `nvs`      | data / nvs            | `0x009000` | `0x005000` | `0x009000–0x00DFFF` |
| `otadata`  | data / ota            | `0x00E000` | `0x002000` | `0x00E000–0x00FFFF` |
| `app0`     | app / ota_0           | `0x010000` | `0x640000` | `0x010000–0x64FFFF` |
| `app1`     | app / ota_1           | `0x650000` | `0x640000` | `0x650000–0xC8FFFF` |
| `spiffs`   | data / spiffs subtype | `0xC90000` | `0x360000` | `0xC90000–0xFEFFFF` |
| `coredump` | data / coredump       | `0xFF0000` | `0x010000` | `0xFF0000–0xFFFFFF` |

Current stock contents:

- `app0` contains valid stock firmware and is selected by valid OTA sequence `1`.
- `app1` is fully erased and contains no valid image.
- `spiffs` partition is labeled as SPIFFS in the table, but on-flash metadata identifies LittleFS.
- `nvs` contains private data and must not be published.
- `coredump` is erased.

Important consequence:

- `app0` is the known-good stock slot.
- `app1` is the likely future development slot.
- Writing `app1` and changing `otadata` are separate operations and should stay separate until a safe workflow exists.

## Important GPIO / board mapping

Application/shared SPI bus:

- SPI2 SCLK: GPIO8
- SPI2 MOSI: GPIO10
- SD MISO: GPIO7
- Display CS: GPIO21, active low
- SD CS: GPIO12, active low

Display control:

- Display D/C: GPIO4
- Display reset: GPIO5
- Display busy: GPIO6

Inputs / power:

- Battery ADC: GPIO0, through 2 × 10 kΩ divider
- Button ADC ladder: GPIO1, Back/Confirm/Left/Right
- Button ADC ladder: GPIO2, Up/Down
- Power/wake button: GPIO3, active low
- USB/charging detect: GPIO20
- Possible X4 power-path hold/control: GPIO13; treat as board-reserved until verified

Reserved / preserve:

- GPIO11 / VDD_SPI: flash supply
- GPIO14: flash CS
- GPIO15: flash clock
- GPIO16: flash DI
- GPIO17: flash DO
- GPIO18/19: native USB D−/D+
- CHIP_EN: reset, not ordinary GPIO

Cautions:

- GPIO12 and GPIO13 are normally memory-SPI aliases (`SPIHD`/`SPIWP`) but are repurposed by this board because stock flash uses DIO.
- GPIO2, GPIO8, and GPIO9 are strapping pins.
- Initialize SD CS GPIO12 and display CS GPIO21 high before SPI traffic.
- Serialize access to shared SPI2 so display and SD transactions cannot interleave.

## Development environment

This repo uses a local Nix/Fenix development shell.

Important files:

- `flake.nix`
- `flake.lock`
- `.envrc`
- `rust-toolchain.toml`
- `.cargo/config.toml`

The shell provides:

- cargo/rustc/rust-analyzer 1.97.1
- rust-src, rustfmt, clippy
- target `riscv32imc-unknown-none-elf`
- espflash 4.5.0
- esptool 5.3.1

Use:

```bash
cd ~/Projects/brewthink
direnv allow   # first time or after .envrc changes
nix flake check
cargo check
```

`cargo check` is safe. `cargo run` must not flash the device; the generated runner was disabled intentionally.

## External references to inspect

Local docs first:

- `docs/notes.md`
- `docs/plan.md`
- `todo.md`

Known useful upstream references:

- MarigoldOS: X4-aware Rust firmware reference
- OpenX4 community SDK: display/controller reference
- Xteink X4 community schematic
- SSD1677 datasheet/guide
- ESP32-C3 datasheet and TRM
- ESP-IDF partition-table documentation

Use those projects as references and test oracles. Do not blindly port a whole SDK before Brewthink has working hardware-driven APIs.

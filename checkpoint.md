# Brewthink checkpoint

Last updated: 2026-08-30

## Active device-interaction implementation

Started after explicit user approval on 2026-08-30.

Current phase: integrated display/storage/input behavior is physically verified after correcting display command/D-C timing. The SSD1677 and ESP32-C3 sleep/wake diagnostic is locally verified and awaiting app1-write approval.

Scope:

1. Button ADC ladders on GPIO1/GPIO2 and the active-low power button on GPIO3.
2. Battery voltage on GPIO0 and USB detection on GPIO20.
3. Exclusive display/microSD access over shared SPI2.
4. Read-only SD bring-up, followed by a separately approved disposable-file write test.
5. Display, storage, and input integration.
6. SSD1677 sleep plus ESP32-C3 deep sleep and GPIO3 wake.

Out of scope until device interaction passes: OTA changes, Wi-Fi, BLE, reader formats, and GPIO13 experiments.

Safety and operator protocol:

- Build and host-test each diagnostic stage before touching hardware.
- Keep every firmware write inside guarded `app1` at `0x650000`.
- Print and review image size, byte range, and sector erase range before every write.
- Wait for the user before each physical action, including button presses, SD insertion/removal, USB changes, sleep/wake, and any removable-media write.
- Keep `app0`, bootloader, partition table, NVS, internal filesystem, and eFuses untouched.
- Keep GPIO13 high impedance and reserved.
- Preserve the user's existing uncommitted `todo.md` changes.

Planned domain shape:

```text
raw GPIO/ADC samples
    -> calibrated input and power types
    -> ButtonEvent / BatteryVoltage / UsbState
    -> DeviceEvent
    -> diagnostic app and later reader app

single SPI2 owner
    -> exclusive DisplaySession or SdSession
    -> no overlapping chip selects

Awake -> Quiescing -> PanelAsleep -> DeepSleep -> cold boot
```

Execution checklist:

- [x] Read the dstack principles and repository safety rules.
- [x] Ground the existing display, board, image, and app1 workflow.
- [x] Compare X4 reference implementations and official ESP32-C3 APIs.
- [x] Generalize the compile-time diagnostic stage without weakening display tests.
- [x] Fix the macOS Bash 3.2 WASM build regression and restore the full local baseline.
- [x] Add structured diagnostic records and raw GPIO0/GPIO1/GPIO2/GPIO3/GPIO20 capture.
- [x] Build and locally validate the first input diagnostic image.
- [x] Review the first app1 write range with the user before flashing.
- [x] Capture this physical unit's idle and per-button measurements.
- [x] Derive and host-test button bands and debounce behavior from captured measurements.
- [x] Physically verify battery voltage, USB transitions, battery-powered continuity, and reset-free serial reconnection.
- [x] Implement and host-test exclusive SPI2 selection plus a CRC-protected read-only SD protocol.
- [x] Physically verify read-only SD initialization, capacity, MBR, and FAT32 boot-sector access.
- [ ] Continue through the separately approved SD write test, integration, and sleep/wake one verified phase at a time.

Throughput checkpoint:

- **Blocking first steps.** Preserve user work, repair the green baseline, define typed stages, and build the raw-input diagnostic before hardware work.
- **Independent workstreams.** Host-pure input logic and hardware adapters use separate files, but the first diagnostic image integrates them serially.
- **Shared mutable state.** SPI2 will have one owner. Display and SD sessions will borrow it exclusively instead of sharing a runtime lock.
- **Smallest safe decomposition.** One implementation owner is required because `main.rs`, X4 peripheral ownership, build configuration, and the app1 image must change together.

Architect status: direct design sketch recorded above. Parallel architect runners were not used because the available subagent tool requires an explicit user request for delegation.

First raw-input image:

```text
image:              artifacts/brewthink-inputs-raw-app1.bin
sha256:             f66f567a5d53fa5de7bfeb4744d0f9dd9c202e6405a64de0ccfb343ec5b91c97
image size:         98,608 bytes
app1 partition:     0x650000..0xC8FFFF
write byte range:   0x650000..0x66812F
sector erase range: 0x650000..0x668FFF
```

Local verification passed with 34 host tests, formatting, host-image Clippy, embedded check and Clippy, WASM library check, guarded image inspection, `nix flake check`, the production web build, and all three Playwright tests.

The user approved the reviewed write. The guarded workflow wrote and read back exactly `0x650000..0x66812F`; both SHA-256 values matched the image hash above. The workflow issued no write command for `otadata` or any non-app1 partition. The matching release ELF was rebuilt after unrelated display-image smoke builds so defmt decoding uses the exact flashed symbol table.

Initial connected idle samples:

```text
battery pin:   2098..2101 mV
battery pack:  4196..4202 mV
navigation:    2973 mV
page:          2973 mV
power pressed: false
USB connected: true
```

The raw-input firmware remains in app1 with display and SD chip selects high. The serial monitor was stopped after the guided capture completed.

Raw button captures on this physical unit:

| Button | Channel | Pressed range | Other channel | Release baseline |
| --- | --- | ---: | ---: | ---: |
| Back | GPIO2 page ladder | `1654..1656 mV` | GPIO1 `2973 mV` | GPIO2 `2973 mV` |
| Confirm | GPIO2 page ladder | `3..4 mV` | GPIO1 `2973 mV` | GPIO2 `2973 mV` |
| Left | GPIO1 navigation ladder | `2562..2565 mV` | GPIO2 `2973 mV` | GPIO1 `2973 mV` |
| Right | GPIO1 navigation ladder | `1984..1986 mV` | GPIO2 `2973 mV` | GPIO1 `2973 mV` |
| Up | GPIO1 navigation ladder | `1108..1111 mV` | GPIO2 `2973 mV` | GPIO1 `2973 mV` |
| Down | GPIO1 navigation ladder | `2..4 mV` | GPIO2 `2973 mV` | GPIO1 `2973 mV` |
| Power/Wake | GPIO3 active-low digital | `true` for 11 samples | both ladders `2973 mV` | `false` |

The host-tested decoder uses bounded bands rather than nearest-value assignment:

```text
GPIO1 idle:  2800..3200 mV
GPIO1 left:  2400..2700 mV
GPIO1 right: 1800..2150 mV
GPIO1 up:     950..1250 mV
GPIO1 down:     0..150 mV

GPIO2 idle:    2800..3200 mV
GPIO2 back:    1500..1800 mV
GPIO2 confirm:    0..150 mV
```

Values in the gaps produce a structured `button_unrecognized` record and reset the debounce candidate. Valid states require three consecutive samples at 20 ms intervals. A stable transition produces exactly one typed press or release event.

First button-event image:

```text
image:              artifacts/brewthink-inputs-events-app1.bin
sha256:             f7a14e8dac28a35ae662a63704b4ae7bc7105485370afb2aeb8b4bf12ed3718b
image size:         99,360 bytes
app1 partition:     0x650000..0xC8FFFF
write byte range:   0x650000..0x66841F
sector erase range: 0x650000..0x668FFF
```

Local verification passed with 41 host tests, formatting, host-image Clippy, embedded check and Clippy, WASM library check, guarded image inspection, and `nix flake check`.

The user approved the button-event image. The guarded workflow wrote and read back exactly `0x650000..0x66841F`; the image and readback SHA-256 values both matched the hash above. The workflow issued no write command for `otadata` or any non-app1 partition. The stage booted with both chip selects high, no unrecognized idle samples, and no false button events before physical input.

Button-event captures:

| Physical action | Press events | Release events | Unrecognized events | Result |
| --- | ---: | ---: | ---: | --- |
| Back normal tap | 1 | 1 | 0 | Pass |
| Confirm normal tap | 1 | 1 | 0 | Pass |
| Left normal tap | 1 | 1 | 0 | Pass |
| Right normal tap | 1 | 1 | 0 | Pass |
| Up normal tap | 1 | 1 | 0 | Pass |
| Down normal tap | 1 | 1 | 0 | Pass |
| Power/Wake normal tap | 1 | 1 | 0 | Pass |

All seven physical controls produced exactly one typed press and release event per normal tap. No duplicate, false-idle, or unrecognized-voltage event was observed.

Durable battery/USB image:

```text
image:              artifacts/brewthink-power-usb-app1.bin
sha256:             2acb46979dd16df3cb55e223e99b8cb2c28b100604472953d2a336ae3d2fe6d1
image size:         99,600 bytes
app1 partition:     0x650000..0xC8FFFF
write byte range:   0x650000..0x66850F
sector erase range: 0x650000..0x668FFF
```

This stage emits plain structured `bench:` records so `tools/device-bench.py` can read without issuing reset or download-mode commands. While USB is absent, an in-RAM tracker counts samples and retains the observed minimum and maximum `BatteryVoltage`. Reconnection emits that summary. Losing battery power or resetting during the disconnected interval intentionally loses the summary and fails the test.

Local verification passed with 44 host tests, formatting, host-image Clippy, embedded check and Clippy, WASM library check, guarded image inspection, Python bytecode compilation for the collector, and `nix flake check`. A two-second read-only collector smoke test opened the current serial port without issuing a reset command.

The first battery/USB image was written and read back only within app1. After the collector race described below, the user approved the durable revision. The final guarded workflow wrote and read back exactly `0x650000..0x66850F`; the image and readback SHA-256 values both matched the durable hash above. No write command targeted `otadata` or another non-app1 partition.

The first disconnect made the serial device disappear as expected, but the original pyserial collector reset the chip while reopening the native USB Serial/JTAG port. Sequence restarted at zero and the in-RAM summary was lost, so that attempt is explicitly inconclusive rather than a pass. The collector now uses a POSIX read-only, nonblocking file descriptor and makes no serial-control-line calls. Opening the corrected collector preserved sequence continuity from the existing 700-series samples through sample 980.

The corrected collector then preserved sequence across a physical disconnect from sample 1300 to sample 1680, proving that Brewthink continued running from battery and that the read-only reopen did not reset it. The one-shot reconnection summary was emitted before macOS recreated and opened the serial device, so its battery range was not observable. This attempt passes battery-powered runtime continuity but does not yet pass disconnected-voltage capture.

The firmware now retains the last disconnected capture and repeats its sample count and voltage range in every connected status record. The user approved the revised image. The guarded workflow wrote and read back exactly `0x650000..0x66850F`; both SHA-256 values matched the revised hash above, and no write command targeted a non-app1 partition.

Final durable capture:

```text
last sample before disconnect: 1170
first observed after reconnect: 2530
disconnected samples:          791 at 100 ms intervals
battery-only duration:         about 79.1 seconds
battery-only minimum:          4172 mV
battery-only maximum:          4180 mV
connected after reconnect:     4202..4206 mV
```

This passes GPIO20 USB disconnect/reconnect detection, GPIO0 battery measurement through the 2:1 divider, continued execution from battery, non-resetting serial reopen, and durable summary reporting. macOS renamed the serial device from `/dev/cu.usbmodem1101` to `/dev/cu.usbmodem101` on final reconnect; the collector now follows a changed path when the requested path disappears and exactly one USB modem candidate is present. The serial collector was stopped after the capture.

Read-only microSD diagnostic image:

```text
image:              artifacts/brewthink-storage-readonly-app1.bin
sha256:             7b53272fced73dc7bf5d2b01b3d4244525b2405571659a40aea303ba4b138540
image size:         104,400 bytes
app1 partition:     0x650000..0xC8FFFF
write byte range:   0x650000..0x6697CF
sector erase range: 0x650000..0x669FFF
```

The shared-SPI owner drives both chip selects high before selecting either device, rejects overlapping sessions, and always deselects both at session end. Host tests prove an SD session deselects the display first and that a display session blocks SD selection. The X4 storage adapter owns SPI2 plus GPIO7/GPIO8/GPIO10 and GPIO12/GPIO21; GPIO13 remains untouched.

The SD protocol sends at least 80 idle clocks at 400 kHz, enters SPI mode, enables CRC, initializes SD v1/v2 cards, switches to 10 MHz, reads and CRC-checks the CSD, and exposes only single-sector reads. No block-write method or CMD24/CMD25 path exists. The diagnostic reads sector zero and, for an MBR card, the first partition's boot sector; it reports capacity, partition metadata, FAT/exFAT identification, `sectors_written=0`, and final chip-select levels.

Local verification passed with 57 host tests, formatting, host-image Clippy, embedded check and Clippy, WASM library check, the board-abstraction build, guarded image inspection, Python compilation, `git diff --check`, and `nix flake check`. No device flash or microSD content was changed while preparing this image.

The user approved the read-only storage image. The guarded workflow wrote and read back exactly `0x650000..0x6697CF`; image and readback SHA-256 values matched, and no write command targeted another flash partition. The physical diagnostic then completed successfully:

```text
card version:       SD v2
card type:          SDHC/SDXC
block count:        30,535,680
capacity:           15,634,268,160 bytes
sector 0:           valid MBR, one partition
partition 0:        type 0x0C, LBA 2048, 30,533,632 sectors
filesystem:         FAT32
sectors read:       2
sectors written:    0
display CS GPIO21:  high after completion
SD CS GPIO12:       high after completion
```

This physically passes SPI2 GPIO7/GPIO8/GPIO10 wiring, GPIO12 SD selection, GPIO21 display exclusion, SD initialization at 400 kHz, CRC-protected transfer at 10 MHz, CSD capacity parsing, sector reads, MBR parsing, and FAT32 boot-sector identification. The serial monitor was stopped after the one-shot diagnostic held without retry.

Disposable-file microSD write-test image:

```text
image:              artifacts/brewthink-storage-write-test-app1.bin
sha256:             bd3a06b72ca7b995d7b314e2cf31e4f8218cbc20be96b538ef36a877d7af0d32
image size:         124,320 bytes
app1 partition:     0x650000..0xC8FFFF
write byte range:   0x650000..0x66E59F
sector erase range: 0x650000..0x66EFFF
```

The user confirmed that the inserted card is disposable or fully backed up. The SD write capability is excluded from normal builds and enabled only by `sd-write-diagnostic`. The test targets root file `BWTST001.TMP`; it refuses to modify the card if that name already exists. Otherwise it creates the file with this exact 52-byte payload, flushes and closes it, reopens and verifies length and bytes, deletes it, then confirms absence:

```text
Brewthink X4 microSD write verification\r\n
version=1\r\n
```

If file creation or verification fails after preflight proved the target absent, the workflow still attempts deletion and reports cleanup failure or a remaining target. FAT32 may write the allocated data sector, both FAT copies, root-directory metadata, and free-space metadata. The diagnostic counts physical sector reads and writes and reports both chip selects after completion.

Local verification passed with 57 default host tests, 62 feature-enabled host tests, default and feature-enabled embedded Clippy, host-image Clippy, WASM check, guarded default/write-image inspections, Python compilation, `git diff --check`, and `nix flake check`.

The user approved the exact app1 range and removable-media operation. The guarded workflow wrote and read back exactly `0x650000..0x66E59F`; image and readback SHA-256 values matched, and no command targeted another flash partition. The physical write test then passed:

```text
file:              BWTST001.TMP
payload verified:  52 bytes
sectors read:      13
sectors written:   10
file deleted:      true
display CS GPIO21: high after completion
SD CS GPIO12:      high after completion
```

This physically passes CRC-protected CMD24 single-sector writes, FAT32 allocation and metadata updates, file flush/close, exact readback, deletion, and post-delete absence verification. No SD erase or multi-block-write command exists in the diagnostic. The serial monitor was stopped after the one-shot result. The write-test firmware remains in app1 and would repeat the temporary create/verify/delete cycle after another reset, so avoid unnecessary resets until it is replaced by the next reviewed image.

Integrated display/storage/input diagnostic image:

```text
image:              artifacts/brewthink-integrated-device-app1.bin
sha256:             0035be4e0b0ed00889c64494a69dfbeeef7d3b16b735af88d5dc780a657a65e4
image size:         111,968 bytes
app1 partition:     0x650000..0xC8FFFF
write byte range:   0x650000..0x66B55F
sector erase range: 0x650000..0x66BFFF
```

This default-feature image contains no SD block-write capability. One `X4StorageHardware` owner holds SPI2, both chip selects, and the SSD1677 control lines. Its typed display session mutably borrows the owner, so the SD protocol cannot run at the same time. The stage initializes the card and fingerprints sector zero, reconfigures the same SPI2 peripheral for a checkerboard full refresh, releases the display session, reinitializes the card, and requires sector zero plus card identity to remain unchanged. It then verifies both chip selects high and samples typed button, battery, and USB state every 20 ms.

Local verification passed with 58 default host tests, 63 feature-enabled host tests, default and feature-enabled embedded Clippy, host-image Clippy, WASM check, guarded default/write/integrated image inspections, Python compilation, `git diff --check`, and `nix flake check`. The matching integrated ELF was built last for defmt decoding.

The user approved the integrated image. The guarded workflow wrote and read back exactly `0x650000..0x66B55F`; image and readback SHA-256 values matched, and no command targeted another flash partition. The firmware reported a successful SD → display → SD handoff:

```text
sector-zero fingerprint before display: 0xC21BA993
checkerboard full refresh:               complete
sector-zero fingerprint after display:  0xC21BA993
sector-zero unchanged:                   true
microSD sectors written:                 0
live battery:                            4058..4060 mV
live USB:                                connected
display CS GPIO21:                       high
SD CS GPIO12:                            high
```

Physical observation disproved the firmware's display-success report: the panel retained its previous image rather than showing the checkerboard. SD fingerprinting, zero writes, live input sampling, and both final chip-select levels still passed. The monitor was stopped after recording the failure.

The failure was traced to a timing difference from the physically proven display adapter. The shared adapter queued its command byte and changed D/C high without first flushing SPI. The ESP SPI implementation may still have been transmitting the byte, causing command bytes followed by data to be sampled with data-level D/C. The corrected adapter flushes each command byte before changing D/C while keeping display CS selected. A new host test fixes the required write → flush → data → final flush ordering.

Corrected integrated image awaiting approval:

```text
image:              artifacts/brewthink-integrated-device-app1.bin
sha256:             b4f8427ccbb7b89cf3c101f0bed728a71d187fb19393591f8a542c8ca9318c60
image size:         112,016 bytes
app1 partition:     0x650000..0xC8FFFF
write byte range:   0x650000..0x66B58F
sector erase range: 0x650000..0x66BFFF
```

Local verification passes with 59 default host tests, 64 feature-enabled host tests, default and feature-enabled embedded Clippy, host-image Clippy, WASM check, guarded image inspection, and `git diff --check`. No SD write capability is compiled into the corrected integrated image.

The user approved the corrected integrated image. The guarded workflow wrote and read back exactly `0x650000..0x66B58F`; both SHA-256 values matched `b4f8427ccbb7b89cf3c101f0bed728a71d187fb19393591f8a542c8ca9318c60`, and no command targeted another flash partition. The physical checkerboard refresh passed. The second SD fingerprint remained `0xC21BA993`, zero sectors were written, and both chip selects remained high throughout live reporting. All seven controls then produced clean debounced press/release pairs while the integrated firmware remained live: Back, Confirm, Left, Right, Up, Down, and Power. The serial monitor was stopped after the final Power release.

SSD1677 and ESP32-C3 sleep/wake diagnostic image:

```text
image:              artifacts/brewthink-sleep-wake-app1.bin
sha256:             8e77081c7463e7584c12decc2bd9b9619e554b42208b4896d8aae7186796c561
image size:         103,792 bytes
app1 partition:     0x650000..0xC8FFFF
write byte range:   0x650000..0x66956F
sector erase range: 0x650000..0x669FFF
```

The SSD1677 deep-sleep API consumes the initialized display state and sends command `0x10` with Rev 1.0 check code `0x03`; it deliberately does not wait for BUSY low because BUSY remains high in deep sleep. The diagnostic refreshes the orientation pattern, enters display deep sleep, requires GPIO21 and GPIO12 high, checks that GPIO3 is released, then enters ESP32-C3 deep sleep after a 500 ms log-drain delay. Its only wake source is RTC-IO GPIO3 active low with pull-up. A GPIO wake takes the fresh-boot branch, hardware-resets the SSD1677 out of sleep, refreshes white, reports `cause=gpio3`, and holds without re-entering sleep. GPIO13 is untouched, no SD protocol runs, and no SD write capability is compiled.

Local verification passes with 60 default host tests, 65 feature-enabled host tests, default and feature-enabled embedded Clippy, host-image Clippy, WASM check, guarded default and sleep/wake image inspection, Python compilation, `git diff --check`, and `nix flake check`.

The user approved the sleep/wake image. The guarded workflow wrote and read back exactly `0x650000..0x66956F`; both SHA-256 values matched `8e77081c7463e7584c12decc2bd9b9619e554b42208b4896d8aae7186796c561`, and no command targeted another flash partition. The device then entered deep sleep before `espflash monitor` could reopen its vanished USB path. The orientation image was physically visible and retained while USB was absent. A brief Power press woke the device; the POSIX read-only collector reopened `/dev/cu.usbmodem101` without manipulating DTR, RTS, reset, or download mode and captured:

```text
bench: sleep_wake state=awake cause=gpio3 display=white display_cs_high=true sd_cs_high=true
```

The display physically refreshed completely white. This passes SSD1677 deep-sleep entry, e-paper image retention, ESP32-C3 deep sleep, GPIO3 active-low RTC-IO wake, reset-free USB re-enumeration, hardware-reset exit from display deep sleep, post-wake refresh, and final shared-SPI deselection. The collector was stopped after the confirmation. The sleep/wake image remains in app1, awake and holding after the GPIO-wake branch; an ordinary future reset will repeat its orientation → deep-sleep sequence.

## Current high-level objective

Build custom Rust firmware for the Xteink X4 e-reader while learning embedded development safely.

The first major product milestone is:

> Boot Brewthink on the X4, log reliably, initialize the e-paper display safely, and show a known raw picture while preserving stock recovery.

Longer-term direction:

1. Board abstraction for the Xteink X4.
2. SSD1677 display module.
3. Raw image rendering.
4. PNG/JPEG image rendering if memory allows.
5. Buttons and battery controller.
6. Local image browser.
7. Sleep/wake support.
8. Offline-first library app.
9. Reading formats: text/CBZ first, EPUB later, PDF only after feasibility review.
10. Wi-Fi/BLE, file transfer, Komga/OPDS sync later.

## Completed groundwork

### Hardware recovery and validation

- Full 16 MiB stock flash backup was read successfully.
- Backup is stored privately under ignored `backup/`.
- Backup SHA-256 verifies.
- Second private verified copy exists at:

  ```text
  $HOME/X4-backups/brewthink-stock/
  ```

- Stock partition table was extracted from offset `0x8000`.
- Partition table MD5 validated.
- Decoded partition CSV committed at:

  ```text
  docs/x4-stock-partition-table.csv
  ```

### Verified physical X4 facts

- SoC: ESP32-C3, revision v0.4
- Crystal: 40 MHz
- Flash: 16 MiB external SPI NOR
- Flash JEDEC: `85 20 18`, Puya-compatible, likely PY25Q128HA family
- SFDP header: `53 46 44 50` (`SFDP`)
- Secure Boot: disabled
- Flash Encryption: disabled
- USB Serial/JTAG recovery works

### Verified stock flash layout

| Name | Offset | Size | Status |
| --- | ---: | ---: | --- |
| `nvs` | `0x009000` | `0x005000` | Contains private settings/data |
| `otadata` | `0x00E000` | `0x002000` | Valid OTA sequence selecting `app0` |
| `app0` | `0x010000` | `0x640000` | Valid stock firmware |
| `app1` | `0x650000` | `0x640000` | Empty/erased; likely future dev slot |
| `spiffs` | `0xC90000` | `0x360000` | Partition label says SPIFFS; contents appear LittleFS |
| `coredump` | `0xFF0000` | `0x010000` | Empty/erased |

### Development environment

- Project has local Fenix/Nix shell:

  ```text
  flake.nix
  flake.lock
  .envrc
  rust-toolchain.toml
  ```

- Toolchain validates:

  ```text
  cargo/rustc/rust-analyzer 1.97.1
  espflash 4.5.0
  esptool 5.3.1
  target: riscv32imc-unknown-none-elf
  ```

- `nix flake check` passes.
- `cargo check` passes.
- Generated `cargo run` flashing runner has been disabled in `.cargo/config.toml`.

### Documentation

Important files:

- `AGENTS.md` — agent-facing project context and safety rules.
- `todo.md` — milestone roadmap.
- `docs/notes.md` — detailed X4 hardware/flash/SPI/device notes.
- `docs/board-info.txt` — redacted board info.
- `docs/x4-stock-partition-table.csv` — verified partition table.
- `docs/plan.md` — initial high-level safety plan.

## Safe app1 workflow status

Completed on 2026-08-22:

- Added guarded app1 build/check/probe/write-readback scripts under `scripts/`.
- Built a logging-only Brewthink app image locally.
- Wrote that image only to `app1` at `0x650000`.
- Read back the same `95,616` bytes and verified SHA-256 matched:

  ```text
  45c97d1090721837af57603a0a72252295157537418a0168d1db61c24bee9630
  ```

- Verified written byte range:

  ```text
  0x650000..0x66757F
  ```

- `otadata` was not modified; stock `app0` should remain selected.
- Bootloader, partition table, NVS, stock `app0`, filesystem, and coredump were not written by the script.
- Raw probe/write logs are under ignored `artifacts/` and may include private identifiers; do not commit them.
- Backed up current `otadata` read-only to ignored private path:

  ```text
  backup/otadata/otadata-20260822-170811.bin
  ```

  SHA-256:

  ```text
  f94c5d786a7a8fab06ac5d10e33bf37711a6697636dc037559ea19cc410a17f0
  ```

  Decoded summary: sector 0 has valid `seq=1 -> app0`; sector 1 is empty/unselected.
- Wrote `otadata` sector 1 only (`0xF000..0xFFFF`) with valid `seq=2 -> app1`, then read back and verified SHA-256:

  ```text
  f5ab818d800af4e3e1efd4c91587ff9019dfc243adb0bc2ef0c7eb4a0c9df16d
  ```

- Reset/monitored app1 with `espflash monitor --log-format defmt --elf target/riscv32imc-unknown-none-elf/release/brewthink` and confirmed logs:

  ```text
  Brewthink logging-only firmware booted: version=0.1.0
  No Wi-Fi, BLE, display, SD, GPIO13, or flash-writing path initialized
  Brewthink heartbeat 0
  Brewthink heartbeat 1
  Brewthink heartbeat 2
  ```

- Current boot selection after this test is `app1`. To restore exact previous stock `app0` selection:

  ```bash
  ESPFLASH_PORT=/dev/cu.usbmodem3101 scripts/restore-otadata.sh
  ```

- Added and hardware-verified guarded restore/cleanup scripts:

  ```text
  scripts/restore-otadata.sh      # verified: wrote/read back only 0xE000..0xFFFF from otadata backup
  scripts/restore-stock-app0.sh   # verified: wrote/read back only stock app0 from private full-flash backup slice
  scripts/erase-app1.sh           # verified: erased only app1 and read back all 0xFF
  scripts/restore-stock-state.sh  # verified: restored app0, erased app1, restored otadata; no whole-flash write
  ```

- Restore drill results:

  ```text
  stock app0 restored sha256: 2b922b52891da078ab7dbe06b5686231c02be7a689ea80a7fd35b17ae389a9f6
  app1 erased verification:   all bytes read back as 0xFF
  otadata restored sha256:    f94c5d786a7a8fab06ac5d10e33bf37711a6697636dc037559ea19cc410a17f0
  ```

- After restore verification, Brewthink was written back to app1 and `otadata` was switched back to `seq=2 -> app1`:

  ```text
  app1 Brewthink sha256:       45c97d1090721837af57603a0a72252295157537418a0168d1db61c24bee9630
  otadata sector 1 sha256:     f5ab818d800af4e3e1efd4c91587ff9019dfc243adb0bc2ef0c7eb4a0c9df16d
  final boot logs confirmed:   Brewthink heartbeat from app1
  ```

## Board abstraction status

Completed on 2026-08-30:

- Added a host-testable X4 pin model under `src/x4/spec.rs`.
- Added the ESP-only `SharedSpiChipSelects` adapter under `src/x4/board.rs`.
- The adapter accepts only `GPIO21` for display CS and `GPIO12` for SD CS, initializes both high, and retains ownership.
- Removed unused radio/network dependencies and restricted ESP dependencies to the RISC-V target so host tests do not compile hardware adapters.
- Added `scripts/check-board-abstraction.sh` and a host-test CI job.
- Four host pin-map and safety-invariant tests pass.
- Embedded check, Clippy, release image generation, and image validation pass.
- Wrote the new image only to app1 and verified readback:

  ```text
  image size:   93,680 bytes
  write range:  0x650000..0x666DEF
  SHA-256:      311f7eb27aebc6f17e5c28d721161457661eba1f8bc2a639d0a77acb8e6ca8e8
  ```

- Hardware monitor confirmed:

  ```text
  X4 display CS GPIO21 high=true
  X4 SD CS GPIO12 high=true
  Brewthink heartbeat 0
  Brewthink heartbeat 1
  ```

- The default build still leaves SPI, display reset, SD protocol, GPIO13, radio, and firmware flash-writing paths uninitialized.

## Display driver and bring-up status

Completed on 2026-08-30:

- Added the portable `src/display/bus.rs` and `src/display/ssd1677.rs` modules on `embedded-hal` traits.
- Added the typed ESP32-C3 adapter in `src/x4/display.rs` for SPI2, GPIO8/GPIO10, and GPIO4/GPIO5/GPIO6 while retaining GPIO12 high.
- Reconciled the 800 × 480 initialization and full-refresh sequences against OpenX4, MarigoldOS, and the SSD1677 datasheet.
- Added a byte-for-byte initialization transcript test, reset timing test, active-high BUSY timeout tests, SPI failure/deselect test, frame-size validation, exact two-plane transfer tests, and no-activation RAM-stage test.
- Added heap-free white, black, checkerboard, and labeled orientation patterns using a 256-byte transfer buffer.
- Added public `Frame` and `Rotation` types supporting 0°/90°/180°/270° with logical dimensions derived from rotation.
- Set the default to 270° after physical inspection found the first 90° portrait result upside down.
- Added `wasm32-unknown-unknown` to the toolchain and CI; the portable library checks successfully for WASM.
- Documented the command contract and stages in `docs/display-bringup.md`.
- Host tests, embedded check, WASM check, Clippy, image checks, and `nix flake check` pass.

The physical X4 completed these guarded stages without SPI errors, BUSY timeouts, panics, or retries:

1. Hardware reset only.
2. Initialization and automatic RAM clears.
3. Two explicit 48,000-byte white RAM-plane writes without activation.
4. White full refresh.
5. Black full refresh.
6. 40 × 40-pixel checkerboard full refresh.
7. Native border/crosshair and `TL`/`TR`/`BL`/`BR` orientation full refresh.
8. 90° portrait full refresh; human inspection found it upside down.
9. Corrected 270° portrait full refresh, visually confirmed upright.

The corrected orientation image was written and read back only in `app1`:

```text
image size:   100,128 bytes
write range:  0x650000..0x66871F
SHA-256:      cb81a48ed2ffe96e379d3c70d339083b46c6b65db5b2d22f41ca8a1ef2bac890
```

## Image pipeline status

Completed on 2026-08-30:

- Added the `no_std` `src/image/` module shared by host, WASM, and firmware builds.
- Added `contain` and `cover` scaling, bilinear sampling, integer luma conversion, fixed thresholding, and 4 × 4 ordered dithering.
- Added the host-only `prepare-image` binary for JPEG, PNG, BMP, and PNM decoding. Full RGB decode data never enters X4 RAM.
- Added an `image` firmware stage that validates and embeds a prepared 48,000-byte frame in mapped flash, then streams it through the existing 256-byte transfer buffer.
- Decoded the progressive 720 × 720 `anime-girl.jpeg` sample and generated a 480 × 800 PBM preview.
- Built, wrote, and read back the sample image only in `app1`:

  ```text
  image size:   99,552 bytes
  write range:  0x650000..0x6684DF
  SHA-256:      66b76caf888a0b6c6239f516a006239f0d44dfd55bf98ebafcb672cc3fc18a25
  ```

- The write verification passed. The immediate readback connection timed out, then a separate read-only retry matched the image SHA-256.
- The controller completed one full image refresh and held without retry.

Current device state:

- `app0` still contains verified stock firmware.
- `app1` contains the decoded JPEG diagnostic.
- `otadata` remains at its pre-bring-up `seq=2 -> app1` state.
- Human inspection confirmed the corrected 270° labels are upright and correctly placed.
- Human inspection of the decoded sample's rotation and dither quality is pending.

## Current uncommitted work

The implementation through Rust/WASM hot reload is committed on `safe-app1-workflow`. The user has an existing uncommitted `todo.md` edit. Preserve it exactly. Device-interaction work and this checkpoint update remain uncommitted until the user asks for commits.

Before committing, run:

```bash
git status --short
git diff --check
rg --pcre2 -n '[0-9a-f]{2}(:[0-9a-f]{2}){5}' AGENTS.md checkpoint.md todo.md docs || true
```

Do not commit anything under `backup/`, `.direnv/`, `target/`, or `artifacts/`.

## Safety rules to preserve

- Do not erase the full flash or any stock/recovery partition.
- Do not write flash without explicit user approval and reviewed offset/size.
- Do not burn eFuses.
- Do not overwrite bootloader, partition table, NVS, filesystem, or stock `app0`.
- Treat `app0` as the known-good stock recovery slot.
- Keep app1 writes and `otadata` changes as separate reviewed operations.
- Keep `backup/` and hardware probe output private and ignored.

## Next steps

1. Restore a fully green local baseline.
2. Add typed diagnostic-stage selection and raw input capture.
3. Build and validate the first app1-only input diagnostic image.
4. Review the hardware write with the user, then calibrate every physical button.
5. Derive button events before proceeding to battery, USB, SD, and sleep.

## Definition of the next successful checkpoint

The next checkpoint is complete when the input diagnostic image passes local checks, its app1 range is reviewed and written with explicit approval, and idle plus per-button readings from this physical unit are captured without touching any other flash partition or GPIO13.

# Brewthink checkpoint

Last updated: 2026-08-22

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

Current device state:

- `app0` still contains verified stock firmware.
- `app1` contains the orientation diagnostic.
- `otadata` remains at its pre-bring-up `seq=2 -> app1` state.
- The controller reported orientation-refresh completion and is holding without retry.
- Human inspection confirmed the corrected 270° labels are upright and correctly placed.

## Current uncommitted work

The board abstraction and display-driver work remain uncommitted. Preserve the unrelated edits already present in `docs/notes.md` and the untracked `docs/thoughts.md` file.

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

1. Display the build-time decoded JPEG sample from guarded `app1`.
2. Add a host/WASM display backend around the shared packed image type.
3. Design bounded-memory runtime decoding before reading images from microSD.
4. Keep partial refresh, custom LUTs, display sleep, SD, GPIO13, and radio out of scope until full-refresh image rendering is stable.

## Definition of the next successful checkpoint

The next checkpoint is complete when a decoded source image renders through the same 1-bit pipeline on the build host and the X4 full-refresh backend.

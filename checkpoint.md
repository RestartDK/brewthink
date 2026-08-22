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

## Current uncommitted work

At the time this checkpoint was created, these files were expected to be uncommitted:

```text
AGENTS.md
checkpoint.md
todo.md
```

Before committing, run:

```bash
git status --short
git diff
rg --pcre2 -n '[0-9a-f]{2}(:[0-9a-f]{2}){5}' AGENTS.md checkpoint.md todo.md docs || true
```

Do not commit anything under:

```text
backup/
.direnv/
target/
```

## Safety rules to preserve

- Do not erase flash.
- Do not write flash without explicit user approval and reviewed offset/size.
- Do not burn eFuses.
- Do not overwrite bootloader, partition table, NVS, filesystem, or stock `app0`.
- Do not modify `otadata` until a tested `app1` image and recovery workflow exist.
- Treat `app0` as the known-good stock recovery slot.
- Treat `app1` as the future development slot, but only after safe write/readback tooling exists.
- Keep `backup/` private and ignored.

## Next step: reference study before hardware writes

The next major work item is **reference study and planning**, not flashing.

Study MarigoldOS/OpenX4 to define the minimum X4-safe first firmware:

1. Firmware entry point and initialization.
2. X4 pin ownership and GPIO setup.
3. Power-hold behavior, especially GPIO13.
4. Display CS GPIO21 and SD CS GPIO12 initialization.
5. SSD1677 reset/init/BUSY/full-refresh flow.
6. Shared SPI handling between display and SD.
7. ESP-IDF-compatible app image creation.
8. OTA slot/update strategy and recovery assumptions.

Reference paths already identified:

```text
/tmp/pi-github-repos/Jon-Vii/marigold-os/fw/src/main.rs
/tmp/pi-github-repos/Jon-Vii/marigold-os/fw/src/display_flush/ssd1677.rs
/tmp/pi-github-repos/Jon-Vii/marigold-os/fw/src/sd_session.rs
/tmp/pi-github-repos/Jon-Vii/marigold-os/fw/src/ota_update.rs
/tmp/pi-github-repos/Jon-Vii/marigold-os/app-core/src/lib.rs
/tmp/pi-github-repos/Jon-Vii/marigold-os/tools/emulator/README.md
/tmp/pi-github-repos/open-x4-epaper/community-sdk/libs/display/EInkDisplay/doc/SSD1677_GUIDE.md
```

## Handoff for the next session/agent

Start here:

1. Read these local files first:

   ```text
   AGENTS.md
   checkpoint.md
   todo.md
   docs/notes.md
   docs/plan.md
   ```

2. Verify the development environment:

   ```bash
   cd /Users/danielkumlin/Projects/brewthink
   direnv reload
   nix flake check
   cargo check
   ```

3. Confirm private files are still ignored:

   ```bash
   git status --short --ignored | head -80
   ```

4. If desired, commit the current documentation state:

   ```bash
   git add AGENTS.md checkpoint.md todo.md
   git diff --cached --check
   git commit -m "Add project checkpoint and agent roadmap"
   ```

5. Create a new branch for the next phase:

   ```bash
   git switch -c x4-reference-study
   ```

6. Study MarigoldOS/OpenX4 without flashing anything.

7. Write a short plan in `docs/plan.md` for the **minimal logging-only X4 firmware**, including:

   - Which modules to create first.
   - Which pins must be initialized first.
   - What not to initialize yet.
   - How the first app image will eventually be built and inspected.
   - How `app1` will eventually be written and verified without changing `otadata`.

8. Do **not** flash the X4 yet.

## Definition of the next successful checkpoint

The next checkpoint is complete when:

- MarigoldOS/OpenX4 reference behavior is summarized.
- `docs/plan.md` explains the minimal logging-only firmware architecture.
- The first Brewthink firmware branch is ready to implement logging and board initialization.
- No hardware write has happened yet.

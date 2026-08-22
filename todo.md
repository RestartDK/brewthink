# Brewthink firmware roadmap

This is an Xteink X4-only learning project. The goal is to understand the hardware and gradually build custom Rust firmware without destroying the known recovery path.

## Non-negotiable safety rules

- [ ] Do **not** run `cargo run` against the X4 yet. The current project is a generic `esp-generate` project and its `espflash flash` runner may use a generic bootloader or partition layout.
- [ ] Do not run `espflash erase-flash` as part of normal development.
- [ ] Do not run any `espefuse burn...` command.
- [ ] Preserve the X4 bootloader, partition table, OTA metadata, and at least one known-good application slot until their behavior is understood.
- [ ] Make one small change per commit and per hardware flash.
- [ ] Never treat the persistent e-paper image as proof that the CPU is currently running; use USB logs or another explicit heartbeat.

## Current verified facts

- [x] The X4 enumerates as `/dev/cu.usbmodem101` through native ESP USB Serial/JTAG.
- [x] ESP32-C3 revision v0.4 detected.
- [x] 40 MHz crystal detected.
- [x] 16 MiB external flash detected.
- [x] Wi-Fi and BLE hardware detected.
- [x] Secure Boot is disabled.
- [x] Flash Encryption is disabled.
- [x] USB recovery/download access works.
- [x] A valid stock backup exists under ignored `backup/` and has been verified as exactly 16,777,216 bytes.
- [x] The backup SHA-256 file verifies successfully.
- [x] A second private backup copy exists at `$HOME/X4-backups/brewthink-stock/` and verifies successfully.
- [x] The actual stock partition table has been extracted and decoded from this physical X4.
- [x] The physical SPI NOR flash has been identified as Puya JEDEC `85 20 18`, consistent with the PY25Q128HA family.

---

## Phase 1 — Create and verify the recovery material

Nothing should be written to the X4 until this phase is complete.

### 1.1 Keep backups private and out of Git

- [x] Keep the working copy of the stock dump under the repository's ignored `backup/` directory.
- [x] Add `/backup/` to `.gitignore` so raw dumps and binary extracts cannot be committed accidentally.
- [x] Store a second private verified copy at `$HOME/X4-backups/brewthink-stock/`.
- [x] Never publish a raw stock dump. It may contain proprietary firmware, Wi-Fi credentials, settings, history, and unique device data.

### 1.2 Read the complete 16 MiB flash

- [x] Re-check the current serial path because it may change after reconnecting:

  ```bash
  espflash list-ports
  ```

- [x] Save a redacted hardware report:

  ```bash
  PORT="/dev/cu.usbmodem101"

  espflash board-info \
    --chip esp32c3 \
    --port "$PORT" \
    2>&1 | tee board-info.txt
  ```

- [x] Read every flash byte from address `0x0` for length `0x1000000`:

  ```bash
  espflash read-flash \
    --chip esp32c3 \
    --port "$PORT" \
    0x0 \
    0x1000000 \
    x4-stock-full-16MiB.bin
  ```

- [ ] If the read fails, stop and understand the exact error. Do not erase or write anything. Only consider retrying with `--no-stub` after reviewing the failure.

### 1.3 Verify and duplicate the backup

- [x] Confirm the exact byte count:

  ```bash
  wc -c < x4-stock-full-16MiB.bin
  ```

  It must print:

  ```text
  16777216
  ```

- [x] Create and verify a SHA-256 fingerprint:

  ```bash
  shasum -a 256 x4-stock-full-16MiB.bin \
    | tee x4-stock-full-16MiB.bin.sha256

  shasum -a 256 -c x4-stock-full-16MiB.bin.sha256
  ```

- [x] Store the `.bin` and `.sha256` files in at least two private locations.
- [ ] Keep the full restore command documented, but do not test it merely for practice:

  ```bash
  espflash write-bin \
    --chip esp32c3 \
    --port "$PORT" \
    0x0 \
    x4-stock-full-16MiB.bin
  ```

### 1.4 Decode the actual partition table

- [x] Extract the standard 4 KiB partition-table sector at flash offset `0x8000` from the local backup:

  ```bash
  dd \
    if=x4-stock-full-16MiB.bin \
    of=x4-stock-partition-table.bin \
    bs=1 \
    skip=$((0x8000)) \
    count=$((0x1000))
  ```

- [x] Decode it:

  ```bash
  espflash partition-table \
    --to-csv \
    --output x4-stock-partition-table.csv \
    x4-stock-partition-table.bin
  ```

- [x] Compare the decoded stock CSV with the community X4 layout in `docs/notes.md`.
- [ ] Commit the decoded CSV if it contains no private data. Do not commit the full dump.
- [x] Record the offsets and maximum sizes of `app0`, `app1`, `otadata`, NVS, filesystem, and coredump.

**Phase 1 exit condition:** a verified full backup exists in two locations, and the partition table from this physical device is understood.

---

## Architecture and learning decision — build an X4 layer, not an SDK first

The best learning path is a hybrid approach:

- Use `esp-hal`, Embassy, `esp-radio`, Trouble, and other established Rust crates for generic ESP32-C3 facilities.
- Preserve or reuse the proven X4 boot, partition, and recovery design.
- Treat MarigoldOS and the OpenX4 community SDK as known-working references and test oracles—not necessarily as Brewthink's permanent application base.
- Implement Brewthink's small X4-specific board-support layer in Rust, one subsystem at a time.
- Build Brewthink's reader application and host-testable logic ourselves.
- Extract a reusable community Rust SDK only after real application use has revealed stable APIs.

Do not begin by porting the complete C++ community SDK. A line-for-line port would create a large amount of code before we understand which abstractions Brewthink actually needs. The preferred progression is:

```text
working Brewthink hardware code
    → repeated and understood patterns
    → stable internal modules
    → reusable X4 board-support crates
    → optional community Rust SDK
```

### Worth implementing and understanding ourselves

- [ ] A central X4 board module that owns and names the physical pins.
- [ ] Deterministic, safe X4 initialization and chip-select behavior.
- [ ] A small SSD1677 Rust driver around a known-good panel initialization sequence.
- [ ] Display command/data/reset/BUSY handling with timeouts and useful errors.
- [ ] Shared SPI2 ownership and locking between the display and SD card.
- [ ] X4 button ADC measurement, calibration, classification, and debounce.
- [ ] Battery ADC conversion based on measurements from this physical device.
- [ ] X4 power-button, sleep, wake, and eventually GPIO13 power-path behavior.
- [ ] Brewthink application state, navigation, pagination, rendering, and persistence.

### Reuse rather than recreate

- [ ] RISC-V startup and low-level ESP peripheral access from `esp-hal`.
- [ ] Embassy executor, timers, synchronization primitives, and async foundations.
- [ ] ESP Wi-Fi support from `esp-radio`.
- [ ] BLE protocol support from Trouble.
- [ ] An established SD/FAT filesystem implementation.
- [ ] Espressif application-image and bootloader compatibility support.
- [ ] The verified stock/X4 partition design during early development.
- [ ] Known-good SSD1677/GDEQ0426T82 waveform and initialization data.
- [ ] Established TLS and cryptographic implementations.

Do not write custom GPIO register access, a FAT filesystem, a BLE stack, cryptography, or a new bootloader merely for learning. Those projects would consume large amounts of time without teaching much about the X4 specifically.

### How to use MarigoldOS without blindly forking it

For each X4 subsystem:

1. [ ] Build or run the relevant MarigoldOS implementation without changing the X4.
2. [ ] Trace its inputs, outputs, pins, initialization sequence, and error handling.
3. [ ] Compare it with the community schematic, controller documentation, and OpenX4 code.
4. [ ] Explain the subsystem in Brewthink's notes.
5. [ ] Implement the smallest useful Rust version in Brewthink.
6. [ ] Compare Brewthink's observable behavior with the known-working reference.
7. [ ] Commit that subsystem as a separate known-good milestone.

This approach makes MarigoldOS executable documentation while ensuring the resulting Brewthink code is understood rather than copied as unexplained machinery.

### When an X4 Rust SDK becomes worthwhile

Do not design a public SDK before the application has exercised the hardware. Reconsider extracting reusable crates only after Brewthink can reliably:

- [ ] Boot and report its state through USB logging.
- [ ] Render repeatable full and partial display updates.
- [ ] Read all buttons and battery voltage.
- [ ] Share SPI2 safely between the display and SD card.
- [ ] Enter and leave deep sleep correctly.
- [ ] Recover from a failed application image using the preserved recovery path.

At that point, likely reusable boundaries include `x4-board`, `x4-display`, and an X4-safe image/flashing tool. Until then, prefer simple internal modules over speculative SDK abstractions.

---

## Phase 2 — Understand the current project and choose the X4 base

The generated Brewthink project is useful for learning Cargo and ESP crates, but it is not yet an X4 board-support package.

- [ ] Read and annotate:
  - [ ] `Cargo.toml` — dependencies and enabled features.
  - [ ] `rust-toolchain.toml` — compiler and target.
  - [ ] `.cargo/config.toml` — target, runner, logging, and build-std.
  - [ ] `src/bin/main.rs` — allocator, Embassy, Wi-Fi, BLE, and startup.
- [x] Run `cargo check` without connecting/flashing the X4.
- [x] Understand why the generated runner was unsafe and disable it before Phase 3:

  ```toml
  # runner = "espflash flash --monitor --chip esp32c3 --log-format defmt"
  ```

- [ ] Study the X4-specific parts of MarigoldOS:
  - [ ] Firmware entry point and initialization.
  - [ ] X4 pin definitions.
  - [ ] SSD1677 display flush path.
  - [ ] Shared SPI/SD session management.
  - [ ] Partition and image configuration.
  - [ ] OTA/recovery flow.
  - [ ] Emulator and host-testable application core.
- [ ] Study the OpenX4 SSD1677 guide and compare it with MarigoldOS’s Rust implementation.
- [ ] Decide how Brewthink will inherit known-good X4 support:
  - [ ] Recommended: begin as a small MarigoldOS-based firmware and progressively replace the application/UI.
  - [ ] Alternative: port the required Marigold hardware/recovery modules into Brewthink one subsystem at a time.
- [ ] Write the decision and rationale in `docs/plan.md`.

**Phase 2 exit condition:** Brewthink has a documented X4-aware architecture and does not depend on a generic flash layout.

---

## Phase 3 — Create a safe X4 build and flashing workflow

Do not improvise flash offsets. Derive them from the decoded stock table and the chosen X4-aware base.

- [ ] Make the Rust application produce an ESP-IDF-compatible app image with the required app descriptor.
- [ ] Confirm the image fits inside one X4 application partition.
- [ ] Inspect the image size and intended flash offset before every flash.
- [ ] Preserve the existing bootloader and partition table for the first experiments.
- [ ] Keep one OTA slot containing a known-good firmware.
- [ ] Define which slot is the development slot and which is the recovery slot.
- [ ] Understand how `otadata` selects a slot before changing it.
- [ ] Replace the generic Cargo runner with an explicit X4 flashing script or `xtask` that:
  - [ ] Refuses an unexpected chip or flash size.
  - [ ] Checks the application size against the partition limit.
  - [ ] Writes only the intended application partition.
  - [ ] Does not silently replace the bootloader or partition table.
  - [ ] Prints every offset before writing.
  - [ ] Requires confirmation for destructive operations.
- [ ] Document the recovery procedure before performing the first custom flash.

**Phase 3 exit condition:** the exact bytes and offsets written by the development command are known in advance, and a known-good slot remains available.

---

## Phase 4 — First hardware boot: logging only

The first custom image should prove boot and recovery, not attempt to run the entire reader.

- [ ] Temporarily disable Wi-Fi and BLE initialization to reduce RAM use and failure sources.
- [ ] Initialize USB/serial or `defmt` logging immediately.
- [ ] Print:
  - [ ] Firmware name and Git commit.
  - [ ] Build version.
  - [ ] Reset/wakeup reason.
  - [ ] Selected partition/slot if available.
  - [ ] A periodic heartbeat counter.
- [ ] Preserve GPIO11 and GPIO14–17 for flash.
- [ ] Preserve GPIO18/19 for USB Serial/JTAG.
- [ ] Avoid changing GPIO13 power control until its required behavior is confirmed from known-good X4 firmware.
- [ ] Verify the custom image boots and logs.
- [ ] Verify the stock/known-good recovery path still works.
- [ ] Commit the result as the first known-good Brewthink hardware baseline.

**Phase 4 exit condition:** Brewthink boots, logs reliably, and can be replaced/recovered without relying on the e-paper screen.

---

## Phase 5 — Safe board initialization

- [ ] Create one central X4 board/pin module rather than scattering GPIO numbers through the code.
- [ ] Represent pin roles clearly:
  - [ ] GPIO0 — battery ADC.
  - [ ] GPIO1/2 — button ADC ladders.
  - [ ] GPIO3 — active-low power/wake button.
  - [ ] GPIO4 — display D/C.
  - [ ] GPIO5 — display reset.
  - [ ] GPIO6 — display busy.
  - [ ] GPIO7 — SD MISO.
  - [ ] GPIO8 — shared SPI clock and strapping pin.
  - [ ] GPIO10 — shared SPI MOSI.
  - [ ] GPIO12 — SD CS, memory-interface alias; initialize high.
  - [ ] GPIO13 — X4 power-path control; treat as reserved until verified.
  - [ ] GPIO21 — display CS; initialize high.
- [ ] Set both SPI chip-select pins high before sending any SPI traffic.
- [ ] Configure button and BUSY pins as inputs with the correct pull behavior.
- [ ] Keep board initialization deterministic and log each completed step.

---

## Phase 6 — E-paper display bring-up

- [ ] Configure SPI2 for the shared X4 bus:
  - [ ] SCLK GPIO8.
  - [ ] MOSI GPIO10.
  - [ ] MISO GPIO7 when needed for SD.
  - [ ] Start at a conservative clock speed before increasing it.
- [ ] Implement or reuse the known-good SSD1677 reset/init sequence.
- [ ] Wait on GPIO6 BUSY with a timeout; never wait forever without logging.
- [ ] Draw a simple, unmistakable test pattern containing the build ID.
- [ ] Perform one full refresh.
- [ ] Add partial refresh only after full refresh is reliable.
- [ ] Handle display sleep and wake correctly.
- [ ] Compare command bytes with the SSD1677 guide/datasheet and explain them in comments.

**Phase 6 exit condition:** a repeatable full-screen test image is rendered after each boot, with serial logs showing every display stage.

---

## Phase 7 — Input and battery

- [ ] Read GPIO1 and GPIO2 through the ADC.
- [ ] Log raw ADC values for every physical button on this device.
- [ ] Derive ranges from measurements instead of copying exact community constants blindly.
- [ ] Add debounce and edge detection.
- [ ] Read GPIO3 as the active-low power button.
- [ ] Read battery voltage through GPIO0 and the 10 kΩ / 10 kΩ divider.
- [ ] Convert ADC measurements to millivolts.
- [ ] Add a conservative battery-percentage curve.
- [ ] Render button events and battery state on a diagnostic screen.

---

## Phase 8 — Shared SPI and microSD

- [ ] Model the shared SPI bus separately from each logical device.
- [ ] Give the display its GPIO21 CS behavior.
- [ ] Give the SD card its GPIO12 CS behavior.
- [ ] Use a mutex/bus manager so Embassy tasks cannot interleave display and SD transactions.
- [ ] Verify the inactive device’s CS remains high during every transaction.
- [ ] Initialize the SD card at a conservative clock speed.
- [ ] Read card metadata and list the root directory.
- [ ] Mount FAT32 first; add exFAT only if required and supported.
- [ ] Display filenames read from the SD card.
- [ ] Test alternating SD reads and display refreshes to prove bus arbitration.

**Phase 8 exit condition:** repeated SD reads and display updates work together without corruption or deadlocks.

---

## Phase 9 — Power and sleep

- [ ] Understand GPIO13’s exact power-path role before driving it.
- [ ] Record reset and deep-sleep wake causes.
- [ ] Configure GPIO3 as the wake source.
- [ ] Shut down or sleep the display before ESP deep sleep.
- [ ] Deselect and quiesce the SD card.
- [ ] Disable Wi-Fi/BLE before sleep.
- [ ] Measure current draw rather than assuming sleep is effective.
- [ ] Implement long-press sleep and deliberate wake behavior.
- [ ] Test repeated sleep/wake cycles without losing SD or display state.

---

## Phase 10 — Build the reader application incrementally

- [ ] Separate hardware drivers from application logic.
- [ ] Keep application state host-testable where possible.
- [ ] Implement a reducer/state-machine model for actions such as button presses and file-open events.
- [ ] First usable feature: browse and read plain `.txt` files.
- [ ] Add page layout and deterministic pagination.
- [ ] Store reading position safely.
- [ ] Add a library screen.
- [ ] Add fonts and Unicode intentionally within RAM/storage limits.
- [ ] Add EPUB parsing only after TXT reading, caching, and pagination are reliable.
- [ ] Use the emulator/host tests for layout, state transitions, and parser behavior.

---

## Phase 11 — Networking

Do not make the radio part of initial hardware bring-up.

### Wi-Fi

- [ ] Re-enable the allocator and Wi-Fi only when there is a concrete feature to test.
- [ ] Join one access point and log connection state transitions.
- [ ] Obtain DHCP configuration.
- [ ] Implement one small transfer, such as fetching a text file.
- [ ] Add timeout, retry, and radio shutdown behavior.
- [ ] Avoid storing credentials in logs or Git.

### BLE / Trouble

- [ ] Define a specific BLE use case before enabling it.
- [ ] Start as one peripheral with one GATT service and one characteristic.
- [ ] Limit connections and packet resources for the X4’s small RAM.
- [ ] Test Wi-Fi/BLE coexistence only after each works independently.
- [ ] Turn radios off when they are not needed to protect battery life.

---

## Phase 12 — Reliability and release discipline

- [ ] Keep a known-good recovery image and checksum.
- [ ] Add host unit tests for parsers, layout, state transitions, and cache logic.
- [ ] Add hardware smoke-test instructions.
- [ ] Make CI run formatting, Clippy, host tests, and firmware builds.
- [ ] Record firmware version and partition compatibility in every release.
- [ ] Never publish private stock dumps, credentials, MAC addresses, or NVS contents.
- [ ] Test OTA only after direct USB recovery and slot rollback are proven.
- [ ] Document upgrade and downgrade behavior.

---

## Suggested first working session

1. [ ] Create a real 16 MiB stock backup outside the repository.
2. [ ] Verify its size and SHA-256 checksum.
3. [ ] Extract and decode the stock partition table.
4. [ ] Add raw `.bin` dumps to `.gitignore`.
5. [ ] Run `cargo check` only; do not flash the generated Brewthink image.
6. [ ] Clone/read MarigoldOS and identify its X4 partition, board, display, shared-SPI, and recovery modules.
7. [ ] Decide whether Brewthink begins as a MarigoldOS-based firmware or ports those modules deliberately.
8. [ ] Write that architectural decision in `docs/plan.md`.

## Reference material

- `docs/notes.md` — device specification, pin map, backup procedure, and partition background.
- `docs/plan.md` — current high-level safety principles and future architecture decision.
- [MarigoldOS](https://github.com/Jon-Vii/marigold-os)
- [OpenX4 SSD1677 guide](https://github.com/open-x4-epaper/community-sdk/blob/main/libs/display/EInkDisplay/doc/SSD1677_GUIDE.md)
- [X4 community schematic](https://github.com/sunwoods/Xteink-X4/blob/main/readme-img/sch.jpg)
- [ESP32-C3 datasheet](https://documentation.espressif.com/esp32-c3_datasheet_en.pdf)
- [ESP32-C3 Technical Reference Manual](https://www.espressif.com/sites/default/files/documentation/esp32-c3_technical_reference_manual_en.pdf)

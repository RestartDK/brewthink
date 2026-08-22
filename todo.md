# Brewthink TODO

Goal: build custom Rust firmware for the Xteink X4 e-reader, learning the hardware one subsystem at a time. The first big milestone is to show a picture on the reader while preserving stock recovery.

## Safety baseline

- [x] Full 16 MiB stock flash backup exists under ignored `backup/`.
- [x] Backup checksum verifies.
- [x] Second private backup copy exists at `$HOME/X4-backups/brewthink-stock/`.
- [x] Stock partition table decoded and committed as `docs/x4-stock-partition-table.csv`.
- [x] Stock `app0` is valid and selected.
- [x] Stock `app1` is empty.
- [x] Secure Boot and Flash Encryption are disabled.
- [x] Physical flash identified as Puya JEDEC `85 20 18`.
- [x] Generic `cargo run` flashing runner disabled.
- [x] Fenix/Nix dev shell validates with `cargo check`.
- [ ] Do not erase flash.
- [ ] Do not burn eFuses.
- [ ] Do not overwrite bootloader, partition table, NVS, filesystem, or stock `app0`.
- [ ] Do not switch OTA boot target until a tested `app1` image and recovery flow exist.

## Milestone 1 — Reference study before writing firmware

- [ ] Study MarigoldOS firmware entry point.
- [ ] Study MarigoldOS X4 pin definitions.
- [ ] Study MarigoldOS SSD1677 display path.
- [ ] Study MarigoldOS shared SPI/SD session handling.
- [ ] Study MarigoldOS OTA/update flow.
- [ ] Study OpenX4/community SSD1677 initialization sequence.
- [ ] Record the minimum required X4 boot/display behavior in `docs/plan.md`.

## Milestone 2 — Safe app1 build/flash workflow

- [ ] Build firmware image locally without flashing.
- [ ] Inspect local image with `esptool image-info`.
- [ ] Confirm image is ESP32-C3-compatible.
- [ ] Confirm image uses compatible DIO/flash settings.
- [ ] Confirm image contains required ESP app descriptor.
- [ ] Confirm image size is below `0x640000` bytes.
- [ ] Create a guarded script/tool that writes only `app1` at `0x650000`.
- [ ] Script must refuse unexpected chip/flash size.
- [ ] Script must print offset and size before writing.
- [ ] Script must not alter bootloader, partition table, NVS, filesystem, `app0`, or `otadata`.
- [ ] Add readback verification for the written `app1` bytes.

## Milestone 3 — First custom boot and logging

Goal: Brewthink boots and logs reliably, with no display/SD/radio yet.

- [ ] Create minimal firmware branch.
- [ ] Remove/defer Wi-Fi initialization.
- [ ] Remove/defer BLE/Trouble initialization.
- [ ] Initialize logging immediately.
- [ ] Print firmware name and version.
- [ ] Print reset/wakeup reason if available.
- [ ] Print heartbeat counter.
- [ ] Preserve flash pins GPIO11/GPIO14/GPIO15/GPIO16/GPIO17.
- [ ] Preserve USB pins GPIO18/GPIO19.
- [ ] Keep stock `app0` bootable.

## Milestone 4 — X4 board abstraction

Goal: centralize hardware ownership so GPIO numbers are not scattered through the code.

- [ ] Create an `x4_board` module.
- [ ] Define named roles for every X4 GPIO used by firmware.
- [ ] Represent display pins: GPIO4/GPIO5/GPIO6/GPIO21.
- [ ] Represent shared SPI pins: GPIO8/GPIO10.
- [ ] Represent SD pins: GPIO7/GPIO12.
- [ ] Represent button ADC pins: GPIO1/GPIO2.
- [ ] Represent battery ADC pin: GPIO0.
- [ ] Represent power button GPIO3.
- [ ] Treat GPIO13 as reserved/power-path until verified.
- [ ] Initialize display CS GPIO21 high.
- [ ] Initialize SD CS GPIO12 high.
- [ ] Add logs for each board-init stage.
- [ ] Make each subsystem independently constructible/testable where possible.

## Milestone 5 — Display module and raw screen bring-up

Goal: show controlled raw output on the e-paper display.

- [ ] Create an SSD1677/display module.
- [ ] Implement command/data write helpers.
- [ ] Implement reset sequence.
- [ ] Implement BUSY wait with timeout.
- [ ] Use known-good SSD1677/GDEQ0426T82 init sequence from references.
- [ ] Render all-white frame.
- [ ] Render all-black frame.
- [ ] Render checkerboard/test pattern.
- [ ] Render a compile-time raw bitmap.
- [ ] Put build/version text or marker in test image if feasible.
- [ ] Document full-refresh sequence and any unexplained command bytes.

## Milestone 6 — First picture on the reader

Goal: display a real picture after raw rendering works.

- [ ] Define framebuffer format for the X4 display.
- [ ] Render a simple raw 1-bit or grayscale image.
- [ ] Add scaling/cropping strategy.
- [ ] Add grayscale conversion.
- [ ] Add thresholding or dithering.
- [ ] Add simple uncompressed image support first, e.g. PBM/BMP if useful.
- [ ] Add PNG decode after raw/uncompressed rendering is reliable.
- [ ] Add JPEG decode after PNG or only if memory allows.
- [ ] Measure RAM usage; remember the X4 has no PSRAM.

## Milestone 6.5 — WASM/web simulator

Goal: see and test the same Brewthink app/rendering behavior in a browser without needing the physical X4 for every UI iteration.

Important design rule: do **not** try to run the exact ESP32 firmware binary in the browser. Share app logic, input events, rendering code, and framebuffer format; use separate hardware backends.

Suggested architecture:

```text
shared app-core/render-core
    ├── X4 firmware backend: GPIO, SPI, SSD1677, ADC, SD, sleep
    └── WASM web backend: canvas, keyboard, fake battery, fake storage
```

Timing:

- [ ] Add this after raw framebuffer/display rendering works on the real X4.
- [ ] Add it before the library app becomes large, so UI/state/rendering boundaries stay clean.

Core tasks:

- [ ] Add `wasm32-unknown-unknown` to `rust-toolchain.toml` when ready.
- [ ] Add web tooling to `flake.nix`, likely `trunk`, `wasm-bindgen-cli`, and `binaryen`.
- [ ] Create a `web-sim` crate/app.
- [ ] Create or extract shared `app-core` for state transitions.
- [ ] Create or extract shared rendering code that outputs an 800 × 480 framebuffer.
- [ ] Render the shared framebuffer to an HTML canvas.
- [ ] Map keyboard input to the same button events used by firmware:
  - [ ] Arrow keys → Up/Down/Left/Right.
  - [ ] Enter → Confirm.
  - [ ] Escape → Back.
  - [ ] P or Space → Power/action as appropriate.
- [ ] Add fake battery state.
- [ ] Add fake file list / fake SD storage.
- [ ] Add browser file picker or drag-and-drop for image testing later.
- [ ] Keep app logic host-testable with normal Rust tests.

Shared concepts to define:

- [ ] `ButtonEvent` enum shared by firmware and web.
- [ ] `BatteryState` shared by firmware and web.
- [ ] `FrameBuffer` or equivalent 800 × 480 display buffer.
- [ ] A display target/backend interface so firmware flushes to SSD1677 while WASM draws to canvas.
- [ ] A storage abstraction so firmware can later use SD while web can use fake files, browser files, or IndexedDB.

Useful simulator features later:

- [ ] Simulated e-paper refresh delay.
- [ ] Optional ghosting/partial-refresh visualization.
- [ ] Simulated sleep screen.
- [ ] Mock Komga/OPDS responses.
- [ ] LocalStorage/IndexedDB state persistence.

## Milestone 7 — Buttons and battery controller module

Goal: collect user input and power state.

- [ ] Create controller/input module.
- [ ] Read GPIO1 ADC ladder for Back/Confirm/Left/Right.
- [ ] Read GPIO2 ADC ladder for Up/Down.
- [ ] Log raw ADC values from this physical unit.
- [ ] Derive button ranges from measurements.
- [ ] Add debounce and event generation.
- [ ] Read GPIO3 power button as active-low digital input.
- [ ] Create battery module.
- [ ] Read GPIO0 battery ADC through 2 × 10 kΩ divider.
- [ ] Convert ADC readings to millivolts.
- [ ] Estimate battery percentage conservatively.
- [ ] Render diagnostic page showing last button event and battery percentage.

## Milestone 8 — Local image browser

Goal: scroll between selected images with buttons and show battery percentage.

- [ ] Add shared SPI bus locking.
- [ ] Add SD card initialization over SPI.
- [ ] Verify display CS and SD CS never assert together.
- [ ] Mount/read filesystem from microSD.
- [ ] List image files from a known directory.
- [ ] Show first image.
- [ ] Use buttons to move next/previous.
- [ ] Show battery percentage overlay/status.
- [ ] Handle missing SD card gracefully.
- [ ] Handle unsupported image files gracefully.

## Milestone 9 — Sleep, wake, and power

Goal: turn on/off safely and preserve battery.

- [ ] Understand GPIO13 power-path role from reference firmware/schematic.
- [ ] Implement display sleep.
- [ ] Quiesce/deselect SD before sleep.
- [ ] Disable radios before sleep when added later.
- [ ] Configure GPIO3 as wake source.
- [ ] Enter ESP deep sleep.
- [ ] Wake reliably with power button.
- [ ] Render optional sleep screen before sleeping.
- [ ] Allow user-selected sleep image eventually.
- [ ] Measure or estimate current draw if possible.

## Milestone 10 — Offline-first library app

Goal: move from image demo to actual reader application.

- [ ] Define host-testable app state model.
- [ ] Define library item model.
- [ ] Scan local storage for books/images.
- [ ] Create library screen.
- [ ] Store reading position/progress.
- [ ] Add recently opened items.
- [ ] Add simple settings.
- [ ] Keep rendering/layout logic testable off-device.

## Milestone 11 — Reading formats

Suggested order from easiest/most realistic to hardest:

- [ ] Plain text or markdown for simple reader flow.
- [ ] CBZ support, since it is mostly zipped page images.
- [ ] EPUB support.
- [ ] PDF support only after feasibility review.
- [ ] Consider pre-rendered/page-image PDF workflow because ESP32-C3 has no PSRAM.
- [ ] Add format-specific caches where needed.
- [ ] Avoid loading whole large books into RAM.

## Milestone 12 — Wi-Fi and BLE

Do this late; radios add RAM pressure, power cost, and async complexity.

- [ ] Define exact Wi-Fi use case before enabling Wi-Fi.
- [ ] Connect to one access point.
- [ ] Avoid logging credentials.
- [ ] Fetch one small file over HTTP.
- [ ] Add timeout/retry handling.
- [ ] Shut down Wi-Fi after use.
- [ ] Define exact BLE use case before enabling BLE.
- [ ] Use BLE for setup/control/small metadata, not large transfer.
- [ ] Test Wi-Fi and BLE separately before coexistence.

## Milestone 13 — File transfer and Komga sync

Goal: sync useful content while staying offline-first.

- [ ] Add simple local file transfer protocol or endpoint.
- [ ] Add download queue.
- [ ] Add offline cache structure.
- [ ] Investigate Komga API and OPDS support.
- [ ] Start with listing library entries.
- [ ] Download one selected book/image set.
- [ ] Sync metadata.
- [ ] Sync reading progress if appropriate.
- [ ] Add conflict/offline handling.
- [ ] Add background sync policy only after manual sync is reliable.

## Current immediate next steps

- [ ] Restart Neovim from inside the Brewthink direnv shell.
- [ ] Read `AGENTS.md`, `docs/notes.md`, and this file before making code changes.
- [ ] Create a branch for reference study / minimal bring-up.
- [ ] Study MarigoldOS and OpenX4 references without flashing.
- [ ] Plan the minimal logging-only firmware.
- [ ] Do not flash until the `app1`-only write/readback workflow exists.

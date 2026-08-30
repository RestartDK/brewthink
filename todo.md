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
- [x] Study MarigoldOS X4 pin definitions.
- [x] Study MarigoldOS SSD1677 display path.
- [ ] Study MarigoldOS shared SPI/SD session handling.
- [ ] Study MarigoldOS OTA/update flow.
- [x] Study OpenX4/community SSD1677 initialization sequence.
- [ ] Record the minimum required X4 boot/display behavior in `docs/plan.md`.

## Milestone 2 — Safe app1 build/flash workflow

- [x] Build firmware image locally without flashing.
- [x] Inspect local image with `esptool image-info`.
- [x] Confirm image is ESP32-C3-compatible.
- [x] Confirm image uses compatible DIO/flash settings.
- [x] Confirm image contains required ESP app descriptor.
- [x] Confirm image size is below `0x640000` bytes.
- [x] Create a guarded script/tool that writes only `app1` at `0x650000`.
- [x] Script must refuse unexpected chip/flash size.
- [x] Script must print offset and size before writing.
- [x] Script must not alter bootloader, partition table, NVS, filesystem, `app0`, or `otadata`.
- [x] Add readback verification for the written `app1` bytes.

## Milestone 3 — First custom boot and logging

Goal: Brewthink boots and logs reliably, with no display/SD/radio yet.

- [x] Create minimal firmware branch.
- [x] Remove/defer Wi-Fi initialization.
- [x] Remove/defer BLE/Trouble initialization.
- [x] Initialize logging immediately.
- [x] Print firmware name and version.
- [ ] Print reset/wakeup reason if available.
- [x] Print heartbeat counter.
- [x] Preserve flash pins GPIO11/GPIO14/GPIO15/GPIO16/GPIO17.
- [x] Preserve USB pins GPIO18/GPIO19.
- [x] Keep stock `app0` bootable.

## Milestone 4 — X4 board abstraction

Goal: centralize hardware ownership so GPIO numbers are not scattered through the code.

- [x] Create an `x4` board module.
- [x] Define named roles for every X4 GPIO used by firmware.
- [x] Represent display pins: GPIO4/GPIO5/GPIO6/GPIO21.
- [x] Represent shared SPI pins: GPIO8/GPIO10.
- [x] Represent SD pins: GPIO7/GPIO12.
- [x] Represent button ADC pins: GPIO1/GPIO2.
- [x] Represent battery ADC pin: GPIO0.
- [x] Represent power button GPIO3.
- [x] Treat GPIO13 as reserved/power-path until verified.
- [x] Initialize display CS GPIO21 high.
- [x] Initialize SD CS GPIO12 high.
- [x] Add logs for each board-init stage.
- [x] Make each subsystem independently constructible/testable where possible.

## Milestone 5 — Display module and raw screen bring-up

Goal: show controlled raw output on the e-paper display.

- [x] Create an SSD1677/display module.
- [x] Implement command/data write helpers.
- [x] Implement reset sequence.
- [x] Implement BUSY wait with timeout.
- [x] Use known-good SSD1677/GDEQ0426T82 init sequence from references.
- [x] Render all-white frame.
- [x] Render all-black frame.
- [x] Render checkerboard/test pattern.
- [x] Render a compile-time raw bitmap.
- [ ] Put build/version text or marker in test image if feasible.
- [x] Document full-refresh sequence and any unexplained command bytes.

## Milestone 6 — First picture on the reader

Goal: display a real picture after raw rendering works.

- [x] Define rotation-aware framebuffer views for 0°/90°/180°/270°.
- [x] Default to a 480 × 800 portrait framebuffer at the corrective 270° rotation.
- [x] Render a simple raw 1-bit image.
- [x] Visually confirm the corrected 270° corner labels are upright and correctly placed.
- [x] Add `contain` and `cover` scaling with bilinear resampling.
- [x] Add integer RGB-to-luma conversion.
- [x] Add fixed threshold and ordered 4 × 4 dithering.
- [x] Add build-time PNM and BMP decoding.
- [x] Add build-time PNG decoding.
- [x] Add build-time baseline and progressive JPEG decoding.
- [x] Keep decoded RGB data on the host; embed only the 48,000-byte packed frame in firmware flash.
- [ ] Design bounded-memory runtime decoding for files read from microSD.

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

- [x] Start this after raw framebuffer/display rendering works on the real X4.
- [ ] Add it before the library app becomes large, so UI/state/rendering boundaries stay clean.

Core tasks:

- [x] Add `wasm32-unknown-unknown` to `rust-toolchain.toml`.
- [ ] Add web tooling to `flake.nix`, likely `trunk`, `wasm-bindgen-cli`, and `binaryen`.
- [ ] Create a `web-sim` crate/app.
- [ ] Create or extract shared `app-core` for state transitions.
- [ ] Create or extract shared rendering code that outputs the default 480 × 800 portrait framebuffer.
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
- [x] Read-only `Frame` view and `Rotation` domain types shared by host, WASM, and firmware.
- [x] Mutable packed `MonochromeImage` render target for host, WASM, and build-time conversion.
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

- [x] Visually confirm the corrected 270° orientation and record the result.
- [ ] Display the build-time decoded sample image from guarded `app1`.
- [ ] Add a host/WASM display backend around the shared packed image type.
- [ ] Design bounded-memory runtime image decoding before adding SD image browsing.

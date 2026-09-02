# Brewthink X4 scripts

Safe app1 workflow for the physical Xteink X4.

## Board abstraction checks

```bash
scripts/check-board-abstraction.sh
```

This runs formatting, host pin-map/display tests, a WASM library check, embedded checks, Clippy, and app1 image inspection. It does not write device flash.

## Display diagnostic images

The default build uses diagnostic stage `heartbeat`. Select a display diagnostic explicitly when building:

```bash
BREWTHINK_DIAGNOSTIC_STAGE=display-orientation \
BREWTHINK_DISPLAY_ROTATION=270 \
  scripts/build-app1-image.sh artifacts/brewthink-display-rotation-270-app1.bin
```

Valid stages are `display-reset`, `display-initialize`, `display-write`, `display-refresh`, `display-black`, `display-checkerboard`, `display-orientation`, and `display-image`. Rotation accepts `0`, `90`, `180`, or `270`; it defaults to Brewthink's corrected portrait value of `270`. The 0°/180° frames are 800 × 480 and the 90°/270° frames are 480 × 800. Each stage runs once and holds without retry. See `docs/display-bringup.md` for the command transcript and staged procedure.

## Raw input diagnostic image

```bash
scripts/build-inputs-raw-app1.sh
```

This stage samples calibrated GPIO0/GPIO1/GPIO2 ADC voltages plus GPIO3 power-button and GPIO20 USB-detect levels every 100 ms. It keeps display and SD chip selects high and does not initialize SPI, the display, SD protocol, GPIO13, or radio hardware.

Build the debounced button-event stage from this unit's measured voltage bands with:

```bash
scripts/build-inputs-events-app1.sh
```

It samples every 20 ms, requires three consecutive readings, emits one structured press and release event per transition, and rejects voltages outside the measured bands instead of assigning them to the nearest button.

Build the battery and USB transition stage with:

```bash
scripts/build-power-usb-app1.sh
```

After booting that image once, stop `espflash monitor` and use the reconnecting, read-only serial collector:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX tools/device-bench.py
```

The collector sends no serial data and requests neither reset nor download mode. The firmware records the number and battery-voltage range of samples taken while USB is disconnected, then reports that retained in-RAM summary after USB reconnects. A reset or loss of battery power during disconnection intentionally loses the summary and fails the test.

## Read-only microSD diagnostic image

```bash
scripts/build-storage-readonly-app1.sh
```

This stage owns SPI2 exclusively, keeps display CS GPIO21 high during every SD session, initializes the card at 400 kHz, enables command/data CRC, then switches to 10 MHz. Its storage API exposes initialization and single-sector reads only: it has no block-write operation and sends no SD write command. It reads the CSD, sector zero, and—when present—the first MBR partition's boot sector to report capacity, partition metadata, and FAT/exFAT identification. The completion record explicitly reports `sectors_written=0` and both chip selects high.

## Disposable microSD write-test image

Build only after confirming the inserted card is disposable or backed up:

```bash
scripts/build-storage-write-test-app1.sh
```

The write path is excluded from normal firmware and exists only behind the `sd-write-diagnostic` feature. It refuses to run if `BWTST001.TMP` already exists. Otherwise it creates that root-directory file with a fixed 52-byte payload, flushes and closes it, reopens and verifies the exact bytes, deletes it, and confirms it is absent. If creation or verification fails after the target was known to be absent, it still attempts cleanup and reports if the file remains. The diagnostic may update the FAT, directory, free-space metadata, and one allocated data cluster. Do not flash or boot it until the exact app1 range and removable-media operation receive separate approval.

## Integrated device diagnostic image

```bash
scripts/build-integrated-device-app1.sh
```

This read-only stage uses one SPI2 owner to initialize and fingerprint microSD sector zero, switch the bus to the SSD1677 for a checkerboard full refresh, then reinitialize the card and verify that sector zero is unchanged. After both chip selects return high, it samples buttons, battery voltage, and USB state every 20 ms. The stage contains no SD write capability because it is built without `sd-write-diagnostic`.

## Display and ESP32-C3 sleep/wake image

```bash
scripts/build-sleep-wake-app1.sh
```

On an ordinary boot, this stage refreshes the orientation pattern, sends the SSD1677 deep-sleep command and check code, verifies both shared-SPI chip selects high, then enters ESP32-C3 deep sleep with active-low GPIO3 as the only wake source. Waking with the power button causes a fresh boot, hardware-resets the display out of deep sleep, refreshes it white, and holds without sleeping again. GPIO13 is not initialized. The stage has no SD write capability.

## Read-only EPUB reader image

```bash
scripts/build-reader-app1.sh
```

This builds the normal X4 reader behind `device-reader`. It scans up to sixteen DRM-free EPUBs from `/Books`, validates each package with bounded fixed-memory ZIP/XML parsing, renders PNG or baseline-JPEG covers, paginates XHTML, handles all seven controls, and retains a checksummed book/chapter/page resume record in RTC fast memory across GPIO3 deep sleep. EPUB and FAT access expose no successful write path, and the decoder workspace is statically allocated and phase-overlaid to preserve the runtime stack reserve. Building the image is local and read-only; copying a book to microSD and flashing the guarded `app1` image each require separate explicit approval.

### USB reader control

The Rust `device-control` host binary sends typed input commands through native USB Serial/JTAG while the reader is awake. Its launcher always builds for the host target, so it cannot invoke the embedded Cargo runner:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh status
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh tap right
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh screen artifacts/device-screen.png
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh monitor
```

A USB tap enters the same `AppInput` path as a debounced physical press. `screen` reads the exact 48,000-byte monochrome frame last sent to the SSD1677, checks its CRC32, and writes a 480 × 800 PNG. The PNG proves what firmware generated, not what physically appeared on the panel. The CLI opens the port directly without changing DTR or RTS. A Power tap renders the sleep frame and enters deep sleep, which disconnects USB; waking still requires GPIO3 through the physical Power button.

Build a JPEG, PNG, BMP, or PNM into an app1 image with:

```bash
scripts/build-image-app1.sh input.jpeg artifacts/image-app1.bin
```

This decodes, scales, converts to grayscale, dithers, and packs the image on the host. It also writes an ignored PBM preview. See `docs/image-pipeline.md` for options and memory constraints.

Building a diagnostic image does not touch hardware. Use the guarded app1 write/readback command below only after reviewing its exact offset, image size, and sector erase range. Do not use `cargo run`.

## Build and inspect locally, no hardware writes

```bash
scripts/build-app1-image.sh
scripts/check-app1-image.sh
esptool --chip esp32c3 image-info artifacts/brewthink-app1.bin
```

The generated image is ignored under `artifacts/`.

## Read-only hardware probe

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/probe-x4.sh
```

The output may contain private MAC/eFuse/device identifiers. Do not commit raw output.

## otadata backup

Before any boot-slot switch, back up the current OTA boot-selection metadata:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/backup-otadata.sh
```

This reads `0xE000..0xFFFF` into ignored `backup/otadata/` and prints a restore command.
It does not write device flash.

## Guarded app1 write/readback

Only run after reviewing the printed offset and size:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/flash-app1-and-readback.sh
```

This writes only `app1` at `0x650000`, reads back the same byte count, and compares SHA-256 hashes.
It does **not** write bootloader, partition table, NVS, filesystem, `app0`, or `otadata`.
Because `otadata` is not changed, stock `app0` should remain the selected boot slot.

## Switch boot selection to app1

Only after app1 write/readback verification and otadata backup:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/switch-boot-app1.sh
```

This writes only `otadata` sector 1 at `0xF000` with `ota_seq=2`, reads it back, and verifies it.
After reset, the ESP-IDF bootloader should choose `app1`. Restore the backed-up `otadata` file to `0xE000` to return to the exact previous stock `app0` selection.

## Restore helpers

These are hardware-writing recovery/cleanup tools. Review the printed ranges before confirming.

Restore only the previous boot selection metadata:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/restore-otadata.sh
```

Restore only stock `app0` from the private full-flash backup:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/restore-stock-app0.sh
```

Erase only the `app1` development slot and verify it is all `0xFF`:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/erase-app1.sh
```

Return to the verified stock boot state without writing the whole flash:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/restore-stock-state.sh
```

`restore-stock-state.sh` restores stock `app0`, erases `app1`, restores backed-up `otadata`, verifies readback, and resets once at the end. It does **not** write bootloader, partition table, NVS, filesystem, or coredump.

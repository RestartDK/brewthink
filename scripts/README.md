# Brewthink X4 scripts

Safe app1 workflow for the physical Xteink X4.

## Board abstraction checks

```bash
scripts/check-board-abstraction.sh
```

This runs formatting, host pin-map/display tests, a WASM library check, embedded checks, Clippy, and app1 image inspection. It does not write device flash.

## UI frame contract

Home, Books, Files, Settings, Reader, Image, Error, and Sleep have byte-exact PBM fixtures under `tests/fixtures/ui`. Empty catalogs, filename clipping, and sleep previews are included. After an intentional visual change, regenerate them with:

```bash
scripts/update-ui-frame-contract.sh
```

Review the rendered fixture changes before committing them. The script runs on the host and does not access the device.

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

## EPUB reader image

```bash
scripts/build-reader-app1.sh
```

This builds the normal X4 reader behind `device-reader`. It scans up to sixteen DRM-free EPUBs from `/books`, validates each package with bounded fixed-memory ZIP/XML parsing, renders PNG or baseline-JPEG covers, paginates XHTML, handles all seven controls, and retains a checksummed book/chapter/page resume record in RTC fast memory across GPIO3 deep sleep. Book access remains read-only. Firmware creates the 8.3-compatible `/brew`, `/brew/cache`, `/brew/bookmark`, and `/files` directories. Fixed application records live under `/brew`; transactional image uploads install named files under `/files`. The decoder workspace is statically allocated and phase-overlaid to preserve the runtime stack reserve. Building the image is local and does not touch hardware; copying a book to microSD and flashing the guarded `app1` image each require separate explicit approval.

The reader build checks its compiled stack frames with `check-reader-stack.py`. The development shell provides Python and LLVM for this check. See [SD recovery](../docs/sd-recovery.md) for read-only USB sector exports that bypass reader startup and FAT mounting.

### USB reader control

The Rust `device-control` host binary sends typed input commands through native USB Serial/JTAG while the reader is awake. Its launcher always builds for the host target, so it cannot invoke the embedded Cargo runner. `put-image` writes a named image on microSD under `/files`. Button taps can change settings, persist the selected sleep image, refresh the display, or enter deep sleep. These commands do not write firmware flash, OTA data, NVS, or eFuses.

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh status
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh tap right
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh screen artifacts/device-screen.png
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh put-image ~/Pictures/sleep.png
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh monitor
```

A USB tap enters the same `AppInput` path as a debounced physical press. `status` reports the current view plus battery percentage, measured millivolts, and USB presence. `screen` reads the exact 48,000-byte monochrome frame last sent to the SSD1677, checks its CRC32, and writes a 480 × 800 PNG. The PNG proves what firmware generated, not what physically appeared on the panel. `put-image` accepts JPEG or PNG input, normalizes oversized or unsupported sources into a bounded 480 × 800 baseline JPEG, sends 4 KiB acknowledged chunks, and requires matching stream and SD-readback CRC32 values before commit. The host preserves a valid 8.3 stem or rewrites an incompatible source name to a deterministic form such as `SUM~8A2F.JPG`. Files lists up to sixteen images from `/files`. Open one to view it, then press Confirm to select it for Custom Image and Automatic sleep modes. The CLI opens the port directly without changing DTR or RTS. A Power tap renders the sleep frame and enters deep sleep, which disconnects USB; waking still requires GPIO3 through the physical Power button.

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

This probes the X4 and reads `0xE000..0xFFFF` into a uniquely named file under ignored `backup/otadata/`. It records a SHA-256 sidecar and makes both files read-only. No `latest` file is replaced. An unrecognized selection is reported as an error, but the captured bytes remain available for inspection.

These read-only commands reset the processor to enter download mode. They do not write flash. Backup digests record integrity, not device identity. Before recovery, compare the digest against your recorded verified backup or independent private copy.

## Guarded app1 write/readback

Only run after reviewing the printed offset and size:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/flash-app1-and-readback.sh
```

The script copies the image to a private read-only snapshot before inspection and confirmation. It writes only `app1` at `0x650000` and compares the same byte count on readback. Reset and optional `--monitor` occur only after verification. A failure stops without a final reset. Monitoring requires an explicit `--elf PATH` from the reviewed image's build. The script snapshots those symbols before confirmation rather than taking whichever ELF was built last. The operator must confirm that image and ELF belong together; the script does not prove that association.

No bootloader, partition table, NVS, filesystem, `app0`, or `otadata` write occurs. The existing boot selection remains unchanged. That selection may already be `app1`.

For a preserved reader image, supply its independently recorded digest with `--image-sha256`. A mismatch fails before hardware access. After separate flash authorization and review of the exact app1 range, the restore invocation is:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/flash-app1-and-readback.sh \
  --image "$READER_BACKUP" --image-sha256 "$VERIFIED_READER_SHA"
```

This covers the pre-grayscale reader readback identified in `checkpoint.md`. Do not substitute the older historical reader artifact or derive the trusted digest from an unverified current file. Fresh builds may omit the digest option and review the printed snapshot digest instead.

If readback fails, do not repeat the write blindly. Keep the reviewed byte count and expected SHA-256 from the write review. A manual readback can enter download mode and reset the processor, but does not write flash:

```bash
espflash read-flash --chip esp32c3 --port "$ESPFLASH_PORT" --after no-reset \
  0x650000 "$REVIEWED_SIZE" "$READBACK_FILE"
shasum -a 256 "$READBACK_FILE"
```

Compare the result with the recorded expected digest. Only after a match, reset the chip to run the selected app with `espflash reset --chip esp32c3 --port "$ESPFLASH_PORT"`. A mismatch stops recovery for inspection. The script reports both digests and the first differing byte when readback completes but differs.

## Switch boot selection to app1

This is a first-switch helper, not an OTA manager. It accepts only a verified sequence-1 `app0` backup with an erased second OTA sector. It compares live app1 bytes with the reviewed image and live otadata with the reviewed backup before any write.

With explicit hardware-write authorization, set `OTA_BACKUP` and `VERIFIED_OTA_SHA` to the reviewed backup and its recorded digest. The following command writes only OTA sector 1 at `0xF000..0xFFFF`, verifies readback, and resets to select app1:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/switch-boot-app1.sh \
  --backup "$OTA_BACKUP" --backup-sha256 "$VERIFIED_OTA_SHA"
```

## Restore helpers

Recovery requires explicit hardware-write authorization and review of each printed range. Every helper probes chip, flash, crystal, and security state. Payloads are private read-only snapshots. Verification occurs before the final reset. No helper infers provenance from `otadata-latest.bin`.

`restore-otadata.sh` writes only `0xE000..0xFFFF`. It requires an explicit backup, recorded digest, and expected slot. It inspects the live target application's checksum and hash before a boot-selection write. For a reviewed backup selecting app0:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/restore-otadata.sh \
  --backup "$OTA_BACKUP" --backup-sha256 "$VERIFIED_OTA_SHA" --expect-slot app0
```

Set `STOCK_BACKUP` and `VERIFIED_STOCK_SHA` from your verified full stock backup and its independently recorded digest. `restore-stock-app0.sh` restores only `0x10000..0x64FFFF`, with no boot-selection change:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/restore-stock-app0.sh \
  --stock-flash-backup "$STOCK_BACKUP" --backup-sha256 "$VERIFIED_STOCK_SHA"
```

`restore-stock-state.sh` extracts app0 and the original sequence-1 otadata from that same verified full backup. It restores and verifies app0 before writing otadata, verifies otadata, and then resets. App1 remains intact. The former `--otadata-backup` option is rejected because a separate checkpoint does not establish stock selection.

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/restore-stock-state.sh \
  --stock-flash-backup "$STOCK_BACKUP" --backup-sha256 "$VERIFIED_STOCK_SHA"
```

Flash erasure is prohibited by `AGENTS.md`. `erase-app1.sh` always refuses without accessing hardware, including with `--yes`. Stock recovery does not require erasing app1.

## Test the scripts without hardware

```bash
python3 -m unittest discover -s scripts -p 'test_*.py'
```

`test_flash_safety.py` copies the scripts into temporary directories and replaces both hardware tools with fakes. Synthetic flash bytes exercise write ranges, ordering, concurrent image replacement, backup integrity, OTA selection, and failure paths. These tests do not establish physical power-loss recovery or replace a reviewed hardware procedure.

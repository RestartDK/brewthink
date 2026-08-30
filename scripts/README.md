# Brewthink X4 scripts

Safe app1 workflow for the physical Xteink X4.

## Board abstraction checks

```bash
scripts/check-board-abstraction.sh
```

This runs formatting, host pin-map/display tests, a WASM library check, embedded checks, Clippy, and app1 image inspection. It does not write device flash.

## Display diagnostic images

The default build uses display stage `none`. Select a hardware diagnostic explicitly when building:

```bash
BREWTHINK_DISPLAY_STAGE=orientation \
BREWTHINK_DISPLAY_ROTATION=270 \
  scripts/build-app1-image.sh artifacts/brewthink-display-rotation-270-app1.bin
```

Valid stages are `reset`, `initialize`, `write`, `refresh`, `black`, `checkerboard`, and `orientation`. Rotation accepts `0`, `90`, `180`, or `270`; it defaults to Brewthink's corrected portrait value of `270`. The 0°/180° frames are 800 × 480 and the 90°/270° frames are 480 × 800. Each stage runs once and holds without retry. See `docs/display-bringup.md` for the command transcript and staged procedure.

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

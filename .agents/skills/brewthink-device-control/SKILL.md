---
name: brewthink-device-control
description: Control and inspect the physical Xteink X4 through Brewthink's USB protocol. Use when sending button taps, reading reader status or structured logs, capturing the device framebuffer, monitoring USB reconnects, or testing firmware interaction without pressing the front buttons.
---

# Brewthink device control

Use this skill only in the Brewthink repository. Read the repository `AGENTS.md` before touching the X4.

## Keep the device safe

- Use `scripts/device-control.sh` for application control. It builds and runs the Rust host binary in `tools/device-control.rs`.
- Do not use `cargo run`. The launcher selects the host target and cannot invoke the embedded runner.
- Do not use `espflash monitor` for application control. It can reset the processor or attempt a bootloader handshake.
- Do not flash firmware unless the user explicitly asks. Review the exact image, offset, byte range, sector erase range, and target before any write.
- Never erase flash, burn eFuses, or write outside the reviewed `app1` range.
- Do not print or retain MAC addresses, credentials, or private device identifiers.

## Run control commands

Run device work in a dedicated Herdr tab. Set the explicit port when it is known:

```bash
export ESPFLASH_PORT=/dev/cu.usbmodemXXXX
```

Read the current application state before sending input:

```bash
scripts/device-control.sh status
```

Send one logical button tap:

```bash
scripts/device-control.sh tap right
scripts/device-control.sh tap confirm
scripts/device-control.sh tap back
```

Valid button names are `back`, `confirm`, `left`, `right`, `up`, `down`, and `power`.

Capture the framebuffer:

```bash
scripts/device-control.sh screen artifacts/device-screen.png
```

Read the resulting PNG before reporting success. The PNG contains the exact 48,000-byte monochrome frame returned by firmware. It proves what firmware generated, not what physically appeared on the e-paper panel.

Monitor structured application events:

```bash
scripts/device-control.sh monitor
```

`monitor` does not exit. Confirm the pane contents before sending Ctrl+C. Stop the process and close its Herdr tab when the task ends.

## Interpret responses

The device prefixes protocol output with `BREWCTL/1`.

- `EVENT source=usb input=<button>` means firmware accepted the tap.
- `LOG stage=<stage> state=start` means the application entered that operation.
- `LOG stage=<stage> state=done` means the operation returned.
- `STATUS` reports the current application view and position.
- `DONE command=<command> status=ok` is the terminal success record.
- `SCREEN` reports the dimensions, byte count, and CRC32 before the raw frame.

If `EVENT` or a start record appears without `DONE`, the USB command succeeded but the application operation stalled. Preserve the last structured record. Do not retry blindly. Resetting with an Espressif tool changes live device state, so explain the reset before running it.

## Respect sleep behavior

A `power` tap can render the sleep frame and enter ESP32-C3 deep sleep. Deep sleep disconnects USB. The CLI cannot wake the device because only active-low GPIO3 remains as a wake source. Send `power` only when the user accepts the need for a physical Power press or an explained processor reset.

## Verify tool changes

Run these checks in a dedicated Herdr tab:

```bash
HOST="$(rustc -vV | awk '/^host:/ {print $2}')"
cargo fmt --all -- --check
cargo test --target "$HOST" --features device-control --bin device-control
cargo clippy --target "$HOST" --features device-control --bin device-control -- -D warnings
```

For an actual-device smoke test, run `status`, a harmless directional tap, and `screen`. Check for terminal `DONE` records and inspect the PNG. This smoke test does not require a firmware write when the connected reader already implements `BREWCTL/1`.

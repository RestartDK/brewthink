# SSD1677 display bring-up

## Scope

Brewthink supports conservative monochrome full refreshes and explicit SSD1677 deep sleep on the Xteink X4's 800 × 480 GDEQ0426T82 panel. The reader also has experimental host-RAM and controller-RAM baseline storage for full-clean, quick-clean, and differential refreshes. Rectangular updates, custom LUTs, grayscale, and radio initialization remain excluded. Integrated diagnostics serialize display and read-only SD traffic through one SPI2 owner.

The portable code is under `src/display/`:

- `bus.rs` owns command/data framing, reset timing, and bounded BUSY polling through `embedded-hal` traits.
- `ssd1677.rs` owns the controller command sequence, geometry, RAM writes, and full-refresh activation.
- `framebuffer.rs` defines rotation-aware logical frame views and panel transforms.
- `diagnostic.rs` generates heap-free test patterns.

The ESP32-C3 adapter is under `src/x4/display.rs`. It binds SPI2 and the verified X4 GPIOs while retaining SD CS high.

## Hardware contract

| Property | Value |
| --- | --- |
| Panel | Good Display GDEQ0426T82 |
| Controller | SSD1677 |
| Panel RAM resolution | 800 × 480 landscape |
| Default logical resolution | 480 × 800 portrait |
| Configurable rotations | 0°, 90°, 180°, 270° clockwise into panel RAM |
| Default rotation | 270° |
| Format | 1 bit per pixel; `0` black, `1` white |
| Panel row size | 100 bytes |
| Default logical row size | 60 bytes |
| Frame size | 48,000 bytes |
| SPI | Mode 0, MSB first, 40 MHz |
| BUSY | Active high |
| BUSY timeout | 15 seconds |
| Reset | High 20 ms, low 2 ms, high 20 ms |
| SCLK / MOSI | GPIO8 / GPIO10 |
| Display CS / D/C / reset / BUSY | GPIO21 / GPIO4 / GPIO5 / GPIO6 |
| SD CS | GPIO12, retained high throughout display diagnostics |

Brewthink runs the X4 display phase at 40 MHz, matching MarigoldOS and the established X4 community overclock. The SSD1677 datasheet specifies a 20 MHz maximum write clock, and current FreeInk/CrossPoint defaults to that in-spec rate unless `FREEINK_X4_OVERCLOCK_SPI` is enabled. Only the display phase uses 40 MHz; Brewthink restores the separately verified 10 MHz SD data clock before card traffic. Moving from 20 to 40 MHz reduces two 48,000-byte plane transfers by about 19 ms but does not shorten the much longer physical e-paper waveform.

The SSD1677 datasheet describes a controller with up to 960 source and 680 gate outputs; those maxima are not the fitted panel dimensions. Brewthink uses the GDEQ0426T82's actual 800 × 480 geometry.

The first labeled test established that panel RAM is naturally landscape. `Frame` exposes rotation as a domain value rather than baking orientation into drawing code:

| `Rotation` | Logical size | Logical-to-panel mapping |
| --- | --- | --- |
| `Degrees0` | 800 × 480 | `panel_x = x`, `panel_y = y` |
| `Degrees90` | 480 × 800 | `panel_x = 799 - y`, `panel_y = x` |
| `Degrees180` | 800 × 480 | `panel_x = 799 - x`, `panel_y = 479 - y` |
| `Degrees270` | 480 × 800 | `panel_x = y`, `panel_y = 479 - x` |

The first 90° portrait test was upside down on the physical unit. Rotating that result another 180° selects absolute `Degrees270`, now Brewthink's default. Logical framebuffers remain directly usable by host and WASM renderers; only the SSD1677 flush path applies the selected transform.

## Initialization transcript

The host test `initialization_matches_x4_golden_transcript` fixes this sequence byte-for-byte:

| Operation | Command | Data |
| --- | ---: | --- |
| Hardware reset | — | high 20 ms, low 2 ms, high 20 ms |
| Software reset | `0x12` | — |
| Wait for BUSY low | — | — |
| Internal temperature sensor | `0x18` | `80` |
| Booster soft start | `0x0C` | `AE C7 C3 C0 40` |
| Driver output control | `0x01` | `DF 01 02` |
| Border waveform | `0x3C` | `01` |
| X increment, Y decrement | `0x11` | `01` |
| RAM X range | `0x44` | `00 00 1F 03` (`0..799`) |
| RAM Y range | `0x45` | `DF 01 00 00` (`479..0`) |
| Auto-clear BW RAM | `0x46` | `F7` |
| Wait for BUSY low | — | — |
| Auto-clear RED RAM | `0x47` | `F7` |
| Wait for BUSY low | — | — |
| Full-refresh comparison mode | `0x21` | `40 00` |
| Prepare full OTP update | `0x22` | `F7` |

A full refresh resets the RAM window and counters, writes the same 48,000-byte frame to BW RAM (`0x24`) and RED/previous RAM (`0x26`), then sends:

```text
0x21  40 00
0x22  F4
0x20
wait until BUSY is low
```

The first `SpiBus::write` and every command/data phase are flushed before D/C or CS changes. A transport error deselects the display. A BUSY timeout is returned as an error; diagnostics stop and retain the GPIO/SPI objects without retrying.

## Experimental baseline storage and refresh

The device reader always has a 48,000-byte next frame in its frame/codec workspace. `PreviousFrameStorage` selects where the last displayed frame lives:

| Storage | Previous frame | ESP32 framebuffer cost |
| --- | --- | ---: |
| `host-ram` | Dedicated ESP32 buffer | 96,000 bytes total |
| `controller-ram` | SSD1677 RED RAM | 48,000 bytes total |

`host-ram` is the conventional dual-buffer strategy. A differential refresh writes the next frame to BW RAM and the host's previous frame to RED RAM before activation. The host copies the next frame into its previous-frame buffer only after BUSY reports completion.

`controller-ram` is the conventional single-buffer strategy. A differential refresh writes only the next frame to BW RAM before activation because RED RAM already holds the previous frame. After completion, Brewthink seeds RED RAM with the new baseline. The stock-parity path also rewrites BW RAM to match FreeInk's conservative controller synchronization.

`BaselineState` tracks whether either storage strategy is trustworthy. A failed refresh marks it unknown. A requested differential refresh becomes quick-clean whenever the baseline is unknown, including the first refresh after boot.

The `automatic` refresh policy requests differential updates during normal interaction. After 15 successful differential updates it requests one quick-clean update and resets the count. A promoted first update also resets the count because the applied mode, rather than the requested mode, drives policy bookkeeping. Fixed `full-clean`, `quick-clean`, and `differential` policies remain available for hardware comparisons.

Two X4 drive profiles are available:

| Drive profile | Initialization | Full-clean | Quick-clean | Differential |
| --- | --- | ---: | ---: | ---: |
| `openx4-fast-du` | Booster tail `40`, border `01` | `F4` | `D4` with temperature `5A` | `1C` |
| `stock-parity` | Booster tail `80`, border `80` | `F7` | `D7` with temperature `5A` | `FC` |

The reader now defaults to the conservative `stock-parity` drive profile, memory-saving `controller-ram` storage, and mixed `automatic` refresh policy. These defaults have compile-time and command-transcript coverage but are not yet verified on the physical panel. Compare baseline storage while holding the drive profile and refresh mode constant:

```bash
BREWTHINK_X4_DRIVE_PROFILE=stock-parity \
BREWTHINK_PREVIOUS_FRAME_STORAGE=host-ram \
BREWTHINK_DISPLAY_REFRESH=differential \
  scripts/build-reader-app1.sh artifacts/brewthink-reader-stock-parity-host-ram-differential-app1.bin

BREWTHINK_X4_DRIVE_PROFILE=stock-parity \
BREWTHINK_PREVIOUS_FRAME_STORAGE=controller-ram \
BREWTHINK_DISPLAY_REFRESH=differential \
  scripts/build-reader-app1.sh artifacts/brewthink-reader-stock-parity-controller-ram-differential-app1.bin
```

Accepted drive profiles are `openx4-fast-du` and `stock-parity`. Accepted previous-frame storage values are `host-ram` and `controller-ram`. Accepted refresh policies are `automatic`, `full-clean`, `quick-clean`, and `differential`. Building does not touch the device. Non-default combinations have command-transcript tests but are not yet verified on the physical panel.

## Display deep sleep

After a completed refresh and BUSY-low wait, `InitializedSsd1677::enter_deep_sleep` sends command `0x10` with check code `0x03`. The SSD1677 Rev 1.0 command table defines `A[1:0] = 11` as deep-sleep entry. BUSY remains high in this state, so the driver does not wait afterward. Only a hardware reset exits controller deep sleep; normal initialization already begins with that reset.

The `sleep-wake` diagnostic refreshes the orientation pattern, puts the SSD1677 to sleep, verifies both SPI chip selects high, and puts the ESP32-C3 into deep sleep with GPIO3 as an active-low RTC-IO wake source. A GPIO3 wake reboots the chip, hardware-resets the display, refreshes it white, and reports the wake before holding without another sleep cycle. GPIO13 remains untouched.

## Diagnostic stages

The default build uses diagnostic stage `heartbeat` and does not initialize SPI or the display control pins. A display stage must be selected at compile time:

```bash
BREWTHINK_DIAGNOSTIC_STAGE=display-orientation \
BREWTHINK_DISPLAY_ROTATION=270 \
  scripts/build-app1-image.sh artifacts/brewthink-display-rotation-270-app1.bin
```

`BREWTHINK_DISPLAY_ROTATION` accepts `0`, `90`, `180`, or `270` and defaults to `270`. Unsupported values stop before SPI initialization.

Supported stages are cumulative:

| Stage | Last operation |
| --- | --- |
| `display-reset` | Hardware reset only; no SSD1677 command |
| `display-initialize` | Golden initialization and BUSY waits; no explicit refresh |
| `display-write` | Write two white 48,000-byte planes; no activation |
| `display-refresh` | Full white refresh |
| `display-black` | Full black refresh |
| `display-checkerboard` | Full 40 × 40-pixel checkerboard refresh |
| `display-orientation` | Border, axes, and corner labels using `BREWTHINK_DISPLAY_ROTATION` |
| `display-image` | Build-time decoded, scaled, and dithered 1-bit image |

Each diagnostic runs once, reports completion or failure, then holds without retry. Generated patterns and compiled images use a 256-byte transfer buffer rather than a 48 KB RAM framebuffer. See `docs/image-pipeline.md` for image preparation.

Building does not touch hardware. A reviewed image can be written and read back with the guarded app1-only workflow:

```bash
ESPFLASH_PORT=/dev/cu.usbmodemXXXX \
  scripts/flash-app1-and-readback.sh \
  --image artifacts/brewthink-display-rotation-270-app1.bin
```

Do not use `cargo run`. Review the printed image size, write range, and sector erase range before confirming.

## Verified on the physical X4

On 2026-08-30, the guarded workflow wrote and read back each stage only in `app1`. The monitor observed completion without SPI errors, BUSY timeouts, panics, or retries:

1. Reset complete.
2. Initialization and automatic RAM clears complete.
3. Two explicit 48,000-byte white-plane writes complete without activation.
4. White full refresh complete.
5. Black full refresh complete.
6. Checkerboard full refresh complete.
7. Native landscape orientation full refresh complete.
8. 90° logical portrait refresh complete; human inspection found it upside down.
9. Corrected 270° portrait refresh complete and visually confirmed upright.
10. Build-time decoded JPEG full refresh complete.

The corrected orientation image was `100,128` bytes with SHA-256 `cb81a48ed2ffe96e379d3c70d339083b46c6b65db5b2d22f41ca8a1ef2bac890`. Its write range was `0x650000..0x66871F`, wholly inside `app1`. See `docs/image-pipeline.md` for the decoded image result.

`otadata` was not changed during display bring-up. This unit was already configured to boot `app1`; verified stock firmware remains in `app0`.

Controller completion and flash readback are machine-verified. Human inspection confirmed the 270° portrait labels are upright and correctly placed on this unit. Results from one X4 do not establish compatibility across all panel revisions, units, temperatures, or battery voltages.

## References

- OpenX4 Community SDK: `libs/display/EInkDisplay/src/EInkDisplay.cpp`
- MarigoldOS: `display/src/epd/ssd1677.rs` and `fw/src/display_flush/ssd1677.rs`
- Solomon Systech SSD1677 datasheet
- Good Display GDEQ0426T82 panel documentation

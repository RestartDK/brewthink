# Test organization

Brewthink tests application behavior in Rust on the host. Browser tests check the simulator's connection to that code. Firmware builds and physical checks cover what a host cannot reproduce.

## Test ownership

| Layer | Owner | What it proves |
| --- | --- | --- |
| Application and parsers | Rust tests beside `src/app`, `src/device_epub`, `src/bounded_layout`, and `src/simulator` | State transitions, parsing, resource limits, navigation, typography, and cover fallbacks |
| Rendering | `src/ui/render_tests.rs` and component tests | Representative screen pixels and per-row selection behavior |
| Hardware adapters | Rust tests beside display, storage, and X4 modules | Real driver logic against fake buses, clocks, and storage with injected failures |
| Ownership constraints | `src/scratch/compile_tests.rs` | Valid scratch reuse compiles. Oversized, over-aligned, Drop-requiring, and overlapping-borrow cases do not |
| Host commands | Rust CLI tests and `tools/test_*.py` | Argument parsing and real executables communicating through pseudo-terminals |
| Build and recovery tools | `scripts/test_*.py` | Image construction, evidence integrity, protected write boundaries, and failure ordering with fake hardware commands |
| Firmware artifacts | `scripts/check-firmware.sh` | ESP32 release links, image headers and bounds, supported configurations, and limited reader-stack evidence |
| Browser adapters | `web/tests` and `web/parity-tests` | WASM loading, user input, imports, native canvas pixels, grayscale export, and hot reload |
| Physical device | Explicitly authorized bench checks | Real SD, USB, display output, timing, and sleep/wake behavior |

Small Rust unit tests stay in `#[cfg(test)] mod tests`. Larger suites use sibling test modules. All current library suites run with `cargo test --lib`; they are not part of the firmware image. Top-level `tests/` currently holds fixtures, not integration-test crates. Adding `tests/*.rs` requires an explicit host test target and CI invocation because `--lib` does not run them.

The scratch checks invoke `rustc --emit=obj`, not just type checking. Code generation evaluates the generic size, alignment, and Drop assertions. Invalid examples are never executed.

## Dependency injection

`DisplayBus`, `BlockDevice`, `TimeSource`, and `TemporaryFileStore` already separate logic from external operations. Tests keep the real parser, paginator, renderer, and driver. Only external data and hardware are substituted.

Application-state tests assert `AppEffect` values directly. They do not need a fake device client. Browser and pseudo-terminal checks remain useful because an in-process fake cannot verify the browser or operating-system adapter.

## Local commands

Use the repository development shell. For browser checks, first run `bun install --frozen-lockfile` and `bunx playwright install chromium` in `web/`.

| Command from the repository root | Scope |
| --- | --- |
| `scripts/check.sh` | Host tests, formatting, host Clippy, and tooling tests |
| `scripts/check.sh firmware` | Embedded Clippy and six offline image builds |
| `scripts/check.sh web` | Native oracle, production browser tests, and development reload |
| `scripts/check.sh all` | All three groups |

All groups are hardware-free. None flashes, resets, or opens the connected X4.

For a focused Rust test, select the host target explicitly. The repository's default target is the ESP32:

```sh
HOST_TARGET="$(rustc -vV | awk '/^host:/ { print $2 }')"
cargo test --locked --lib --target "$HOST_TARGET" app::
cargo test --locked --lib --features web-sim --target "$HOST_TARGET" simulator::
cargo test --locked --lib --features device-reader --target "$HOST_TARGET" scratch::
```

## Rendering snapshots

`tests/fixtures/ui/*.png` stores native 480 × 800 grayscale pixels. Tests compare decoded pixels, not PNG compression bytes. Failures save the current render in `artifacts/render-diffs/`. To accept an intentional design change, regenerate and inspect the affected PNGs:

```sh
BLESS_UI_FRAMES=1 cargo test --locked --lib --target "$HOST_TARGET" ui::render_tests
```

The snapshots cover screen compositions and edge cases such as empty catalogs and clipped filenames. Component tests cover every drawer and settings selection without a separate snapshot per row.

## Browser scope

The host suite owns exhaustive typography combinations, EPUB limits, navigation formats, and cover outcomes. Native oracle checks still exercise all 17 synthetic EPUB fixtures. Chromium checks representative success, failure, drawer, and sleep paths against the native frames, including both grayscale bitplanes.

The browser suite deliberately does not pin the exact focus-outline color or run a screenshot-only tour. Capture helpers remain available for manual visual review. Browser accessibility, input mapping, full-resolution covers, and native PNG export remain checked.

## Safety and limitations

The reader image builder produces and verifies its own stack evidence. CI retains that image-bound evidence instead of running the same two diagnostic links a second time.

Python remains for Python analyzers and shell orchestration. Moving their tests to Rust without changing the implementation would add a language boundary, not remove one. Pseudo-terminal tests still exercise real host executables. Flash and recovery tests still use fake hardware tools.

Host results do not establish ESP32 runtime stack safety. The stack gate is limited, as described in [reader memory evidence](reader-memory.md). Browser parity can also preserve a bug shared by both callers.

Physical tests require separate authorization and the review required by `AGENTS.md`. A captured framebuffer proves generated pixels, not optical output. Sleep/wake, USB re-enumeration, card faults, and power-loss recovery remain physical verification tasks.

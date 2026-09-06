# Brewthink checkpoint

## Active hardware override, grayscale bench, 2026-09-06

This section supersedes the historical live-firmware state below. Source `main` was at `ef32fbb` when the diagnostic was added. The diagnostic changes and `docs/grayscale-depth.md` remain uncommitted.

- App1 now runs the explicitly triggered grayscale bench, not the reader app. Its USB protocol is `BREWGRAY/1`; normal `scripts/device-control.sh` reader commands do not apply.
- Installed version 3 image `artifacts/grayscale-depth/eight-repeat/bench.bin`, 105,840 bytes, SHA-256 `9024b7218bc2697def152b7a7c7d64a45aac60e3880358bb27a81eb99f3c64a5`.
- Reviewed write range `0x650000..0x669D6F`; sector range `0x650000..0x669FFF`. Guarded flash readback matched. Boot selection, stock app0, other partitions, and SD files were not modified.
- Version 1's black-and-white pattern completed in 4,070 ms and four-state pattern in 1,511 ms. The user's `IMG_1619.HEIC` photograph shows four separated tones in the expected raw-state order. See ignored `artifacts/grayscale-depth/photo-1619/` for the crop and descriptive analysis.
- Version 2's four-state reference completed in 1,523 ms. Its `sixteen-probe` command completed in 2,036 ms. The probe applies two unchanged stock-derived waveforms to sixteen combinations of starting state and adjustment state. It does not establish sixteen optical shades.
- The user's `IMG_1621.HEIC` photograph supplied the eight-tone candidate palette. Several of the sixteen recipes were near-duplicates. See ignored `artifacts/grayscale-depth/photo-1621/` for the selection evidence.
- Version 3 freezes those eight recipes and displays two shuffled copies each. Its four-state conditioning command completed in 1,530 ms and `eight-repeat` in 2,245 ms. The drive sequence and waveform bytes are unchanged from version 2.
- `IMG_1622.HEIC` passes the frozen within-photo separation check. All eight tones retain their expected order across both copies. The smallest normalized P10-to-P90 gap is 0.0101 between the two lightest tones. Mottling remains visible. See ignored `artifacts/grayscale-depth/eight-repeat/plan.json` and `artifacts/grayscale-depth/photo-1622/`.
- The eight-tone repeat is held on the panel. The last completed paint put the display controller to sleep; the ESP32 remains awake for explicit bench commands. No automatic refreshes are scheduled.
- Final version 3 USB status reports `attempts=2`, `faulted=false`, and `probe_attempt=2`. Both probe commands share the consumed one-attempt-per-boot allowance. Both chip selects were high after each paint.
- This is one successful validation render after palette selection. It supports experimental eight-tone output under the tested conditions, not a general reliability guarantee. Sixteen distinct tones and maximum panel depth remain unestablished.
- The current pre-experiment reader differs from the old verified storage image below. Its validated live readback is `backup/grayscale-before/reader.bin`, 503,952 bytes. Use that file for restoration, after reviewing its app1 range. It covers the entire diagnostic write and sector boundary.

See [grayscale depth investigation](docs/grayscale-depth.md) for the pattern geometry, command protocol, evidence, and next measurement. The host work tab is closed; no monitor remains running.

## Historical storage checkpoint

Last updated: 2026-09-06. Git and worktree state checked at 20:04 UTC.

## Current state

- Storage [PR #8](https://github.com/RestartDK/brewthink/pull/8) merged at 19:35:35 UTC as `1481f89cd6a1e87c8069e471910746a30158269b`.
- Local `main` and `origin/main` both point to `1481f89`. The working tree was clean before this documentation update.
- The former 45 local storage changes are now committed on `main`. They are not additional unshipped code.
- The UI-refactor worktree has unfinished integration work. This session has checked its Git status, but has not reviewed, tested, or changed its code.
- The last observed reader state was deep sleep after a successful custom-image refresh. Physical wake on that reader build remains unverified here. The PR merge does not establish that result.

## Storage work completed

The device uses FAT32 through `embedded-sdmmc` 0.10.0. No filesystem replacement or card reformat occurred.

The application creates this logical layout with 8.3-compatible writable names:

```text
/books/
/files/
/brew/cache/
/brew/bookmark/
```

The existing EPUB remains in `/books`. There was no old `/BREWTHINK` directory or image collection to migrate. Long-name creation, exact lowercase presentation, and `/.brew` remain deferred.

Six images were uploaded to `/files`, verified through stream CRC and SD readback, and opened in the device viewer. `NICE.JPG` selection persisted across a firmware reboot. A real Power command then rendered the selected custom image, completed the display refresh, and entered deep sleep.

Other merged capabilities include shared bounded JPEG/PNG decoding, image browsing, Automatic/Custom Image/Book Cover sleep modes, persisted preferences, interrupted-upload recovery, and read-only USB SD-sector exports. Wi-Fi has a defined upload boundary, not a running network stack.

## Why startup failed

There were two distinct failures:

1. `/BREWTHINK` exceeded the filesystem library's eight-character writable basename limit. `/brew` avoids that limitation.
2. The later scan freeze was stack exhaustion. The reader reserved 37,520 bytes before calling a library loader with a 41,872-byte frame. The linked stack had 62,280 bytes available.

Removing `#[inline(always)]` from `run_effect` and `load_chapter` in `src/x4/reader_app.rs` reduced the reader task frame to 1,104 bytes. The same card then completed its book scan and booted.

The suspected FAT cycle was not confirmed. Read-only inspection found matching FAT copies and valid book/directory chains. The same scan succeeded against captured bytes on the host. Five orphan clusters were preserved without repair.

`scripts/check-reader-stack.py` now checks compiled entry frames against the linked stack with an 8 KiB reserve. It catches the known regression but is not whole-program stack analysis.

## Verification record

The storage work passed these checks before merge:

- 109 default library tests, 118 SD-write tests, 143 reader tests, 11 device-control tests, and four stack-check tests.
- Host and embedded Clippy, formatting, WASM/TypeScript build, and the local-system Nix flake check.
- All ten Playwright tests, including the previously skipped screenshot walkthrough. CI now runs that walkthrough.
- All 14 PR CI checks after commit `ffb12ed` normalized `llvm-objdump` output for the stack checker.
- Physical boot, existing EPUB catalog/metadata validation, six image uploads and previews, selection persistence across reboot, and custom sleep-frame generation with completed refresh.

Not yet verified in this session:

- Physical wake and USB re-enumeration on the final storage reader build.
- Selected-image state and EPUB pagination after that wake.
- Every sleep mode end to end on hardware.
- UI-refactor behavior, frame contracts, combined builds, or physical rendering.

The earlier GPIO3 wake diagnostic passed on 2026-08-30. That historical result is not a substitute for the pending reader-build wake check.

## Worktrees

| Worktree | Branch and state at this checkpoint |
| --- | --- |
| `~/Projects/brewthink` | `main`, `1481f89`, documentation-only edits from this update |
| `~/.herdr/worktrees/brewthink/daniel-storage-sleep` | `daniel/storage-sleep`, `ffb12ed`, clean, same tracked tree as merged `main` |
| `~/.herdr/worktrees/brewthink/daniel-ui-refactor` | `daniel/ui-refactor`, `1481f89`, staged integration changes and frame fixtures, no unmerged Git paths |

The UI worktree has no open PR. Seven unmerged paths observed at 20:00 UTC were cleared by the 20:04 check. Git now reports staged integration changes and frame fixtures. That does not establish review, test, or visual parity results.

Recheck that worktree and its ownership before resuming. Preserve its staged work. Do not reset or overwrite it from `main`.

## Last firmware verified in this session

```text
image:              artifacts/brewthink-storage-reader-verified.bin
ELF:                artifacts/brewthink-storage-reader-verified.elf
SHA-256:            75d437688959d96a99f23658d612a9ee56fa9e32860d96000186ebf23515fafb
image size:         503,952 bytes
app1 partition:     0x650000..0xC8FFFF
write byte range:   0x650000..0x6CB08F
sector erase range: 0x650000..0x6CBFFF
```

The flash readback matched the image. Boot selection was already configured for `app1` by an earlier approved workflow. Storage recovery did not change `otadata`, stock `app0`, the bootloader, the partition table, NVS, other flash partitions, or eFuses.

The user granted standing firmware-write permission for the ongoing storage/UI work. Every write still requires the exact image, SHA-256, size, byte range, sector range, and `app1` boundary review. Use the guarded scripts. Do not erase flash, reformat the SD card, or change boot selection as a shortcut.

## Next actions

1. Recheck the UI worktree and preserve its staged integration. Compare the resolved files against merged storage, image, settings, and sleep behavior.
2. Review the UI refactor separately. Verify frame contracts and the simulator walkthrough against the merged storage baseline. Do not regenerate expected frame hashes merely to hide a regression.
3. When the reader is awake, discover its current USB port and use `scripts/device-control.sh`. Verify wake, persisted image selection, and EPUB pagination. Physical Power is required if it is still asleep.
4. Complete the remaining hardware sleep-mode checks. Review any final UI firmware image and its app1 ranges before flashing it.
5. Open and merge the UI PR only after review and verification. Storage PR #8 is already merged and needs no further merge action.

UI review can proceed while hardware wake is pending. No physical actions, conflict resolution, commits, or pushes were performed during this documentation update.

## Evidence and history

- [Roadmap and remaining checks](todo.md).
- [Current development plan](docs/plan.md).
- [SD inspection procedure](docs/sd-recovery.md).
- [Hardware notes](docs/notes.md).
- Local-only continuation details: `artifacts/storage-recovery-handoff.md`.
- Local-only decision trail: `artifacts/sd-recovery-decisions.tsv`.
- Private SD recovery data: `backup/sd-recovery-20260906/`, with a second copy under `$HOME/X4-backups/brewthink-sd-20260906/`. The capture includes metadata and every allocated cluster, not the entire raw card or deleted contents.

Firmware-project research tasks, names, and comparison links have been removed from current notes. Useful technical references remain anonymous. Hardware datasheets, measured behavior, and font license notices remain. Runtime configuration identifiers were not renamed.

The detailed 2026-08-30 diagnostic history remains in Git at `1481f89:checkpoint.md`. Keep raw backups, user pictures, device screenshots, and private logs out of commits.

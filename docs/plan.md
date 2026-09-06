# Brewthink development plan

Updated 2026-09-06. The current state and exact firmware ranges are in [checkpoint.md](../checkpoint.md). The full milestone list is in [todo.md](../todo.md).

## Preserve the recovery path

- Keep the verified full stock backup and a second private copy.
- Preserve the stock bootloader, partition layout, recovery `app0`, NVS, other flash partitions, and eFuses.
- Use the tested X4 board mappings and shared-SPI ownership.
- Build and test each firmware change before reviewing its exact app1 image, checksum, byte range, and sector range.
- Use the guarded write/readback workflow. Keep boot-selection changes separate from normal firmware writes.
- Do not erase flash or use `cargo run` to flash.

## Completed storage phase

Storage [PR #8](https://github.com/RestartDK/brewthink/pull/8) is merged as `1481f89`.

- Keep FAT32 and `embedded-sdmmc` 0.10.0. Use `/books`, `/files`, `/brew/cache`, and `/brew/bookmark` with short writable names.
- Preserve existing books. The six requested images are uploaded and verified.
- Use the shared bounded JPEG/PNG decoder for image previews, covers, and custom sleep frames.
- Keep raw USB SD diagnostics separate from FAT startup and write-enabled reader firmware.
- Check compiled reader-entry stack use after the forced-inlining startup regression.

The selected custom image persisted across reboot and rendered during real sleep. Physical wake on that reader build and the full hardware sleep-mode matrix remain unverified in this session.

## Complete the UI phase

1. Recheck the `daniel/ui-refactor` worktree and its ownership. At 20:04 UTC it was on `1481f89` with staged integration changes and no unmerged Git paths.
2. Preserve that work and check the resolved files for lost storage, image, settings, or sleep behavior.
3. Review the refactor independently against merged storage behavior.
4. Run host and embedded checks, compiled-stack checks, frame contracts, and the full browser walkthrough. Investigate unexpected frame differences before changing expected hashes.
5. Verify reader wake, selected-image persistence, EPUB pagination, and remaining sleep modes on hardware. Verify any changed UI rendering on the X4 with a reviewed app1 build.
6. Open and merge the UI PR after review and passing checks.

UI review does not require the sleeping device to be awake. Hardware checks do. Discover the current USB port after a physical Power press and use `scripts/device-control.sh`.

## Deferred work

Writable long filenames, exact lowercase presentation, `/.brew`, Wi-Fi uploads, additional formats, and OTA automation remain outside the current storage/UI verification work. Do not expand the task into a filesystem replacement or card repair without new evidence and scope.

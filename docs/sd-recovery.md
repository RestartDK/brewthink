# Inspect the SD card without removing it

Keep raw captures under ignored `backup/`. They can contain books, personal images, and deleted file contents. Firmware flash backups do not include the microSD card.

1. Build the read-only USB diagnostic locally:

   ```bash
   scripts/build-storage-usb-app1.sh
   ```

2. Review the generated image, byte count, `app1` write range, and sector range. Flash only after approval, using `scripts/flash-app1-and-readback.sh`. Reset the processor after readback to leave the flashing stub.

3. Query the card while the diagnostic firmware is running:

   ```bash
   ESPFLASH_PORT=/dev/cu.usbmodemXXXX scripts/device-control.sh sd-info
   ```

4. Export a range of 512-byte sectors. This example captures sector zero, not the complete card:

   ```bash
   ESPFLASH_PORT=/dev/cu.usbmodemXXXX \
     scripts/device-control.sh sd-read 0 1 backup/sd-inspection/sector-zero.bin
   ```

The host splits exports into at most eight sectors per request. It checks the returned sector address, count, payload length, CRC32, and terminal success record. Files are private and existing outputs are never replaced. Interrupted exports retain a `.partial` file; they are not complete backups.

The diagnostic answers USB commands before initializing the card. It uses `ReadOnlySdCard` directly, without FAT mounting, catalog scans, automatic directory creation, or SD-write features. It exports bytes over serial; it does not expose a Finder drive. The normal reader firmware does not accept these diagnostic commands.

Use read-only filesystem tools on a local copy. A prefix capture is not a full-card image. Before reconstructing a sparse image, verify that the capture contains all filesystem metadata and every allocated cluster. Zero-filled uncaptured free space does not preserve deleted content. Do not repair or write the physical card based on an incomplete capture.

Rebuild and flash the normal reader after inspection. `scripts/build-reader-app1.sh` also checks the compiled reader-entry stack budget. The check reserves 8 KiB beyond the task and its largest checked entry frame. It catches the known startup regression, but is not whole-program stack analysis.

Power-button deep sleep disconnects USB. Only a physical Power press can wake the sleeping X4 through the configured GPIO3 wake source.

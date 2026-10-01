# Upload EPUBs over USB

Install a reviewed reader build with book-upload support before transferring files. Firmware installation requires separate approval of the app1 image and write range. Do not use `cargo run`.

1. Check the source directory without opening USB:

   ```bash
   scripts/device-control.sh check-books ~/Books
   ```

   This validates the EPUB archives and runs the device package parser and default chapter layout checks. Invalid archives stop the batch before any upload. Reader compatibility warnings do not stop a transfer. A book can copy successfully without being readable or appearing in the Books list. Cover decoding is not part of this check.

2. Wake the reader with its physical Power button and return to Home. Do not interrupt another process using the USB port.

3. Inspect the current device state:

   ```bash
   scripts/device-control.sh --port /dev/cu.usbmodemXXXX status
   ```

4. Upload the EPUB files directly inside the directory:

   ```bash
   scripts/device-control.sh --port /dev/cu.usbmodemXXXX --timeout 900 put-books ~/Books
   ```

   This writes the books to microSD under `/books`. It does not write firmware flash, NVS, OTA data, or eFuses. To upload one file, use `put-book /path/to/book.epub`.

5. Wait for `DONE command=upload status=ok` for every book, then open Books. The firmware refreshes the catalog after each committed book. Unsupported books remain on the card but may not appear in the catalog.

If the host times out after the full payload was acknowledged, do not reset or resend while the device may still be committing. Once the reader responds to `status`, verify the existing SD file without retransmitting it:

```bash
scripts/device-control.sh --port /dev/cu.usbmodemXXXX --timeout 900 verify-book /path/to/book.epub
```

Success requires the stored length, CRC32, and ZIP signature to match the source. Verification opens the file read-only and does not change book content or add or delete files. The FAT library can rewrite allocation metadata when closing a volume, even after file reads. A failed verification is not permission to overwrite or reset the device.

## Filenames and limits

The filesystem library creates only 8.3 filenames. The uploader uses `.EPB`, because `.epub` has four extension characters. EPUB bytes remain unchanged. The reader recognizes both extensions and displays the title and author from EPUB metadata.

A valid short source stem is preserved. Other names become eight uppercase hexadecimal digits derived from the content CRC32, followed by `.EPB`. The host rejects conflicting names within a batch. The device refuses to overwrite a different existing file at the target name. Files with identical content under different target names are not deduplicated against the existing library.

Each book is limited to 32 MiB. The device receives acknowledged 4 KiB chunks and never buffers a whole book. The host snapshots the batch in memory before opening USB. Images retain their separate 96 KiB limit and conversion behavior.

Book uploads require Home. The reader supports at most sixteen catalog entries, sixty-four reading-order entries per book, and 140 KiB per chapter resource. Uploading a book does not remove these limits. The host timeout applies to each transfer exchange and the final commit, not the whole batch. During an upload, firmware polls USB every 250 microseconds instead of the normal 20 milliseconds. The device aborts after 30 seconds without transfer activity.

## Verification and interrupted transfers

The transfer checks the declared length, ZIP signature, and stream CRC32. The device stages bytes in `/brew/UPLOAD.TMP`, checks the SD readback, journals the new target in `/brew/UPLOAD.TXN`, and copies it to `/books`. It verifies the final target before acknowledging success. Publication parsing happens during the catalog refresh, separately from file-transfer verification.

A retry starts the transfer from the beginning. An identical existing target is accepted without replacement. Recovery retains unreadable targets for inspection and removes only an incomplete target identified by a valid journal. Existing image journal bytes and image-upload commands remain compatible.

USB Serial/JTAG carries the protocol. The reader does not appear as a Finder drive.

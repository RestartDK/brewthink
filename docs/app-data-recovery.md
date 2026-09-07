# Application data recovery

`AppDataStore` stores preferences, image selection, and the upload journal under `/brew`. Record reads distinguish three outcomes:

- `Ok(None)` means that the record is absent.
- `Err(AppDataError::InvalidMetadata)` means that a present record is malformed and no valid backup was found.
- Filesystem and device errors remain `Err` with their typed cause. An unreadable primary does not silently select a backup.

A missing or corrupt primary can recover from a valid backup. Updates verify the temporary record, copy only a valid primary into the backup, verify that backup, and then replace and verify the primary. A corrupt primary does not overwrite the previous valid backup. The final check reads the primary directly rather than accept a backup as proof of a successful write.

Upload recovery deletes a named target only when a valid journal identifies it and readable target bytes prove a length, checksum, or format mismatch. A missing target needs no deletion. An I/O failure leaves the journal and target intact. An empty journal has no encoded target identity: recovery removes only that empty journal and the staging file, never a named image. A nonempty corrupt journal remains an error for inspection instead of an instruction to delete data. It blocks image scanning and new uploads until inspected; the code does not guess which named file might be incomplete.

The application-data helpers close FAT volumes explicitly and return close errors. FAT32 volume closure can update FSInfo, even after reads. An operation error takes precedence over a later close error, but all open directories and the volume receive a close attempt.

`AppDataError` implements `Display` and `core::error::Error`. The X4 adapter logs failed metadata reads and catalog scans before it uses retained preferences, the existing image-selection fallback, or an empty catalog. Both `FatStorage::scan()` and the reader’s in-place `scan_into()` distinguish a missing Books directory from an I/O failure; the in-place form clears stale entries before scanning. Firmware orchestration and its broader recovery UI are separate work.

## Host verification

`src/storage/catalog/recovery_tests.rs` constructs a synthetic FAT32 volume and runs the real `embedded-sdmmc` implementation against an in-memory `BlockDevice`. Tests inject read failures and interrupt each sector write in selection updates, preference updates, and image commits. After remount, a valid old or new metadata record remains readable; image recovery retains existing images and selection, and a new target is either complete or absent. Empty-journal recovery cannot delete a named image, while a nonempty corrupt journal preserves all stored bytes. Both book scan APIs are exercised against missing and unreadable directories.

These tests do not use private card data or access hardware. Sector writes are atomic in the test device. The tests do not prove recovery from torn physical sectors, power loss, controller faults, or every filesystem operation.

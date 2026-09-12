# Persistent book identity

`BookId` is a position in the current app catalog. It is suitable for navigation within one boot, not for saved progress. Discovery order can change, and unreadable EPUBs disappear from the validated catalog.

`storage::book_resume::BookIdentity` identifies a file by its complete observed UTF-8 filename under `/books`, plus its byte length. It uses the same long name or short-name fallback that `FatStorage` uses to open the file. Names are bounded to 256 bytes without truncation. Comparison is exact. There is no case folding, Unicode normalization, title matching, or hash-based identity.

## Resolution

The reader derives identities from existing `BookFile` metadata. It does not retain another identity array, read EPUB contents for a digest, or write identity files to SD.

On sleep, `SavedResume::capture` converts a transient book selection into a unique identity. On startup, `SavedResume::resolve` searches the current validated catalog and returns a current `BookId` only for one exact match. The result is typed. Missing and ambiguous identities cannot become a chapter-load request through an index fallback.

Reordering, inserting unrelated books, removing unrelated books, and skipping unreadable entries preserve progress when the saved file remains in the admitted catalog. Duplicate matching entries reject both capture and restore. A book omitted by the sixteen-book scan limit is missing for resume purposes.

Reader progress, Books selections, and book selections in Files all use this identity. Files image selections retain an image-relative index, so changes in book count cannot turn an image selection into a book. Persistent image identity is unchanged and outside this boundary.

## Intentional loss of resume

The reader returns to Home instead of opening another book when the saved identity is missing or ambiguous. Valid RTC preferences remain available, and existing SD preferences keep their normal precedence. A capture failure invalidates the retained record rather than retaining stale progress.

A rename, case-only name change, a different long-name fallback, Unicode spelling change, or file-size change does not restore progress. Identity is scoped to `/books` on the mounted volume, not to a physical SD card. The same name and size on another card denotes the same identity.

Replacing file contents while preserving the name and size also preserves identity. This is a file-locator contract, not a content fingerprint. Semantic pagination and migration after content edits are separate work. The saved chapter and page remain positional within a matched file.

## RTC compatibility and capacity

The new RTC format uses magic `0x42575234` and 74 little-endian u32 words, 296 bytes. The previous format used `0x42575233` and eight words, 32 bytes. All previous index-based records are `LegacyInvalid`, including checksum-valid records and non-reader views. No legacy index bytes become identity bytes. Unknown versions and malformed records are rejected.

The record retains the eight-word header layout. Words 8 and 9 hold filename byte length and file byte length. Words 10 through 73 hold the complete filename with zero padding. Book-bearing records do not store a catalog index in the primary header word. The checksum covers all words except checksum word 7. It detects record corruption, not file identity. Even two filenames with the same 32-bit hash remain distinct because resolution compares their complete names and sizes.

The RTC allocation grows by 264 bytes. The ESP32-C3 linker reserves an 8 KiB RTC fast region. Release linking verifies allocation fit. The reader-stack check covers selected compiled entry frames plus its existing 8 KiB reserve, not the whole call graph or physical wake behavior.

No on-disk format changes or migrations occur. `/brew` preference and image records, upload recovery, read-only book access, and filesystem error handling are unchanged.

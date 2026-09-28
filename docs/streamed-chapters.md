# Streamed chapters

Chapter text no longer has to fit in the 140 KiB resource buffer. The reader keeps verified XHTML and prepared pages on the SD card. Original EPUBs remain unchanged.

## Read path

```text
EPUB entry → bounded ZIP extraction → cached XHTML
                                      ↓
                             incremental XML/layout
                                      ↓
                             page records + index
                                      ↓
                         requested page → image cache → frame
```

Cold preparation finishes and validates the entire chapter before it opens. This preserves exact page counts and rejects malformed tails. Pages are emitted incrementally, but they are not displayed before the completed cache is published. There is no background prefetch or early partial-chapter display.

Page turns read indexed records rather than inflate and parse the chapter again. Typography changes reuse the verified XHTML and build another page cache. Chapter metadata and the 16-title navigation window remain independent of the reusable pixel/text scratch.

## Memory and file bounds

- The resource scratch payload remains **143,360 bytes**. XML state, a page-record buffer, a 4 KiB I/O buffer, and the fixed page index share that allocation. The typed overlay adds bookkeeping/alignment; it is not a larger chapter buffer.
- The existing **96,000-byte** frame/image workspace remains unchanged.
- XML input is read in **1 KiB** chunks. Tokens are limited to **4 KiB**, names to **128 bytes**, and nesting to **64 levels**. Long text, UTF-8 characters, entities, and CDATA can cross input boundaries.
- One extracted chapter is limited to **16 MiB**, one chapter to **8,192 pages**, and one prepared page-cache file to **32 MiB**.
- The storage path uses at most **one volume, three directories, and two files**. During layout, the text reader stays open; the page writer closes before an image probe opens the EPUB.
- The firmware memory gate retains the **8,192-byte reserve**. Its `PASS_LIMITED` result, when obtained for an exact artifact, is not a whole-program stack bound.

Package, navigation, path, image-format, and image-decoder limits still apply. This does not add GIF, progressive JPEG, or interlaced PNG support.

## Persistent cache

Eight-digit hash filenames with `.HTM` and `.PGS` extensions live in the existing cache directory. Filenames are lookup hints, not proof of identity.

The source identity binds the book filename/size, chapter path/length/CRC, and the ZIP central-directory CRC. The directory identity invalidates layout when referenced image metadata changes. Page identity additionally binds reader preferences and the layout version.

The source header is 416 bytes; the page-cache header is 540 bytes. Page fields have explicit little-endian encodings rather than a dump of Rust memory. The header, index, and page payloads have checksums. The source checksum is also checked over the bytes actually consumed by layout. Completion headers are written last. Image probes distinguish storage read errors from unsupported content; read errors abort preparation rather than publish a persistent missing-image placeholder. Incomplete, corrupt, incompatible, or mismatched cache entries cannot be accepted as hits.

Chapter files have a separate **64 MiB / 128-file** logical quota. Eviction protects the current source and output. Image-cache files and unrelated files are excluded. Staging, filesystem metadata, and cluster rounding consume additional space. This is checked cache publication, not crash-consistent FAT metadata; an interrupted metadata update can still leak clusters.

## Simulator and tools

The simulator and native oracle use separate host adapters over the shared streaming parser, layout, and page codec. Each book's simulator adapter retains one staged chapter and one prepared page set; it does not retain every decompressed chapter. Reflow reuses that staged text. The simulator still checks all chapters on import, without retaining all their decoded contents.

Host staging and volatile caches do not establish firmware SRAM usage or SD persistence. FAT-backed tests and device checks cover those separate surfaces.

`check-books` uses streamed text checks instead of the old chapter-buffer limit. It reports compatibility warnings separately from archive/upload validation and does not check image decoding. `inspect-device-epub` streams chapters, checks page-record capacity, and probes inline image geometry.

## Verification

The authored `web/tests/fixtures/streamed-chapters.epub` contains a **756,222-byte** chapter with an illustration, a **225,090-byte** paragraph chapter, and a final short chapter. Regenerate it with `scripts/generate-streamed-chapter-fixture.py`.

Tests cover chunk boundaries, streamed/slice layout equality, page-record round trips, late headings, remount hits, reflow, source/header/index/page corruption, every write boundary in the small cold-preparation fixture, quota eviction, and full-card recovery. An opt-in FAT test reads an unchanged local EPUB:

```sh
BREWTHINK_CHAPTER_EPUB=/path/to/book.epub cargo test --locked --lib \
  --features device-reader --target aarch64-apple-darwin \
  local_epub_streams_every_spine_through_the_persistent_fat_cache -- --ignored --nocapture
```

Use the host target reported by `rustc -vV` on other machines. The opt-in test uses a larger **emulated SD card** to accommodate upload staging and its destination; it does not change any firmware RAM allocation or access the physical device.

Native/WASM traces compare exact packed pixels through chapter return, drawer cancellation, page endpoints, reflow, sleep, and wake. Only authored fixture captures belong in the repository. Real-book inputs, captures, raw device backups, and acceptance logs remain private.

## Physical acceptance, 2026-09-27

The guarded app1 installation and exact artifact are recorded in [the checkpoint](../checkpoint.md). With default Noto Serif / Medium / Normal typography:

| Chapter | XHTML bytes | Pages | One cold cache preparation |
| --- | ---: | ---: | ---: |
| DDIA index, spine 22 | 596,231 | 663 | 26,462 ms |
| Everyday Things index, spine 18 | 175,238 | 147 | 6,876 ms |

These are individual `stage=chapter-cache` records, not statistical or end-to-end page-turn timings. They exclude the separate metadata/navigation work and display refresh. Complete cold preparation is a visible wait; the reader does not show an early partial chapter.

The X4's first and last index pages matched the browser exactly. Hamming/Coffee illustrations also matched, and page/drawer returns preserved pixels. A restart/reopen reused cached Coffee pages and returned the same frame. All four stored uploads matched the unchanged originals before and after the workload; no book was resent or converted.

The full flash matched the post-install snapshot after that workload. Subsequent reset/reopen and sleep checks are outside that comparison. The final sleep tap retained Automatic mode, logged the current Coffee cover CRC, completed, and disconnected USB. This is pre-sleep logical-frame evidence, not a post-sleep or optical panel check. Private SD identity equality remains unverified.

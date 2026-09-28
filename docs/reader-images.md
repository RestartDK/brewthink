# Streaming reader images

Covers, inline EPUB illustrations, Files images, and sleep images share `ImageWorkspace`, `ImageSpec`, packed rendering, and the application-data cache. Originals are never converted or replaced by this pipeline. Automatic/book-cover sleep keeps the current cover contained and uncropped, with the built-in fallback when unavailable. The existing custom-image override remains separate.

## Cold and warm paths

1. Resolve an EPUB image relative to its chapter. A bounded prefix probe supplies dimensions; it does not validate the complete entry.
2. Pagination reserves a whole image block with an explicit vertical position. Large images fit the page; small images retain native size, with packed-width alignment. Text and images share the 50-element page bound.
3. Identify the source and target. A book key includes filename/size, entry path/length/CRC, render dimensions, scaling, and renderer version. Standalone files are CRC-scanned in bounded chunks.
4. Read a valid prepared entry, or stream the ZIP entry to `/brew/cache/IMAGE.TMP`. Full extraction checks compressed consumption, output length, and CRC.
5. Close extraction handles, reuse the 96,000-byte phase workspace for decoding, and produce planar pixels. No full RGB image or encoded EPUB image is held in firmware SRAM.
6. Publish and read back the prepared entry. Compose text and cached image blocks, then draw the reader drawer over them. Resource scratch reused for pixels requires XHTML reload before later layout.

PNG supports non-interlaced applicable 1–16-bit samples, palette/transparency, white alpha compositing, chunk CRCs, zlib validation, and PNG filters. Baseline JPEG uses a reader-backed decoder. Encoded input is bounded to 8 MiB, dimensions to 4,096, and source area to 8 Mi pixels; PNG row capacity can reject images inside those bounds. GIF, progressive JPEG, and interlaced PNG are unsupported. The Files catalog and USB image-upload boundary still cap standalone files at 96 KiB.

Supported `<img>` and SVG `<image>` references use `src`, `href`, or `xlink:href`. Unresolved/unsupported metadata retains alt placeholders. A later decode/cache failure renders an unavailable marker and alt text inside the already-reserved block, without changing pagination.

## Persistent cache

Eight-hex-digit `.IMG` filenames are lookup hints, not proof of identity. The 432-byte record header contains the full 404-byte key, source dimensions, payload length/CRC, header CRC, and completion marker. The key owns its render specification, preventing callers from supplying a different output geometry. The payload is native four-shade planar data, not another compressed image.

Publication writes an invalid header, writes and flushes pixels, then writes the valid checksummed header and verifies readback. Truncated, mismatched, or corrupted records are misses. Entries have no TTL: valid data remains reusable across restarts until identity/integrity changes or quota eviction.

The quota is 32 MiB of logical recognized image-file bytes or 512 entries. A CRC-protected `CLOCK.BIN` cursor drives bounded round-robin eviction. Current-page slots are protected. Other filenames are not eviction candidates. Staging, metadata files, cluster rounding, and filesystem overhead are outside that quota; a full card remains a recoverable preparation failure.

The narrow `embedded-sdmmc` 0.10.0 patch bounds final-sector allocation scans and releases deleted/truncated cluster chains. Without it, eviction and repeated staging leaked storage. Deletion unlinks before freeing. Interrupted FAT metadata writes can still leak clusters: this is checked cache publication, **not crash-consistent FAT**. The patch provenance and licenses live beside the vendored source, and the release evidence binds every vendored file to the source snapshot.

## Verification boundaries

Host tests cover interrupted preparation writes, remount hits, corrupt data/key/header rejection, render-key changes, pinning/quota, unrelated filenames, full-card recovery, and repeated cluster reuse. Layout tests cover placement, whole-image page breaks, SVG links, alt text, and repeatable navigation. The synthetic browser fixture checks every illustration pixel, page return, and cover-only sleep. Regenerate it with `python3 scripts/generate-inline-image-fixture.py`.

The native oracle and WASM share codecs/layout but use separate adapters. Simulator staging and its 16-entry volatile cache are host-memory conveniences, not evidence of SD persistence. Cache hits avoid decoding, resizing, and quantization; standalone source hashing and e-paper refresh still cost time.

Physical acceptance on 2026-09-27 used unchanged EPUBs. Hamming at spine 10, page 19 of 57, and Coffee at spine 8, page 8 of 12, matched their native simulator captures pixel-for-pixel. Page and drawer returns preserved the frames. Coffee also opened spine 20 through the 16-title navigation window and returned to the same illustration.

The 1,236,516-byte DDIA PNG cover took 34,030 ms to prepare and 290 ms on a cache hit. After a processor restart, its hit took 291 ms. The restart checks produced no new prepared entries; DDIA/Coffee covers and the Coffee illustration retained identical pixels. These are observed preparation times, not end-to-end page latency or statistical benchmarks. First preparation can take tens of seconds.

All four uploaded books matched their original byte counts and CRCs before and after cache writes. The sleep test kept Automatic mode unchanged and logged the current Coffee cover CRC before USB disconnected; it matched the opening cover. Native captures prove logical pixels, not optical panel quality or a post-sleep screenshot. Private records and captures remain under ignored `artifacts/reader-images/`.

The reader does not expose the storage diagnostic's raw-SD export. The separate `/TSC3/IDENTITY.BIN` record therefore lacks a post-reader byte comparison. No image or reader command targeted it, and no additional diagnostic was flashed to force that check.

Chapter XHTML remains limited to 140 KiB. The simulator validates all chapters at import, so oversized later chapters reject the whole book there; firmware loads chapters on demand. The unchanged DDIA sample has twelve oversized resources, and Everyday Things has a 175,238-byte index. These limits are not hidden or bypassed. Source-pixel shrinking remains phase-dependent; area-averaged shrinking and zoom/pan are later work.

# Simulator reading parity

Imported EPUBs use `DeviceEpub`, `StreamingZip`, bounded XML/layout, and the native PNG/JPEG decoders. The browser has no separate imported-book parser, paginator, or image decoder. Host inspection and conversion tools remain separate.

`simulator::Book` retains the EPUB and chapter descriptors, with one volatile staged chapter/page cache per book. `Chapter::page` uses the shared streaming XML/layout and page codec. The chapter limit is 16 MiB, with at most 8,192 pages and 128 spine items; the 140 KiB firmware scratch is not a chapter-length limit. EPUB 3 navigation and EPUB 2 NCX supply chapter names; missing names use `Chapter n`. Malformed navigation clears partial names and emits a typed browser warning without rejecting readable text.

## Images and UI

JPEG/PNG encoded data streams through the shared decoder, replacing the former 96/128 KiB EPUB cover gates. The limit is 8 MiB encoded, 4,096-pixel dimensions, and 8 Mi pixels, with a further PNG row-capacity bound. The Books list renders title and author rows and decodes no covers. Opening and book-cover sleep decode the original into a contained 480 × 800 frame, never an enlarged thumbnail. Invalid covers retain the existing fallback behavior.

Inline images reserve whole blocks during pagination. Positioned text and prepared images render before the drawer overlay. Unresolved resources use alt placeholders; later decode failures keep their reserved geometry. A 16-entry in-memory cache reuses prepared pixels, while staging uses host-memory chunks. This is not the device's persistent FAT cache.

Interactive chrome uses monochrome dithering. Reader illustrations, opening covers, image viewing, and sleep retain neutral 0, 85, 170, and 255. Browser CSS does not tint the native bitmap.

## Verification

From the development shell, with web dependencies and Chromium installed:

```sh
bash scripts/check-simulator-parity.sh
```

The script regenerates the 18 deterministic parity EPUBs and compares them with committed fixtures. The excessive-spine case contains 129 entries. A separately authored inline-image fixture contains a 230,728-byte PNG; regenerate it with `python3 scripts/generate-inline-image-fixture.py`. `scripts/generate-streamed-chapter-fixture.py` generates a 756,222-byte chapter with an illustration and a 225,090-byte paragraph chapter. The large fixtures use `simulator-oracle --trace-only` to compare navigation endpoints without saving thousands of full frames.

`simulator-oracle` runs without `web-sim`. It reads through device APIs and uses a separate host staging adapter, image-aware layout, and the shared native renderer. It does not call `simulator::Book` or `Cover`. Playwright compares all 96,000 framebuffer bytes against its references. This verifies the adapter and WASM boundary, not an independent implementation of the codecs or paginator.

Coverage includes:

- Every text and inline-fixture page across two typography configurations and both chapter transitions.
- Drawer drafts, cancel, chapter jumps, backward/forward chapter endpoints, typography, and sleep/wake, including chapters above the former text-buffer limit.
- EPUB 3/NCX titles and missing/malformed navigation fallback.
- PNG/JPEG opening and cover-only sleep, including inputs at and beyond the former encoded gates.
- Malformed XML and excessive spine rejection; formerly oversized XHTML is accepted.
- Series folder grouping, volume order, list paging, row selection, and clipping in host tests.
- Exact illustration pixels, page return, and cover-only sleep in `web/tests/inline-images.spec.ts`.

The parity workflow owns a strict-port production preview. `BREWTHINK_PARITY_PORT` overrides port 4185. It never reuses a development server. Production, development-reload, and parity configurations share port validation and TypeScript checks. Capture native canvas bitmaps, not CSS-scaled element screenshots.

## Limits

This is reading-path parity, not ESP32 emulation. The browser checks all chapter resources at import, retaining only one staged chapter per book; the device prepares chapters on demand. Neither path rewrites or converts EPUB originals. [Streamed chapters](streamed-chapters.md) documents the current bounds and persistence tests.

Browser memory does not demonstrate bounded device SRAM, persistent SD cache reuse, FAT fault handling, USB timing, display waveforms, optical separation, refresh latency, or physical power-loss behavior. Those require separate tests and device acceptance. Source-pixel shrinking remains phase-dependent; zoom/pan and improved area averaging are not part of this change.

# Simulator reading parity

Imported EPUBs use `DeviceEpub`, `StreamingZip`, bounded XML/layout, and the native PNG/JPEG decoders. The browser has no separate imported-book parser, paginator, or image decoder. Host inspection and conversion tools remain separate.

`simulator::Book` owns chapter XHTML bounded to 140 KiB per resource and 128 spine items. `Chapter::page` uses the image-aware bounded layout. EPUB 3 navigation and EPUB 2 NCX supply chapter names; missing names use `Chapter n`. Malformed navigation clears partial names and emits a typed browser warning without rejecting readable text.

## Images and UI

JPEG/PNG encoded data streams through the shared decoder, replacing the former 96/128 KiB EPUB cover gates. The limit is 8 MiB encoded, 4,096-pixel dimensions, and 8 Mi pixels, with a further PNG row-capacity bound. Shelf output is 176 × 264; smaller slots use the shared two-by-two luma average. Opening and book-cover sleep decode the original into a contained 480 × 800 frame, never an enlarged thumbnail. Invalid covers retain the existing fallback behavior.

Inline images reserve whole blocks during pagination. Positioned text and prepared images render before the drawer overlay. Unresolved resources use alt placeholders; later decode failures keep their reserved geometry. A 16-entry in-memory cache reuses prepared pixels, while staging uses host-memory chunks. This is not the device's persistent FAT cache.

Interactive chrome uses monochrome dithering. Reader illustrations, opening covers, image viewing, and sleep retain neutral 0, 85, 170, and 255. Browser CSS does not tint the native bitmap.

## Verification

From the development shell, with web dependencies and Chromium installed:

```sh
bash scripts/check-simulator-parity.sh
```

The script regenerates the 17 deterministic parity EPUBs and compares them with committed fixtures. The excessive-spine case contains 129 entries. A separately authored inline-image fixture contains a 230,728-byte PNG; regenerate it with `python3 scripts/generate-inline-image-fixture.py`.

`simulator-oracle` runs without `web-sim`. It reads through device APIs and uses a separate host staging adapter, image-aware layout, and the shared native renderer. It does not call `simulator::Book` or `Cover`. Playwright compares all 96,000 framebuffer bytes against its references. This verifies the adapter and WASM boundary, not an independent implementation of the codecs or paginator.

Coverage includes:

- Every text and inline-fixture page across two typography configurations and both chapter transitions.
- Drawer drafts, cancel, chapter jumps, position endpoints, typography, and sleep/wake.
- EPUB 3/NCX titles and missing/malformed navigation fallback.
- PNG/JPEG opening and cover-only sleep, including inputs at and beyond the former encoded gates.
- Malformed XML, oversized XHTML, and excessive spine rejection.
- All 256 four-shade two-by-two shelf patterns in host tests.
- Exact illustration pixels, page return, and cover-only sleep in `web/tests/inline-images.spec.ts`.

The parity workflow owns a strict-port production preview. `BREWTHINK_PARITY_PORT` overrides port 4185. It never reuses a development server. Production, development-reload, and parity configurations share port validation and TypeScript checks. Capture native canvas bitmaps, not CSS-scaled element screenshots.

## Limits

This is reading-path parity, not ESP32 emulation. The browser checks all chapter resources at import; the device reads chapters on demand. The unchanged DDIA sample has twelve oversized XHTML resources, and Everyday Things has a 175,238-byte index. Both currently fail simulator import even though their image resources decode successfully. The limits are not bypassed by converting the originals.

Browser memory does not demonstrate bounded device SRAM, persistent SD cache reuse, FAT fault handling, USB timing, display waveforms, optical separation, refresh latency, or physical power-loss behavior. Those require separate tests and device acceptance. Source-pixel shrinking remains phase-dependent; zoom/pan and improved area averaging are not part of this change.

# Brewthink web simulator

The simulator runs Brewthink's shared home, books, files, settings, reader, sleep state, EPUB package parser, cover decoder, and monochrome renderers in WebAssembly. The canvas is the exact 48,000-byte `480 × 800` frame shape used by the X4.

## Run locally

```bash
cd web
bun install
bun run dev
```

The built-in catalog uses public-domain titles and generated covers. Choose or drop a DRM-free EPUB to parse its package metadata and show its declared cover in the first shelf slot. The app opens on Home. Books shows the cover shelf, Files shows source EPUB names and sizes, and Settings changes the reader font, text size, and line spacing. Applied reader settings persist in browser storage. Use the two front rockers, the right-edge buttons, or keyboard arrows. The simulator gets its front-button centers from the same Rust constants as the footer hints. A newly opened book shows only its cover; Confirm starts the text. Missing or invalid covers are skipped, and existing reading checkpoints resume directly. In Reader, Confirm opens a rounded bottom sheet for whole-book position, named chapter selection, and typography. Up and Down choose a row. Left and Right change its value. Confirm applies it; Back cancels. Whole-book position is an approximate equal-chapter estimate, not a global page count. EPUB nav/NCX labels name spine destinations; links to fragments in the same spine item do not create separate chapters.

`bun run dev` watches both sides of the simulator. Vite hot-reloads TypeScript and CSS. Changes to Rust source, Cargo inputs, or the WASM build script trigger an incremental WASM rebuild and a full browser reload. A failed Rust build leaves the last generated module in place and reports the error in the terminal.

## Capture native pixels

Use **Save frame PNG** to export the exact 480 × 800 canvas bitmap. The preview also renders at native size. On narrow screens, scroll the preview rather than shrinking the device pixels.

For UI iteration, keep `bun run dev` running and use its Rust hot reload. Do not use the physical device as the iteration loop. Do not judge bitmap text from CSS-scaled element screenshots.

The Playwright walkthrough uses `tests/capture-frame.ts` to validate opaque black-and-white pixels and export them without resizing.

```bash
BREWTHINK_WALKTHROUGH_DIR=../artifacts/ui-walkthrough bun run test:e2e
python3 ../scripts/render-ui-fixtures.py
```

## Verify

```bash
bun run build
bun run test:e2e
```

The browser tests cover Home, Books, Files, reader settings and reflow, rounded drawers, physical hint alignment, named navigation, whole-book endpoints, cover-only opening, optional-cover failures, sleep and resume, invalid input, narrow layouts, WCAG AA rules, and Rust-triggered WASM reloads. A synthetic 480 × 800 cover verifies every pixel to catch thumbnail enlargement or chrome overlays. The native decoder checks the same fixture. Regenerate the navigation/cover fixtures with `python3 scripts/generate-navigation-fixture.py` from the repository root.

A private acceptance EPUB can be supplied without adding it to the repository:

```bash
BREWTHINK_TEST_EPUB=/path/to/book.epub \
BREWTHINK_SCREENSHOT=/tmp/brewthink-shelf.png \
  bun run test:e2e --grep "parses an EPUB"
```

The simulator's `std` ZIP and image decoders remain a host implementation, separate from the fixed-memory FAT/ZIP/XML/PNG/JPEG pipeline now used by X4 firmware. Both drive the same application state and framebuffer renderers. See [`../docs/epub-reader.md`](../docs/epub-reader.md).

# Brewthink web simulator

The simulator runs Brewthink's shared home, books, files, settings, reader, sleep state, EPUB package parser, cover decoder, and grayscale renderers in WebAssembly. The canvas decodes the same 96,000-byte, four-shade `480 × 800` logical frame used by the X4 reader. UI light gray is a screen-anchored black-and-white dot pattern with black text and outlines. Shelf covers and settings previews are dithered; full-screen covers, image content and sleep images retain genuine grayscale. EPUB body illustrations currently use text placeholders. Native pixels are neutral 0, 85, 170, and 255. Simulated tones do not prove optical separation on the panel.

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

Browser tests use `tests/capture-frame.ts` to accept only neutral logical values 0, 85, 170, and 255. Native captures are saved under `test-results/`. Rust rendering snapshots are directly viewable PNGs under `../tests/fixtures/ui/`.

## Verify

```bash
bun run build
bun run test:e2e
bun run test:e2e:dev
```

`test:e2e` starts a production preview of the assets from `bun run build` on port 4173. It refuses to reuse an existing server. Tests cover production loading, file import, input controls, hint alignment, cover-only opening, image selection, sleep and resume, invalid-input recovery, and an accessibility scan. Exhaustive application and parser cases belong to host Rust tests. The parity suite checks representative reading and drawer flows against native frames. The menu test requires only 0 and 255 in interactive frames, including shelf covers and settings previews. The grayscale test requires exact neutral values 0, 85, 170, and 255 in full-screen image, cover, and sleep frames. A synthetic 480 × 800 cover verifies every pixel to catch thumbnail enlargement or chrome overlays. The native decoder checks the same fixture. Regenerate the navigation and cover fixtures with `python3 scripts/generate-navigation-fixture.py` from the repository root.

`test:e2e:dev` starts a separate development server on port 4174 and checks Rust-triggered WASM reloads. Both commands stop their servers when the tests finish. `BREWTHINK_WEB_PORT` selects an isolated test port. Screenshot-only walkthroughs and exact browser-chrome styling are not release gates. See [test organization](../docs/testing.md).

A private acceptance EPUB can be supplied without adding it to the repository:

```bash
BREWTHINK_TEST_EPUB=/path/to/book.epub \
BREWTHINK_SCREENSHOT=/tmp/brewthink-shelf.png \
  bun run test:e2e --grep "parses an EPUB"
```

Imported books use the device's bounded ZIP/XML/layout and PNG/JPEG pipeline. Chapters are limited to 140 KiB and 64 spine items. Shelf covers accept up to 128 KiB; original-resolution opening/sleep covers accept up to 96 KiB of encoded data. Missing or invalid navigation uses numbered chapter names without rejecting readable text. The browser retains bounded sources and decoded covers in host memory rather than emulating SD reads or SRAM pressure. Run `bash scripts/check-simulator-parity.sh` from the repository root for native-oracle comparisons. See [reading parity and limits](../docs/simulator-parity.md).

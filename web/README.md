# Brewthink web simulator

The simulator runs Brewthink's shared library state, shelf renderer, EPUB package parser, cover decoder, scaler, and monochrome renderer in WebAssembly. The canvas is the exact 48,000-byte `480 × 800` frame shape used by the X4.

## Run locally

```bash
cd web
bun install
bun run dev
```

The built-in catalog uses public-domain titles and generated covers. Choose or drop a DRM-free EPUB to parse its package metadata and show its declared cover in the first shelf slot. Use the on-screen direction pad or keyboard arrow keys to move the selection.

`bun run dev` watches both sides of the simulator. Vite hot-reloads TypeScript and CSS. Changes to Rust source, Cargo inputs, or the WASM build script trigger an incremental WASM rebuild and a full browser reload. A failed Rust build leaves the last generated module in place and reports the error in the terminal.

## Verify

```bash
bun run build
bun run test:e2e
```

The browser tests cover the shared 2 × 2 shelf, directional navigation, synthetic EPUB metadata and cover parsing, invalid input, narrow layouts, WCAG AA rules, and Rust-triggered WASM reloads.

A private acceptance EPUB can be supplied without adding it to the repository:

```bash
BREWTHINK_TEST_EPUB=/path/to/book.epub \
BREWTHINK_SCREENSHOT=/tmp/brewthink-shelf.png \
  bun run test:e2e --grep "parses an EPUB"
```

The simulator's `std` ZIP and image decoders remain a host implementation, separate from the fixed-memory FAT/ZIP/XML/PNG/JPEG pipeline now used by X4 firmware. Both drive the same application state and framebuffer renderers. See [`../docs/epub-reader.md`](../docs/epub-reader.md).

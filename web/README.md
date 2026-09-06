# Brewthink web simulator

The simulator runs Brewthink's shared home, books, files, settings, reader, sleep state, EPUB package parser, cover decoder, and monochrome renderers in WebAssembly. The canvas is the exact 48,000-byte `480 × 800` frame shape used by the X4.

## Run locally

```bash
cd web
bun install
bun run dev
```

The built-in catalog uses public-domain titles and generated covers. Choose or drop a DRM-free EPUB to parse its package metadata and show its declared cover in the first shelf slot. The app opens on Home. Books shows the cover shelf, Files shows source EPUB names and sizes, and Settings changes the reader font, text size, and line spacing. Applied reader settings persist in browser storage. Use the on-screen direction pad or keyboard arrows to move and change values.

`bun run dev` watches both sides of the simulator. Vite hot-reloads TypeScript and CSS. Changes to Rust source, Cargo inputs, or the WASM build script trigger an incremental WASM rebuild and a full browser reload. A failed Rust build leaves the last generated module in place and reports the error in the terminal.

## Verify

```bash
bun run build
bun run test:e2e
bun run test:e2e:dev
```

`test:e2e` starts a production preview of the assets from `bun run build` on port 4173. It refuses to reuse an existing server. The tests cover Home, Books, Files, reader settings and reflow, the shared 2 × 2 shelf, directional navigation, synthetic EPUB metadata and cover parsing, sleep and resume, invalid input, narrow layouts, and WCAG AA rules.

`test:e2e:dev` starts a separate development server on port 4174 and checks Rust-triggered WASM reloads. Both commands stop their servers when the tests finish. Set `BREWTHINK_TEST_PORT` to use a different port when another worktree has a server running. Set `BREWTHINK_WALKTHROUGH_DIR` to save the screenshot walkthrough.

A private acceptance EPUB can be supplied without adding it to the repository:

```bash
BREWTHINK_TEST_EPUB=/path/to/book.epub \
BREWTHINK_SCREENSHOT=/tmp/brewthink-shelf.png \
  bun run test:e2e --grep "parses an EPUB"
```

The simulator's `std` ZIP and image decoders remain a host implementation, separate from the fixed-memory FAT/ZIP/XML/PNG/JPEG pipeline now used by X4 firmware. Both drive the same application state and framebuffer renderers. See [`../docs/epub-reader.md`](../docs/epub-reader.md).

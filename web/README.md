# Brewthink web simulator

The simulator runs Brewthink's Rust image decoder, scaler, and monochrome renderer in WebAssembly. TypeScript owns file selection, the physical X4 preview, browser state, and frame downloads.

## Run locally

```bash
cd web
bun install
bun run dev
```

Open the Vite URL, then choose or drop a JPEG, PNG, BMP, or PNM image. The download button writes the raw 48,000-byte `480 × 800` packed frame used by the current firmware image stage.

`bun run dev` rebuilds the WebAssembly module when it starts. Restart it after changing Rust code.

## Verify

```bash
bun run build
bun run test:e2e
```

To compare a browser-rendered image with an existing host-prepared frame:

```bash
BREWTHINK_TEST_IMAGE=/path/to/source.jpeg \
BREWTHINK_EXPECTED_FRAME=/path/to/source.frame.bin \
bun run test:e2e
```

The browser test checks the canvas dimensions, packed payload size, download path, narrow layout, WCAG AA rules, console errors, and optional byte-for-byte frame equality.

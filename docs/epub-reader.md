# EPUB reader architecture

Brewthink is an EPUB-first reader. The target is content-complete, readable reflow on the X4, not browser-equivalent publisher styling.

## Product contract

Brewthink opens on Home with Books, Files, and Settings. Books contains the 2 × 2 cover shelf. Four covers occupy most of the 480 × 800 frame. The selected book has a stronger border, and its title and creator appear in the footer. Files shows source EPUBs from `/books` plus named JPEG and PNG images from `/files`. Settings changes reader font, text size, line spacing, and sleep-screen mode without changing the Brewthink wordmark or application chrome.

A complete reader must preserve:

- Spine order and table-of-contents navigation.
- Paragraphs, headings, lists, quotations, preformatted text, and horizontal rules.
- Inline emphasis, strong text, code, subscript, and superscript.
- Figures, captions, inline images, and image alternatives.
- Tables, with a dedicated viewer when a table cannot remain legible in normal flow.
- Internal links, external-link labels, footnotes, endnotes, and index navigation.
- All textual content from unknown elements, unless EPUB semantics explicitly hide it.

Unsupported CSS is ignored. Unknown XHTML elements retain and lay out their textual descendants. Script, forms, animation, browser networking, Flexbox, Grid, and publisher-supplied executable behavior are out of scope.

## Boundaries

```text
untrusted .epub bytes
        │ ZIP structure, path, count, size, encryption checks
        ▼
EPUB package
        │ container.xml + OPF parse
        ▼
Publication
  metadata · resources · spine · navigation · cover
        │ resource lookup by typed manifest identity
        ├──────────────► cover decoder ─► shelf image region
        └──────────────► XHTML/CSS parser ─► semantic book flow
                                              │
                                              ▼
                                      bounded paginator
                                              │
                                              ▼
                                     48,000-byte frame
```

`Publication` is the trusted domain boundary. UI and layout code do not inspect raw OPF tags or ZIP paths.

## Implemented vertical slice

The current host, WASM, and X4 paths provide:

- Bounded ZIP entry count and archive/resource size checks.
- Rejection of encrypted entries and parent-directory or absolute paths.
- EPUB mimetype and `META-INF/container.xml` discovery.
- OPF version, title, creators, language, manifest, spine, navigation, and EPUB 2/3 cover discovery.
- Entity-aware XML metadata parsing.
- Cover extraction with CRC verification supplied by the ZIP decoder.
- Bounded PNG/JPEG cover decoding, alpha compositing onto white, center cropping, grayscale conversion, and ordered dithering.
- Immediate conversion of decoded covers into 5,808-byte, 176 × 264 packed shelf bitmaps; no full-color frame is retained.
- Shared Home, Books, Files, Settings, Reader, Error, and Sleep navigation and framebuffer rendering in ordinary Rust tests, WASM, and X4 firmware.
- A shared battery indicator backed by a smoothed voltage estimate on X4 and a fake battery state in WASM.
- Bounded reader typography choices whose resolved metrics drive both pagination and rendering. Noto Serif 14 pt is the default.
- Read-only FAT `/books` discovery, a seekable file adapter, bounded streaming ZIP/DEFLATE, fixed-memory XML, and page-at-a-time XHTML layout on the X4.
- A normal X4 application loop connecting all seven controls, shelf, chapter/page navigation, SSD1677 refresh, retained sleep frame, GPIO3 deep sleep/wake, and checksummed book/chapter/page resume.
- Synthetic EPUB, PNG-alpha, and JPEG fixtures plus private acceptance against every spine item and the cover in the Hamming EPUB.

The simulator's imported books use the same bounded ZIP/XML/layout and PNG/JPEG APIs as the device, with host-owned chapter sources and decoded cover buffers. The X4 implementation uses read-at FAT access, fixed-capacity publication state, incremental DEFLATE, no-heap PNG/JPEG decoding, and statically allocated phase-overlaid workspaces. Checksummed RTC-fast-memory state retains the active screen and reader preferences across deep sleep. The browser checks all chapters at import rather than reading them on demand; see [simulator parity and limits](simulator-parity.md).

## Device memory contract

The ESP32-C3 has no PSRAM. Code running on the X4 must not retain an entire EPUB, compressed resource, decoded full-size cover, chapter DOM, or book-wide pagination map.

The device path uses:

1. A seekable, read-only FAT file capability rooted at `/books`.
2. Repeated bounded central-directory scans that retain no archive-wide heap index.
3. Incremental stored/DEFLATE reads with CRC checks and output limits.
4. Pull-based XML tokenization into fixed-capacity publication and page state.
5. No-heap PNG/JPEG decoding, alpha compositing, resizing, grayscale conversion, and dithering into a packed destination.
6. Page-at-a-time layout with at most 50 retained lines and no chapter DOM.
7. A checksummed RTC-fast-memory resume record.

The final monochrome framebuffer is 48,000 bytes. ZIP inflation and PNG/JPEG decoding share one phase-checked union workspace because they never run concurrently. The release image retains about 50 KiB for stack after a 16-book catalog; no successful allocator exists in firmware.

## Initial limits

Host parsing currently applies these limits:

| Item | Limit |
| --- | ---: |
| EPUB archive | 32 MiB |
| ZIP entries | 2,048 |
| Container XML | 256 KiB |
| OPF package XML | 2 MiB |
| Extracted resource | 16 MiB |
| ZIP entry inflation ratio | 200:1 |
| Simulator cover dimensions | 2,048 × 2,048 |
| Simulator image decoder allocation | 32 MiB |

The X4 path applies lower fixed limits:

| Item | X4 limit |
| --- | ---: |
| Catalogued books | 16 |
| ZIP entries | 1,024 |
| ZIP path | 256 bytes |
| Publication resource path | 128 bytes |
| Container XML | 2 KiB |
| OPF package XML | 64 KiB |
| Extracted resource | 140 KiB |
| Encoded shelf cover | 128 KiB |
| Linear spine items per book | 64 |
| Cached catalog spine paths | 64 / 2 KiB total |
| Manifest items | 512 |
| Retained lines per page | 50 |
| UTF-8 bytes per retained line | 320 |
| PNG cover width | 1,536 pixels |

A limit failure becomes a visible, recoverable book error or a cover placeholder; content is never silently truncated.

## Reading location and persistence

A durable reading location is semantic: book identity, spine resource, and source/token position. Page numbers are derived and can change with fonts or layout settings.

Normal book access stays read-only. The current resume record is versioned by magic and checksummed in RTC fast memory; it stores the active screen, application preferences, catalog index, spine index, and page index. When typography changes, Brewthink maps the saved chapter progress into the new page count. Application preferences also use checksummed primary and backup records under `/brew`. Durable reading progress still needs stable book identity plus semantic source/token position so reflow can return to the exact paragraph.

All firmware-initiated filesystem writes pass through `AppDataStore`. Fixed application records and upload transaction files live under `/brew`; completed images live under `/files`. Firmware creates the 8.3-compatible `/brew`, `/brew/cache`, `/brew/bookmark`, and `/files` directories when missing. The general book/file capability exposes no arbitrary write path.

## Sleep screens

Settings exposes one exhaustive mode:

| Mode | Primary source | Fallback |
| --- | --- | --- |
| Custom Image | Image selected from Files | Built-in screen |
| Book Cover | Cover associated with the current reader or selected book | Built-in screen |
| Automatic | Reader cover only when sleep starts in Reader; custom image everywhere else | Built-in screen |

Files lists up to sixteen uppercase 8.3 JPEG or PNG names from `/files` alongside EPUBs. Opening an image renders a contained full-screen preview. Confirm selects it as the custom sleep source and persists the filename in a checksummed primary and backup record. Sleep rendering center-crops the selected image into the 480 × 800 frame. Missing or invalid selections fall back safely.

The host command accepts ordinary JPEG or PNG sources. Files already within the 96 KiB decoder boundary upload unchanged. Oversized PNGs and progressive JPEGs are resized and converted to bounded baseline JPEGs before transfer. The transfer state machine accepts a typed image name, declared length, format, checksum, and bounded chunks. USB Serial/JTAG supplies the first adapter through `scripts/device-control.sh put-image`. The device writes `UPLOAD.TMP`, verifies it, records `UPLOAD.TXN`, copies to a new named target, verifies SD readback, then removes the transaction. A retry cleans interrupted copies and treats an identical existing target as success; it refuses to overwrite a different file.

`transfer::wifi` defines the future `PUT /api/files/images` request boundary; it does not start a network stack or expose an HTTP server yet.

## Acceptance matrix

The private Hamming EPUB is an acceptance target, not a repository fixture.

| Capability | Current evidence | Remaining device evidence |
| --- | --- | --- |
| EPUB 3 package | All 340 resources and 42 spine items parsed through the fixed-memory reader | Parse the same file through physical FAT |
| Metadata | Bounded XML extraction and physical SD catalog/EPUB metadata validation pass | Verify the catalog after reader wake |
| Cover | `OEBPS/Images/Cover.png`, 143,179 bytes, decoded to packed fingerprint `b8bce90b` | Refresh the physical shelf region |
| 2 × 2 shelf | Shared Rust framebuffer/navigation tests and ten passing Playwright tests, including the full walkthrough | Navigate with physical buttons |
| Chapter text | Every spine document read and first/last page-count consistency checked | Read and turn physical pages |
| 286 PNG images | All fit the current extracted-resource bound | Add inline figures and image viewer |
| Tables and footnotes | Text and alternatives survive fallback layout | Add semantic overlays and dedicated viewers |
| Sleep screens | Shared resolver and simulator cover all modes. Selected custom image persisted across reboot and rendered during real sleep | Verify every mode end to end, GPIO3 wake, and exact physical resume on the final reader build |
| USB upload | Six JPEG uploads passed CRC, SD readback, and device previews. One selected image rendered during real sleep | Verify native PNG upload and interrupted-transfer recovery on hardware |

## Next vertical slice

Storage PR #8 is merged as `1481f89`. Use the existing EPUB in `/books` for the remaining reader-wake and pagination checks rather than copying it again. Review and verify the UI refactor separately against that storage baseline. See [checkpoint.md](../checkpoint.md) for worktree status and the last verified image. Every firmware write still requires exact image and app1-range review. Inline figures, image/table viewers, links, footnotes, and semantic source checkpoints remain subsequent reader-engine work.

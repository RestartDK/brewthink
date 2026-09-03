# EPUB reader architecture

Brewthink is an EPUB-first reader. The target is content-complete, readable reflow on the X4, not browser-equivalent publisher styling.

## Product contract

Brewthink opens on Home with Books, Files, and Settings. Books contains the 2 × 2 cover shelf. Four covers occupy most of the 480 × 800 frame. The selected book has a stronger border, and its title and creator appear in the footer. Files shows the source EPUB names and sizes. Settings changes reader font, text size, and line spacing without changing the Brewthink wordmark or application chrome.

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
- Bounded reader typography choices whose resolved metrics drive both pagination and rendering. Noto Serif 14 pt is the default, matching CrossPoint Reader.
- Read-only FAT `/Books` discovery, a seekable file adapter, bounded streaming ZIP/DEFLATE, fixed-memory XML, and page-at-a-time XHTML layout on the X4.
- A normal X4 application loop connecting all seven controls, shelf, chapter/page navigation, SSD1677 refresh, retained sleep frame, GPIO3 deep sleep/wake, and checksummed book/chapter/page resume.
- Synthetic EPUB, PNG-alpha, and JPEG fixtures plus private acceptance against every spine item and the cover in the Hamming EPUB.

The simulator's `std` ZIP and image decoders remain separate from the device pipeline. The X4 implementation uses read-at FAT access, fixed-capacity publication state, incremental DEFLATE, no-heap PNG/JPEG decoding, and statically allocated phase-overlaid workspaces. Checksummed RTC-fast-memory state now retains the active screen and reader preferences across deep sleep.

## Device memory contract

The ESP32-C3 has no PSRAM. Code running on the X4 must not retain an entire EPUB, compressed resource, decoded full-size cover, chapter DOM, or book-wide pagination map.

The device path uses:

1. A seekable, read-only FAT file capability rooted at `/Books`.
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

Normal book access stays read-only. The current resume record is versioned by magic and checksummed in RTC fast memory; it stores the active screen, reader preferences, catalog index, spine index, and page index. When typography changes, Brewthink maps the saved chapter progress into the new page count. Durable storage must eventually use stable book identity plus semantic source/token position so reflow can return to the exact paragraph. Any future SD writes go through a separate `AppDataStore` restricted to Brewthink-owned files under `/Brewthink`; the general book/file capability must not expose arbitrary writes.

## Acceptance matrix

The private Hamming EPUB is an acceptance target, not a repository fixture.

| Capability | Current evidence | Remaining device evidence |
| --- | --- | --- |
| EPUB 3 package | All 340 resources and 42 spine items parsed through the fixed-memory reader | Parse the same file through physical FAT |
| Metadata | Exact title and creator recovered with bounded XML | Show from microSD catalog |
| Cover | `OEBPS/Images/Cover.png`, 143,179 bytes, decoded to packed fingerprint `b8bce90b` | Refresh the physical shelf region |
| 2 × 2 shelf | Shared Rust framebuffer/navigation tests and six Playwright flows | Navigate with physical buttons |
| Chapter text | Every spine document read and first/last page-count consistency checked | Read and turn physical pages |
| 286 PNG images | All fit the current extracted-resource bound | Add inline figures and image viewer |
| Tables and footnotes | Text and alternatives survive fallback layout | Add semantic overlays and dedicated viewers |
| Sleep cover | X4 app renders a retained cover frame and stores checksummed resume state | Verify deep sleep, GPIO3 wake, and exact physical resume |

## Next vertical slice

Copy the private acceptance EPUB into `/Books` only after explicit removable-media approval, then flash the locally checked reader image only after separate guarded-`app1` approval. Verify shelf → open → page/chapter navigation → retained sleep frame → GPIO3 deep-sleep wake → exact resume one physical action at a time. Inline figures, image/table viewers, links, footnotes, and semantic source checkpoints remain subsequent reader-engine work.

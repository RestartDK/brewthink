# Brewthink design system

## Device scene

A reader uses Brewthink one-handed on a slow 480 × 800 monochrome e-paper display. Controls must remain obvious without animation, color, touch, or frequent refreshes.

## Frame

- Logical size is 480 × 800 pixels.
- Content begins at x = 18 and uses 444 pixels of width.
- Application chrome uses generated Noto Sans: 24 px semibold headings, 22 px control labels, 18 px body text, and 14 px metadata and hints. Measurement and rendering use the same glyph tables.
- Application labels use sentence case. Book titles and filenames retain their source casing. Clipped labels end with an ellipsis at a glyph boundary.
- The application bar has no wordmark or enclosing rule.
- Footer controls sit below a one-pixel rule at y = 730.

## Application bar

The active section occupies the upper-left. A Wi-Fi-off icon, battery icon, and percentage occupy the upper-right. Wi-Fi is off because the firmware does not start a network stack. USB presence removes the capacity fill and adds a lightning bolt inside the outlined battery. The interface does not claim that charge current is flowing.

Reading pages, opening covers, and book-cover sleep frames have no application bar. Confirm reveals the bar with the reader drawer. Battery updates do not trigger a hidden-status reading refresh.

## Selection

Home, Files, Settings, and the reader drawer use borderless rows. A selected row has a black fill, white text and icons, and the shared 12-pixel `ROW_CORNERS` size. Unselected rows have no enclosure. The cover shelf retains outlines around the selected book.

`Icon` in `src/ui/icons.rs` owns the 24 × 24 icon grid and two-pixel strokes. Icons inherit the row foreground color. Books, folders, images, typography, sleep, chapters, and navigation share this set. The smaller battery glyph keeps its bounded capacity and USB-power treatment.

`CommandBar` places hints at x = 100, 192, 300, and 392 in physical front-button order: Back, Confirm, Left, Right. These centers approximate the two front rockers in the [manufacturer's X4 guide illustration](https://www.xteink.com/cdn/shop/files/2_0693b512-beb4-44b0-991a-b3c261e41b1e.jpg?v=1787034936&width=1500); they are not measurements of this physical unit. The simulator imports the same centers from WASM and has two right-edge buttons instead of a D-pad. Each position shows the hardware glyph and the current action. An empty label means no action. The two side buttons select rows. Front Left and Right move through Home and Files, select shelf columns, or change setting values.

## Reader typography

Reader typography is separate from application chrome. The settings are bounded choices:

- Font is Noto Serif, Compact, or Mono.
- Text size is Small, Medium, or Large.
- Line spacing is Compact, Normal, or Relaxed.
- Sleep screen is Automatic, Custom image, or Book cover.

One resolved `ReaderTheme` supplies glyph metrics to both pagination and rendering. The default Noto Serif and Medium combination uses 14 pt text. Application chrome remains on its bitmap fonts.

Reading content occupies y = 24 through 775. Both paginators enforce the shared 50-line memory budget as well as page height. Compact fonts start a new page at that line limit rather than overflowing the device buffer.

## Surfaces and reader drawer

`DrawerSurface` is a bottom sheet, not a horizontal divider. It has an eight-pixel outer inset, shared 28-pixel `PANEL_CORNERS`, and a two-pixel outline. The page behind it is faded with a white checker pattern: the framebuffer remains strictly one-bit. There is no drag handle. The X4 has no touch input, so the sheet uses physical-button hints without swipe or drag affordances. Settings preview panels share `PANEL_CORNERS`; selected rows share `ROW_CORNERS`. Book images retain their original rectangular edges.

Confirm opens `AppView::ReaderDrawer` without turning the page. Its rows are Book position, Chapter, Font, Text size, and Line spacing. Side Up and Down choose rows. Front Left and Right change values. Confirm jumps to the active navigation target or applies typography. Back discards the draft. No chapter loads occur while adjusting a value.

`BookPosition` stores a bounded per-mille position. The slider changes by five percentage points and reaches the first or last page of the whole book. The UI labels the estimate as approximate: each spine chapter receives equal weight, rather than pretending to know a global page count.

Chapter shows its name and ordinal. Both adapters use the shared EPUB 3 nav / EPUB 2 NCX label parser. The first label resolving to a spine resource names that chapter; fragment links within the same resource do not become separate destinations. The device retains one book's bounded 64-byte UTF-8 labels, loaded before chapter text. Missing, malformed, oversized, or unmatched navigation retains chapter fallbacks. This is spine-level selection, not a hierarchical table-of-contents browser.

Typography reflow maps the prior page progress into the new chapter pagination. Sleep discards drawer edits and retains the committed reading location.

## Opening a book

A newly opened book enters `AppView::BookCover`. Only the image is visible. Confirm or Right begins the text; Back or Left returns to the originating Books or Files screen. A missing or undecodable cover proceeds directly to text. Reopening an existing reading checkpoint bypasses the cover. Sleeping on an opening cover resumes at the start of the book.

## Sleep screen

Automatic uses the current book cover only when sleep begins in Reader and uses the selected custom image elsewhere. Custom Image always prefers the selected image. Book Cover uses the associated reader or selected-book cover. Files combines EPUBs from `/books` with images from `/files`; opening an image shows a full-screen preview and Confirm selects it for sleep. Every missing or invalid asset falls back to the built-in sleep screen. Settings shows the selected mode, image name, status, and a bounded preview.

Opening and sleep covers contain only the image, aspect-fitted to the 480 × 800 frame with white margins as needed. No title, author, status bar, or hints overlay it. Full-screen covers are decoded from the original PNG/JPEG, not enlarged from the 176 × 264 shelf cache. The X4 reuses the image decoder's bounded encoded-data buffer and full framebuffer; unsupported dimensions or sizes follow the optional-asset fallback.

The current writable FAT layout uses the 8.3-compatible `/brew`, `/brew/cache`, `/brew/bookmark`, and `/files` paths. Firmware creates missing directories. Application records and transfer state stay under `/brew`; user images stay under `/files`. The filesystem layer will migrate `/brew` to `/.brew` when it can create VFAT long names.

## UI architecture

`App` owns behavior and emits effects. `AppEffect::Render` tells the platform to render the current `AppView`; it does not duplicate the selected screen or reading location. Loading carries its `PendingChapter` inside `AppView::Loading`. Leaving that state discards the request.

`AppFrame` is a borrowed snapshot containing the state and data needed for a shared screen render. Both runtimes pass snapshots to `render_app`. The X4 shelf supplies decoded covers through `ShelfBook` before rendering. The selected cover uses full resolution and other visible covers borrow half-resolution buffers.

Full-screen image decoding remains in-place. The X4 decodes directly into its reusable framebuffer, and `render_image_viewer` overlays shared controls. This avoids retaining another full-size image buffer.

Screens compose short-lived Rust values with `ui!` and `ui_column!`. The macros build `embedded-layout` view chains, draw them directly into `FrameTarget`, and retain no widget tree after the frame is complete. `embedded-graphics` remains the source of geometry, clipping, text, primitives, and `DrawTarget` behavior.

Shared components own recurring visual rules: `AppBar`, `CommandBar`, `DrawerSurface`, `Label`, `MenuRow`, `SettingsRow`, and `FileRow`. `SettingsItem::ALL` supplies the row order for navigation and rendering. `SettingsItem::value` supplies values for device rows and browser metadata. `CustomImagePreview` distinguishes missing and invalid images from a ready image with a name and bitmap.

Semantic `TextRole` values resolve application typography centrally. Reader typography continues to resolve through `ReaderTheme` and never changes application chrome.

Home, Books, Files, Settings, Reader, all five drawer selections, Image, Error, and Sleep renders are pinned as PBM fixtures with exact 48,000-byte pixel payloads. Coverage includes empty catalogs, populated shelf pages, every settings selection, filename clipping, and sleep-image previews. PBM uses the opposite bit polarity from the device framebuffer. A component or layout change must preserve those frames unless the visual change is deliberate and the fixtures are reviewed.

## Simulator

The browser shell remains a restrained developer tool around the exact packed X4 frame. Its warm neutral palette and system typography do not replace or reinterpret the device UI. The canvas always displays the same 48,000-byte frame consumed by the SSD1677 backend. Its backing bitmap and CSS dimensions are both 480 × 800. Narrow viewports scroll the preview instead of shrinking it. The canvas uses black and white pixels; the warm palette belongs to the browser shell.

Save frame PNG and the Playwright capture helper export the canvas bitmap without CSS resampling. UI iteration uses the hot-reloading web simulator, not device flashes. `scripts/render-ui-fixtures.py` converts pinned PBM fixtures to native-size PNGs for inspection.

# Brewthink design system

## Device scene

A reader uses Brewthink one-handed on a slow 480 × 800 monochrome e-paper display. Controls must remain obvious without animation, color, touch, or frequent refreshes.

## Frame

- Logical size is 480 × 800 pixels.
- Content begins at x = 18 and uses 444 pixels of width.
- Application chrome uses `FONT_6X10` and `FONT_9X18_BOLD`.
- Application labels use sentence case. Book titles and filenames retain their source casing.
- The application bar has no wordmark or enclosing rule.
- Footer controls sit below a one-pixel rule at y = 738.

## Application bar

The active section occupies the upper-left. A Wi-Fi-off icon, battery icon, and percentage occupy the upper-right. Wi-Fi is off because the firmware does not start a network stack. USB presence removes the capacity fill and adds a lightning bolt inside the outlined battery. The interface does not claim that charge current is flowing.

Reading pages and book-cover sleep frames have no application bar. Confirm reveals the bar with the reader drawer. Battery updates do not trigger a hidden-status reading refresh.

## Selection

Home, Files, Settings, and the reader drawer use borderless rows. A selected row has a black fill, white text and icons, and an eight-pixel corner size. Unselected rows have no enclosure. The cover shelf retains outlines around the selected book.

`Icon` in `src/ui/icons.rs` owns the 24 × 24 icon grid and two-pixel strokes. Icons inherit the row foreground color. Books, folders, images, typography, sleep, chapters, and navigation share this set. The smaller battery glyph keeps its bounded capacity and USB-power treatment.

`CommandBar` has four equal columns in physical front-button order, Back, Confirm, Left, Right. Each column shows the hardware glyph and the current action. An empty label means no action. The two side buttons select rows. Front Left and Right move through Home and Files, select shelf columns, or change setting values.

## Reader typography

Reader typography is separate from application chrome. The settings are bounded choices:

- Font is Noto Serif, Compact, or Mono.
- Text size is Small, Medium, or Large.
- Line spacing is Compact, Normal, or Relaxed.
- Sleep screen is Automatic, Custom image, or Book cover.

One resolved `ReaderTheme` supplies glyph metrics to both pagination and rendering. The default Noto Serif and Medium combination uses 14 pt text. Application chrome remains on its bitmap fonts.

Reading content occupies y = 24 through 775. Both paginators enforce the shared 50-line memory budget as well as page height. Compact fonts start a new page at that line limit rather than overflowing the device buffer.

## Reader drawer

Confirm opens `AppView::ReaderDrawer` without turning the page. The drawer overlays the lower page and reveals the status header. Its rows are Page in chapter, Chapter, Font, Text size, and Line spacing.

The Page slider previews a jump within the current chapter in steps of roughly five percent. Chapter selects an EPUB spine section by number. Named EPUB table-of-contents navigation is not implemented. Side Up and Down choose rows. Front Left and Right change values. Confirm jumps to the active navigation target or applies typography. Back discards the draft. No chapter loads occur while adjusting a value.

Typography reflow maps the prior page progress into the new chapter pagination. Sleep discards drawer edits and retains the committed reading location.

## Sleep screen

Automatic uses the current book cover only when sleep begins in Reader and uses the selected custom image elsewhere. Custom Image always prefers the selected image. Book Cover uses the associated reader or selected-book cover. Files combines EPUBs from `/books` with images from `/files`; opening an image shows a full-screen preview and Confirm selects it for sleep. Every missing or invalid asset falls back to the built-in sleep screen. Settings shows the selected mode, image name, status, and a bounded preview.

Book-cover sleep frames contain only the cover, fitted to 480 × 720 with white margins above and below. No title, author, status bar, or button hints overlay the image. The current cover cache is 176 × 264, so full-screen display enlarges that bitmap rather than decoding new detail.

The current writable FAT layout uses the 8.3-compatible `/brew`, `/brew/cache`, `/brew/bookmark`, and `/files` paths. Firmware creates missing directories. Application records and transfer state stay under `/brew`; user images stay under `/files`. The filesystem layer will migrate `/brew` to `/.brew` when it can create VFAT long names.

## UI architecture

`App` owns behavior and emits effects. `AppEffect::Render` tells the platform to render the current `AppView`; it does not duplicate the selected screen or reading location. Loading carries its `PendingChapter` inside `AppView::Loading`. Leaving that state discards the request.

`AppFrame` is a borrowed snapshot containing the state and data needed for a shared screen render. Both runtimes pass snapshots to `render_app`. The X4 shelf supplies decoded covers through `ShelfBook` before rendering. The selected cover uses full resolution and other visible covers borrow half-resolution buffers.

Full-screen image decoding remains in-place. The X4 decodes directly into its reusable framebuffer, and `render_image_viewer` overlays shared controls. This avoids retaining another full-size image buffer.

Screens compose short-lived Rust values with `ui!` and `ui_column!`. The macros build `embedded-layout` view chains, draw them directly into `FrameTarget`, and retain no widget tree after the frame is complete. `embedded-graphics` remains the source of geometry, clipping, text, primitives, and `DrawTarget` behavior.

Shared components own recurring visual rules: `AppBar`, `CommandBar`, `Label`, `MenuRow`, `SettingsRow`, and `FileRow`. `SettingsItem::ALL` supplies the row order for navigation and rendering. `SettingsItem::value` supplies values for device rows and browser metadata. `CustomImagePreview` distinguishes missing and invalid images from a ready image with a name and bitmap.

Semantic `TextRole` values resolve application typography centrally. Reader typography continues to resolve through `ReaderTheme` and never changes application chrome.

Home, Books, Files, Settings, Reader, all five drawer selections, Image, Error, and Sleep renders are pinned as PBM fixtures with exact 48,000-byte pixel payloads. Coverage includes empty catalogs, populated shelf pages, every settings selection, filename clipping, and sleep-image previews. PBM uses the opposite bit polarity from the device framebuffer. A component or layout change must preserve those frames unless the visual change is deliberate and the fixtures are reviewed.

## Simulator

The browser shell remains a restrained developer tool around the exact packed X4 frame. Its warm neutral palette and system typography do not replace or reinterpret the device UI. The canvas always displays the same 48,000-byte frame consumed by the SSD1677 backend. Its backing bitmap and CSS dimensions are both 480 × 800. Narrow viewports scroll the preview instead of shrinking it. The canvas uses black and white pixels; the warm palette belongs to the browser shell.

Save frame PNG and the Playwright capture helper export the canvas bitmap without CSS resampling. UI iteration uses the hot-reloading web simulator, not device flashes. `scripts/render-ui-fixtures.py` converts pinned PBM fixtures to native-size PNGs for inspection.

# Brewthink design system

## Device scene

A reader uses Brewthink one-handed on a slow 480 × 800 monochrome e-paper display. Controls must remain obvious without animation, color, touch, or frequent refreshes.

## Frame

- Logical size is 480 × 800 pixels.
- Content begins at x = 18 and uses 444 pixels of width.
- Application chrome uses `FONT_6X10` and `FONT_9X18_BOLD`.
- The Brewthink wordmark never inherits reader typography.
- A two-pixel rule at y = 58 closes the application bar.
- Footer controls sit below a rule near the bottom of the frame.

## Application bar

The wordmark occupies the upper-left. The active section appears beneath it. A battery icon and percentage occupy the upper-right on Home, Books, Files, Settings, Reader, Error, and Sleep screens. USB presence removes the capacity fill and adds a lightning bolt inside the outlined battery. The interface does not claim that charge current is flowing.

## Selection

A one-pixel outline is neutral. A three or four-pixel outline is selected. Selection never depends on color. Up and Down move through vertical lists. Left and Right change settings values. Confirm opens or applies. Back returns to the parent screen.

## Reader typography

Reader typography is separate from application chrome. The settings are bounded choices:

- Font is Noto Serif, Compact, or Mono.
- Text size is Small, Medium, or Large.
- Line spacing is Compact, Normal, or Relaxed.
- Sleep screen is Automatic, Custom Image, or Book Cover.

One resolved `ReaderTheme` supplies glyph metrics to both pagination and rendering. The default Noto Serif and Medium combination matches CrossPoint Reader's Noto Serif 14 pt default. The Brewthink wordmark and application chrome remain on their existing bitmap fonts.

## Sleep screen

Automatic uses the current book cover only when sleep begins in Reader and uses the selected custom image elsewhere. Custom Image always prefers the selected image. Book Cover uses the associated reader or selected-book cover. Files combines EPUBs from `/books` with images from `/files`; opening an image shows a full-screen preview and Confirm selects it for sleep. Every missing or invalid asset falls back to the built-in Brewthink screen. Settings shows the selected mode, image name, status, and a bounded preview.

The current writable FAT layout uses the 8.3-compatible `/brew`, `/brew/cache`, `/brew/bookmark`, and `/files` paths. Firmware creates missing directories. Application records and transfer state stay under `/brew`; user images stay under `/files`. The filesystem layer will migrate `/brew` to `/.brew` when it can create VFAT long names.

## UI architecture

`App` owns behavior and emits effects. `AppEffect::Render` tells the platform to render the current `AppView`; it does not duplicate the selected screen or reading location. Loading carries its `PendingChapter` inside `AppView::Loading`. Leaving that state discards the request.

`AppFrame` is a borrowed snapshot containing the state and data needed for a shared screen render. Both runtimes pass snapshots to `render_app`. The X4 shelf supplies decoded covers through `ShelfBook` before rendering. The selected cover uses full resolution and other visible covers borrow half-resolution buffers.

Full-screen image decoding remains in-place. The X4 decodes directly into its reusable framebuffer, and `render_image_viewer` overlays shared controls. This avoids retaining another full-size image buffer.

Screens compose short-lived Rust values with `ui!` and `ui_column!`. The macros build `embedded-layout` view chains, draw them directly into `FrameTarget`, and retain no widget tree after the frame is complete. `embedded-graphics` remains the source of geometry, clipping, text, primitives, and `DrawTarget` behavior.

Shared components own recurring visual rules: `AppBar`, `CommandBar`, `Label`, `MenuRow`, `SettingsRow`, and `FileRow`. `SettingsItem::ALL` supplies the row order for navigation and rendering. `SettingsItem::value` supplies values for device rows and browser metadata. `CustomImagePreview` distinguishes missing and invalid images from a ready image with a name and bitmap.

Semantic `TextRole` values resolve application typography centrally. Reader typography continues to resolve through `ReaderTheme` and never changes application chrome.

Home, Books, Files, Settings, Reader, Image, Error, and Sleep renders are pinned as PBM fixtures with exact 48,000-byte pixel payloads. Coverage includes empty catalogs, populated shelf pages, every settings selection, filename clipping, and sleep-image previews. PBM uses the opposite bit polarity from the device framebuffer. A component or layout change must preserve those frames unless the visual change is deliberate and the fixtures are reviewed.

## Simulator

The browser shell remains a restrained developer tool around the exact packed X4 frame. Its warm neutral palette and system typography do not replace or reinterpret the device UI. The canvas always displays the same 48,000-byte frame consumed by the SSD1677 backend.

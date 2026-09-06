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

## Simulator

The browser shell remains a restrained developer tool around the exact packed X4 frame. Its warm neutral palette and system typography do not replace or reinterpret the device UI. The canvas always displays the same 48,000-byte frame consumed by the SSD1677 backend.

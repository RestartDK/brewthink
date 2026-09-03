# Reader font source

Brewthink's default EPUB typeface is Noto Serif at 14 pt, matching CrossPoint Reader's default reader family and size.

The regular and bold source files came from CrossPoint Reader commit `b95965f475e6b7075bf7c5e8d260e0e73f17d4b8`:

- `lib/EpdFont/builtinFonts/source/NotoSerif/NotoSerif-Regular.ttf`
- `lib/EpdFont/builtinFonts/source/NotoSerif/NotoSerif-Bold.ttf`

`tools/generate-reader-font.py` rasterizes 12, 14, and 16 pt variants at CrossPoint's 150 DPI and generates the checked-in one-bit tables in `src/fonts/noto_serif.rs`.

Noto Serif is licensed under the SIL Open Font License 1.1. See `OFL.txt`.

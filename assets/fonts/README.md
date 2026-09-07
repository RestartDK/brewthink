# Font sources

Brewthink's default EPUB typeface is Noto Serif at 14 pt.

The regular and bold source files came from an upstream reader repository at commit `b95965f475e6b7075bf7c5e8d260e0e73f17d4b8`:

- `lib/EpdFont/builtinFonts/source/NotoSerif/NotoSerif-Regular.ttf`
- `lib/EpdFont/builtinFonts/source/NotoSerif/NotoSerif-Bold.ttf`

`tools/generate-reader-font.py` rasterizes 12, 14, and 16 pt variants at 150 DPI and generates the checked-in one-bit tables in `src/fonts/noto_serif.rs`.

Noto Serif is licensed under the SIL Open Font License 1.1. See `OFL.txt`.

## Application UI

Noto Sans Regular and SemiBold come from [notofonts/noto-fonts](https://github.com/notofonts/noto-fonts/tree/ffebf8c1ee449e544955a7e813c54f9b73848eac/hinted/ttf/NotoSans), pinned at `ffebf8c1ee449e544955a7e813c54f9b73848eac`. The source files and `OFL-NotoSans.txt` are included here.

```sh
uv run --script tools/generate-reader-font.py --ui
```

This generates `src/fonts/noto_sans.rs` with 14, 18, and 22 px Regular and 24 px SemiBold. These are pixel sizes, independent of the reader's point-size preferences. Both foreground colors use the same glyph masks; proportional glyph advances drive clipping, wrapping, and button-hint centering.

#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["freetype-py==2.5.1"]
# ///

from pathlib import Path
import subprocess

import freetype

ROOT = Path(__file__).resolve().parents[1]
FONT_DIR = ROOT / "assets" / "fonts"
OUTPUT = ROOT / "src" / "fonts" / "noto_serif.rs"
POINT_SIZES = (12, 14, 16)
STYLES = ("Regular", "Bold")
CODEPOINTS = tuple(
    sorted(
        {
            *range(0x20, 0x7F),
            *range(0xA0, 0x180),
            *range(0x2010, 0x203B),
            *range(0x20A0, 0x20D0),
        }
    )
)
DPI = 150
THRESHOLD = 96


def packed_bitmap(bitmap: freetype.Bitmap) -> list[int]:
    pixels = list(bitmap.buffer)
    packed = [0] * ((bitmap.width * bitmap.rows + 7) // 8)
    for index, value in enumerate(pixels):
        if value >= THRESHOLD:
            packed[index // 8] |= 0x80 >> (index % 8)
    return packed


def rust_array(name: str, type_name: str, values: list[str], columns: int) -> str:
    lines = [f"static {name}: [{type_name}; {len(values)}] = ["]
    for offset in range(0, len(values), columns):
        lines.append("    " + ", ".join(values[offset : offset + columns]) + ",")
    lines.append("];")
    return "\n".join(lines)


def generate_font(point_size: int, style: str) -> tuple[str, str, str]:
    face = freetype.Face(str(FONT_DIR / f"NotoSerif-{style}.ttf"))
    face.set_char_size(point_size * 64, 0, DPI, 0)
    glyphs: list[str] = []
    bitmaps: list[int] = []
    for codepoint in CODEPOINTS:
        face.load_char(chr(codepoint), freetype.FT_LOAD_RENDER)
        slot = face.glyph
        bitmap = slot.bitmap
        offset = len(bitmaps)
        bitmaps.extend(packed_bitmap(bitmap))
        advance = max(1, round(slot.advance.x / 64))
        top = round(face.size.ascender / 64) - slot.bitmap_top
        glyphs.append(
            "BitmapGlyph::new("
            f"{offset}, {bitmap.width}, {bitmap.rows}, {slot.bitmap_left}, {top}, {advance}"
            ")"
        )

    prefix = f"NOTO_SERIF_{point_size}_{style.upper()}"
    glyph_table = rust_array(f"{prefix}_GLYPHS", "BitmapGlyph", glyphs, 1)
    bitmap_table = rust_array(
        f"{prefix}_BITMAP", "u8", [f"0x{value:02X}" for value in bitmaps], 16
    )
    font = (
        f"pub(crate) static {prefix}: BitmapFont = BitmapFont::new(\n"
        "    &NOTO_SERIF_CODEPOINTS,\n"
        f"    &{prefix}_GLYPHS,\n"
        f"    &{prefix}_BITMAP,\n"
        f"    {round(face.size.height / 64)},\n"
        ");"
    )
    return glyph_table, bitmap_table, font


def main() -> None:
    codepoints = [f"0x{codepoint:04X}" for codepoint in CODEPOINTS]
    sections = [
        "use super::{BitmapFont, BitmapGlyph};",
        rust_array("NOTO_SERIF_CODEPOINTS", "u16", codepoints, 12),
    ]
    for point_size in POINT_SIZES:
        for style in STYLES:
            sections.extend(generate_font(point_size, style))
    OUTPUT.write_text("\n\n".join(sections) + "\n")
    subprocess.run(["rustfmt", "--edition", "2024", str(OUTPUT)], check=True)


if __name__ == "__main__":
    main()

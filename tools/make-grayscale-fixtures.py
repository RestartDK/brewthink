#!/usr/bin/env python3
"""Regenerate synthetic ramps and the grayscale EPUB. Requires ImageMagick."""

from pathlib import Path
import struct
import subprocess
import zipfile
import zlib


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "web/tests/fixtures"
SHADES = (0, 36, 73, 109, 146, 182, 219, 255)


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def ramp(width, height):
    row = b"\0" + b"".join(bytes((0, 0, 0, 255 - SHADES[x * 8 // width])) for x in range(width))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(row * height))
        + chunk(b"IEND", b"")
    )


def main():
    png = FIXTURES / "gray-ramp.png"
    png.write_bytes(ramp(64, 16))
    subprocess.run([
        "magick", str(png), "-background", "white", "-alpha", "remove", "-alpha", "off",
        "-strip", "-quality", "100", str(FIXTURES / "gray-ramp.jpg"),
    ], check=True)
    with zipfile.ZipFile(FIXTURES / "minimal.epub") as source, zipfile.ZipFile(FIXTURES / "gray-cover.epub", "w") as target:
        covers = 0
        for entry in source.infolist():
            data = source.read(entry)
            if entry.filename.endswith(".png"):
                data = ramp(176, 264)
                covers += 1
            info = zipfile.ZipInfo(entry.filename, date_time=(2020, 1, 1, 0, 0, 0))
            info.compress_type = entry.compress_type
            target.writestr(info, data)
        if covers != 1:
            raise RuntimeError(f"Expected one synthetic cover, found {covers}")


if __name__ == "__main__":
    main()

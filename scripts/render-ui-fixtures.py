#!/usr/bin/env python3
import argparse
from pathlib import Path
import struct
import zlib


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def png_frame(pbm):
    header = b"P4\n480 800\n"
    if not pbm.startswith(header) or len(pbm) != len(header) + 48000:
        raise ValueError("Expected a packed 480 x 800 UI fixture")
    payload = pbm[len(header):]
    rows = bytearray()
    for y in range(800):
        rows.append(0)
        for byte in payload[y * 60:(y + 1) * 60]:
            rows.extend(0 if byte & (1 << bit) else 255 for bit in range(7, -1, -1))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", 480, 800, 8, 0, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows))
            + chunk(b"IEND", b""))


def main():
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description="Export UI fixture pixels to native-size monochrome PNGs without resizing.")
    parser.add_argument("--source", type=Path, default=root / "tests/fixtures/ui")
    parser.add_argument("--output", type=Path, default=root / "artifacts/ui-fixtures")
    args = parser.parse_args()
    fixtures = sorted(args.source.glob("*.pbm"))
    if not fixtures:
        parser.error("No PBM fixtures found")
    args.output.mkdir(parents=True, exist_ok=True)
    for fixture in fixtures:
        destination = args.output / (fixture.stem + ".png")
        destination.write_bytes(png_frame(fixture.read_bytes()))
        print(destination)


if __name__ == "__main__":
    main()

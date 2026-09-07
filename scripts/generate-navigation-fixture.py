#!/usr/bin/env python3
from pathlib import Path
import struct
import zlib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def main() -> None:
    pixels = b"".join(
        b"\0" + bytes(0 if x % 8 == 0 or (x < 240 and y % 8 == 0) else 255 for x in range(480))
        for y in range(800)
    )
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 480, 800, 8, 0, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b"")
    (ROOT / "web/tests/fixtures/navigation-cover.png").write_bytes(png)
    entries = {
        "mimetype": b"application/epub+zip",
        "META-INF/container.xml": b'<container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>',
        "OPS/book.opf": b'<package version="3.0"><metadata><title>A small journey</title><creator>Fixture Author</creator></metadata><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="three" href="three.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/><itemref idref="three"/></spine></package>',
        "OPS/nav.xhtml": b'<html><body><nav epub:type="toc"><ol><li><a href="one.xhtml#start">A quiet morning</a></li><li><a href="two.xhtml">Along the river</a></li><li><a href="three.xhtml">The way home</a></li></ol></nav></body></html>',
        "OPS/cover.png": png,
    }
    for index, name in enumerate(["one", "two", "three"], 1):
        text = f"<html><body><h1>Section {index}</h1>" + "".join(f"<p>Passage {n}: The path follows the river, past the old bridge and into the quiet woods.</p>" for n in range(60)) + "</body></html>"
        entries[f"OPS/{name}.xhtml"] = text.encode()
    for name, invalid_cover in [("navigation-cover.epub", False), ("invalid-cover.epub", True)]:
        with zipfile.ZipFile(ROOT / "web/tests/fixtures" / name, "w") as archive:
            for path, data in entries.items():
                if invalid_cover and path == "OPS/cover.png":
                    data = b"not a PNG"
                archive.writestr(zipfile.ZipInfo(path, (2020, 1, 1, 0, 0, 0)), data)


if __name__ == "__main__":
    main()

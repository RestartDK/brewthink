#!/usr/bin/env python3
"""Generate an authored EPUB with a large encoded four-tone illustration."""
from pathlib import Path
import struct
import zlib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


rows = bytearray()
for y in range(240):
    rows.append(0)
    for x in range(320):
        value = [0, 85, 170, 255][(x - 16) // 72] if 16 <= x < 304 and 24 <= y < 216 else 255
        rows.extend([value] * 3)
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 320, 240, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(rows, level=0)) + chunk(b"IEND", b"")

files = {
    "mimetype": b"application/epub+zip",
    "META-INF/container.xml": b'<container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>',
    "OPS/book.opf": b'''<package><metadata><title>Streaming illustrations</title><creator>Brewthink fixture</creator></metadata><manifest><item id="cover" href="images/diagram.png" media-type="image/png" properties="cover-image"/><item id="one" href="text/one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="text/two.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>''',
    "OPS/nav.xhtml": b'<html><nav type="toc"><a href="text/one.xhtml">Illustration</a><a href="text/two.xhtml">Next chapter</a></nav></html>',
    "OPS/text/one.xhtml": b'<html><body><h1>A real image block</h1><p>Before the illustration.</p><figure><img src="../images/diagram.png" alt="Four tone diagram"/><figcaption>Figure 1. Four tones from a streamed PNG.</figcaption></figure><p>After the illustration.</p></body></html>',
    "OPS/text/two.xhtml": b'<html><body><h1>Next chapter</h1><p>Go back to reuse the prepared illustration.</p></body></html>',
    "OPS/images/diagram.png": png,
}
path = ROOT / "web/tests/fixtures/inline-images.epub"
with zipfile.ZipFile(path, "w") as archive:
    for name, data in files.items():
        entry = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
        entry.compress_type = zipfile.ZIP_STORED
        entry.external_attr = 0o644 << 16
        archive.writestr(entry, data)
print(f"{path.relative_to(ROOT)}: {len(png)} encoded image bytes")

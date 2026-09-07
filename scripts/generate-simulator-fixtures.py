#!/usr/bin/env python3
import io
from pathlib import Path
import sys
import zipfile

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "artifacts/simulator-parity"
OUTPUT.mkdir(parents=True, exist_ok=True)


def epub(name, chapter, *, spine_count=2, cover=None):
    entries = {
        "mimetype": b"application/epub+zip",
        "META-INF/container.xml": b'<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
    }
    manifest = ''.join(f'<item id="s{i}" href="s{i}.xhtml" media-type="application/xhtml+xml"/>' for i in range(spine_count))
    spine = ''.join(f'<itemref idref="s{i}"/>' for i in range(spine_count))
    if cover is not None:
        media_type = "image/jpeg" if cover.startswith(b'\xff\xd8') else "image/png"
        manifest += f'<item id="cover" href="cover" properties="cover-image" media-type="{media_type}"/>'
        entries["OPS/cover"] = cover
    entries["OPS/book.opf"] = f'<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="book-id">urn:brewthink:parity</dc:identifier><dc:title>Parity &amp;amp; literal</dc:title><dc:creator>Fixture Author</dc:creator><dc:language>en</dc:language></metadata><manifest>{manifest}</manifest><spine>{spine}</spine></package>'.encode()
    for index in range(spine_count):
        entries[f"OPS/s{index}.xhtml"] = chapter if index == 0 else b'<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Second chapter</h1><p>End of book.</p></body></html>'
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        for path, data in entries.items():
            info = zipfile.ZipInfo(path, date_time=(2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            archive.writestr(info, data)
    (OUTPUT / name).write_bytes(buffer.getvalue())


text = b'<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Bounded text parity</h1><p><![CDATA[A &amp; B, C & D]]></p><pre>first\n    second\n\n\tthird</pre>'
text += b'<p>' + b'a ' * 11 + b'deve<em>lop</em>ment</p>'
text += b'<p>development</p>' * 24
text += b'<p>' + b'longword' * 40 + b'</p></body></html>'
png = (ROOT / "web/tests/fixtures/transparent.png").read_bytes()
jpeg = (ROOT / "web/tests/fixtures/cover.jpg").read_bytes()
epub("text.epub", text, cover=png)
epub("jpeg.epub", text, cover=jpeg)
epub("no-cover.epub", text)
epub("oversized-cover.epub", text, cover=b'x' * (128 * 1024 + 1))
epub("unsupported-cover.epub", text, cover=b'not an image')
epub("broken-cover.epub", text, cover=b'\x89PNG\r\n\x1a\ntruncated')
epub("malformed.epub", b'<html><body><p>wrong</other></body></html>')
epub("oversized-chapter.epub", b'<body>' + b'x' * (140 * 1024) + b'</body>')
epub("too-many-chapters.epub", b'<body>text</body>', spine_count=65)
print(f"Generated nine synthetic EPUBs in {OUTPUT}")

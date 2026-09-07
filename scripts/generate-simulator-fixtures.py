#!/usr/bin/env python3
import io
from pathlib import Path
import struct
import sys
import zipfile
import zlib

ROOT = Path(__file__).resolve().parent.parent


def archive_bytes(entries, bloated_cover=False):
    body = bytearray()
    directory = bytearray()
    for name, data in entries.items():
        encoded_name = name.encode()
        method = 0
        payload = data
        if name == "OPS/cover" and bloated_cover:
            assert len(data) < 65536
            method = 8
            payload = b"\x00\x00\x00\xff\xff" * 26215
            payload += b"\x01" + struct.pack("<HH", len(data), len(data) ^ 0xFFFF) + data
        crc = zlib.crc32(data)
        offset = len(body)
        body += struct.pack(
            "<IHHHHHIIIHH", 0x04034B50, 20, 0, method, 0, 33,
            crc, len(payload), len(data), len(encoded_name), 0,
        ) + encoded_name + payload
        directory += struct.pack(
            "<IHHHHHHIIIHHHHHII", 0x02014B50, 20, 20, 0, method, 0, 33,
            crc, len(payload), len(data), len(encoded_name), 0, 0, 0, 0, 0, offset,
        ) + encoded_name
    result = bytes(body + directory + struct.pack(
        "<IHHHHIIH", 0x06054B50, 0, 0, len(entries), len(entries),
        len(directory), len(body), 0,
    ))
    with zipfile.ZipFile(io.BytesIO(result)) as archive:
        assert archive.testzip() is None
        assert {name: archive.read(name) for name in archive.namelist()} == entries
    return result


def epub(chapter, *, spine_count=2, cover=None, navigation="nav", bloated_cover=False):
    entries = {
        "mimetype": b"application/epub+zip",
        "META-INF/container.xml": b'<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
    }
    manifest = ''.join(f'<item id="s{i}" href="s{i}.xhtml" media-type="application/xhtml+xml"/>' for i in range(spine_count))
    spine = ''.join(f'<itemref idref="s{i}"/>' for i in range(spine_count))
    spine_attributes = ""
    names = ["Opening", "Closing"] + [f"Chapter {i + 1}" for i in range(2, spine_count)]
    if navigation == "ncx":
        manifest += '<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>'
        spine_attributes = ' toc="ncx"'
        points = ''.join(f'<navPoint id="n{i}"><navLabel><text>{names[i]}</text></navLabel><content src="s{i}.xhtml#start"/></navPoint>' for i in range(spine_count))
        entries["OPS/toc.ncx"] = f'<ncx><navMap>{points}</navMap></ncx>'.encode()
    elif navigation is not None:
        manifest += '<item id="nav" href="nav.xhtml" properties="nav" media-type="application/xhtml+xml"/>'
        links = ''.join(f'<li><a href="s{i}.xhtml#start">{names[i]}</a></li>' for i in range(spine_count))
        if navigation == "malformed":
            links = '<li><a href="s0.xhtml">Opening</a></li><li><a href="s1.xhtml">Broken</bad></li>'
        entries["OPS/nav.xhtml"] = f'<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol>{links}</ol></nav></body></html>'.encode()
    if cover is not None:
        media_type = "image/jpeg" if cover.startswith(b'\xff\xd8') else "image/png"
        manifest += f'<item id="cover" href="cover" properties="cover-image" media-type="{media_type}"/>'
        entries["OPS/cover"] = cover
    entries["OPS/book.opf"] = f'<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="book-id">urn:brewthink:parity</dc:identifier><dc:title>Parity &amp;amp; literal</dc:title><dc:creator>Fixture Author</dc:creator><dc:language>en</dc:language></metadata><manifest>{manifest}</manifest><spine{spine_attributes}>{spine}</spine></package>'.encode()
    for index in range(spine_count):
        entries[f"OPS/s{index}.xhtml"] = chapter if index == 0 else b'<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Second chapter</h1><p>End of book.</p></body></html>'
    return archive_bytes(entries, bloated_cover)


def padded_png(png, size):
    padding_length = size - len(png) - 12
    assert padding_length >= 0
    assert png[-12:-4] == b"\x00\x00\x00\x00IEND"
    chunk = b"paDd" + bytes(padding_length)
    padded = png[:-12] + struct.pack(">I", padding_length) + chunk + struct.pack(">I", zlib.crc32(chunk)) + png[-12:]
    assert len(padded) == size
    return padded


def fixtures():
    text = b'<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>Bounded text parity</h1><p><![CDATA[A &amp; B, C & D]]></p><pre>first\n    second\n\n\tthird</pre>'
    text += b'<p>' + b'a ' * 11 + b'deve<em>lop</em>ment</p>'
    text += b'<p>development</p>' * 24
    text += b'<p>' + b'longword' * 40 + b'</p></body></html>'
    png = (ROOT / "web/tests/fixtures/transparent.png").read_bytes()
    jpeg = (ROOT / "web/tests/fixtures/cover.jpg").read_bytes()
    return {
        "text": epub(text, cover=png),
        "jpeg": epub(text, cover=jpeg),
        "no-cover": epub(text),
        "frame-limit": epub(text, cover=padded_png(png, 96 * 1024)),
        "shelf-only-cover": epub(text, cover=padded_png(png, 96 * 1024 + 1)),
        "shelf-limit": epub(text, cover=padded_png(png, 128 * 1024)),
        "oversized-cover": epub(text, cover=padded_png(png, 128 * 1024 + 1)),
        "compressed-oversized-cover": epub(text, cover=png, bloated_cover=True),
        "unsupported-cover": epub(text, cover=b'not an image'),
        "broken-cover": epub(text, cover=b'\x89PNG\r\n\x1a\ntruncated'),
        "broken-jpeg": epub(text, cover=jpeg[:len(jpeg) // 2]),
        "malformed-nav": epub(text, navigation="malformed"),
        "no-nav": epub(text, navigation=None),
        "ncx": epub(text, navigation="ncx"),
        "malformed": epub(b'<html><body><p>wrong</other></body></html>'),
        "oversized-chapter": epub(b'<body>' + b'x' * (140 * 1024) + b'</body>'),
        "too-many-chapters": epub(b'<body>text</body>', spine_count=65),
    }


def main():
    if len(sys.argv) > 2:
        raise SystemExit("usage: generate-simulator-fixtures.py [output-directory]")
    output = Path(sys.argv[1]) if len(sys.argv) == 2 else ROOT / "web/tests/fixtures/parity"
    output.mkdir(parents=True, exist_ok=True)
    generated = fixtures()
    for name, data in generated.items():
        (output / f"{name}.epub").write_bytes(data)
    print(f"Generated {len(generated)} deterministic synthetic EPUBs in {output}")


if __name__ == "__main__":
    main()

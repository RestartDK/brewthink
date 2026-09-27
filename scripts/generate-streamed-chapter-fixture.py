#!/usr/bin/env python3
from pathlib import Path
from zipfile import ZIP_STORED, ZipFile, ZipInfo

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "web/tests/fixtures"


def main():
    entries = [
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", b'<container><rootfiles><rootfile full-path="EPUB/book.opf"/></rootfiles></container>'),
        ("EPUB/book.opf", b'<package><metadata><title>Streamed chapters</title><creator>Brewthink</creator></metadata><manifest><item id="long" href="long.xhtml" media-type="application/xhtml+xml"/><item id="paragraph" href="paragraph.xhtml" media-type="application/xhtml+xml"/><item id="end" href="end.xhtml" media-type="application/xhtml+xml"/><item id="figure" href="figure.png" media-type="image/png"/></manifest><spine><itemref idref="long"/><itemref idref="paragraph"/><itemref idref="end"/></spine></package>'),
    ]
    paragraphs = [f'<p>Entry {index:05d}. A bounded reader keeps this sentence intact across buffers. The words båten, 文, and café keep their characters. <em>Inline text</em> stays in its paragraph.</p>' for index in range(4200)]
    paragraphs.insert(0, '<h1>Across the old limit</h1><p>Before the illustration.</p><img src="figure.png" alt="Four tones"/><p>After the illustration.</p>')
    paragraphs.append('<p>FINAL CHAPTER MARKER. The complete chapter reached its end.</p>')
    long = ('<html><body>' + ''.join(paragraphs) + '</body></html>').encode()
    assert 600 * 1024 < len(long) < 1024 * 1024
    paragraph = ('<html><body><h1>One long paragraph</h1><p>' + 'A word and å. ' * 15000 + '</p><p>FINAL PARAGRAPH MARKER.</p></body></html>').encode()
    with ZipFile(FIXTURES / "inline-images.epub") as source:
        image_path = next(name for name in source.namelist() if name.endswith('.png'))
        figure = source.read(image_path)
    entries += [
        ("EPUB/long.xhtml", long),
        ("EPUB/paragraph.xhtml", paragraph),
        ("EPUB/end.xhtml", b'<html><body><h1>Last chapter</h1><p>The following chapter remains reachable.</p></body></html>'),
        ("EPUB/figure.png", figure),
    ]
    output = FIXTURES / "streamed-chapters.epub"
    with ZipFile(output, 'w', compression=ZIP_STORED) as archive:
        for name, data in entries:
            info = ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = ZIP_STORED
            archive.writestr(info, data)
    print(f"Generated {output.name}; XHTML bytes: {len(long)}, {len(paragraph)}")


if __name__ == '__main__':
    main()

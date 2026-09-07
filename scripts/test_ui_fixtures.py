from pathlib import Path
import runpy
import struct
import unittest
import zlib

png_frame = runpy.run_path(str(Path(__file__).with_name("render-ui-fixtures.py")))["png_frame"]


class UiFixtureTests(unittest.TestCase):
    def test_keeps_native_dimensions_and_pbm_polarity(self):
        png = png_frame(b"P4\n480 800\n" + bytes([0xAA]) * 48000)
        self.assertEqual(png[:8], b"\x89PNG\r\n\x1a\n")
        self.assertEqual(struct.unpack(">II", png[16:24]), (480, 800))
        offset = 8
        compressed = bytearray()
        while offset < len(png):
            length = struct.unpack(">I", png[offset:offset + 4])[0]
            kind = png[offset + 4:offset + 8]
            data = png[offset + 8:offset + 8 + length]
            crc = struct.unpack(">I", png[offset + 8 + length:offset + 12 + length])[0]
            self.assertEqual(crc, zlib.crc32(kind + data))
            if kind == b"IDAT":
                compressed.extend(data)
            offset += 12 + length
        self.assertEqual(zlib.decompress(compressed), (b"\0" + bytes([0, 255]) * 240) * 800)

    def test_rejects_truncated_or_wrong_size_frames(self):
        for value in [b"", b"P4\n480 800\n", b"P4\n800 480\n" + bytes(48000)]:
            with self.assertRaises(ValueError):
                png_frame(value)


if __name__ == "__main__":
    unittest.main()

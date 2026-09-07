import concurrent.futures
import os
from pathlib import Path
import pty
import select
import subprocess
import tempfile
import termios
import time
import unittest
import zlib


ROOT = Path(__file__).resolve().parents[1]


class Peer:
    def __init__(self, fd):
        self.fd = fd
        self.buffer = bytearray()
        self.deadline = time.monotonic() + 15
        os.set_blocking(fd, False)

    def wait(self, writable=False):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("pseudo-terminal peer timed out")
        readers, writers, _ = select.select([] if writable else [self.fd], [self.fd] if writable else [], [], remaining)
        if not readers and not writers:
            raise TimeoutError("pseudo-terminal peer timed out")

    def write(self, data):
        while data:
            self.wait(writable=True)
            count = os.write(self.fd, data[:16384])
            data = data[count:]

    def read(self, count):
        while len(self.buffer) < count:
            self.wait()
            self.buffer.extend(os.read(self.fd, 16384))
        result = bytes(self.buffer[:count])
        del self.buffer[:count]
        return result

    def line(self):
        while b"\n" not in self.buffer:
            self.wait()
            self.buffer.extend(os.read(self.fd, 16384))
        return self.read(self.buffer.index(b"\n") + 1)


class DeviceControlCliTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.control = Path(os.environ["DEVICE_CONTROL_BIN"]).resolve()
        cls.prepare = Path(os.environ["PREPARE_IMAGE_BIN"]).resolve()
        if not cls.control.is_file() or not cls.prepare.is_file():
            raise RuntimeError("Build device-control and prepare-image before these tests")

    def client(self, arguments, serve):
        master, slave = pty.openpty()
        original_termios = termios.tcgetattr(slave)
        try:
            peer = Peer(master)
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
                server = executor.submit(serve, peer)
                result = subprocess.run([
                    str(self.control), "--port", os.ttyname(slave), "--timeout", "10", *map(str, arguments)
                ], capture_output=True, timeout=20)
                try:
                    server.result(timeout=20)
                except Exception as error:
                    raise AssertionError(f"peer failed; client stdout={result.stdout!r}, stderr={result.stderr!r}") from error
                restored = termios.tcgetattr(slave)
                # PENDIN is kernel input-reprocessing state, not a terminal setting.
                pending = getattr(termios, "PENDIN", 0)
                restored[3] &= ~pending
                original_termios[3] &= ~pending
                self.assertEqual(restored, original_termios)
                return result
        finally:
            os.close(master)
            os.close(slave)

    def test_screenshots_round_trip_legacy_four_and_eight_tones(self):
        for bits, quantizer in [(1, "threshold"), (2, "gray4"), (3, "gray8")]:
            with self.subTest(bits=bits), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "screen.png"
                packed = Path(directory) / "frame.bin"
                preview = Path(directory) / "preview.pnm"
                frame = b"".join(bytes([pattern]) * 48000 for pattern in [0x55, 0x33, 0x0F][:bits])

                def serve(peer):
                    self.assertEqual(peer.line(), b"BREWCTL/1 screen\n")
                    extra = "" if bits == 1 else f" bpp={bits} encoding=planar"
                    header = f"BREWCTL/1 SCREEN width=480 height=800 bytes={len(frame)} crc32={zlib.crc32(frame):08x}{extra}\n"
                    peer.write(header.encode() + frame + b"\nBREWCTL/1 DONE command=screen status=ok\n")

                result = self.client(["screen", output], serve)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                conversion = subprocess.run([
                    str(self.prepare), str(output), str(packed), str(preview), "480", "800", "contain", quantizer
                ], capture_output=True, timeout=20)
                self.assertEqual(conversion.returncode, 0, conversion.stderr.decode())
                self.assertEqual(packed.read_bytes(), frame)

    def test_bad_screen_checksum_never_writes_a_png(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "screen.png"
            frame = bytes(96000)

            def serve(peer):
                self.assertEqual(peer.line(), b"BREWCTL/1 screen\n")
                header = f"BREWCTL/1 SCREEN width=480 height=800 bytes=96000 crc32={zlib.crc32(frame) ^ 1:08x} bpp=2 encoding=planar\n"
                peer.write(header.encode() + frame)

            result = self.client(["screen", output], serve)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"checksum mismatch", result.stderr)
            self.assertFalse(output.exists())

    def test_upload_uses_the_advertised_eight_tone_image_limit(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "ramp.png"
            source.write_bytes((ROOT / "web/tests/fixtures/gray-ramp.png").read_bytes() + bytes(64 * 1024))
            limit = 56 * 1024

            def serve(peer):
                self.assertEqual(peer.line(), b"BREWCTL/1 status\n")
                peer.write(f"BREWCTL/1 IMAGE_PROFILE tones=8 max_image_bytes={limit}\nBREWCTL/1 STATUS view=home selected=0\nBREWCTL/1 DONE command=status status=ok\n".encode())
                command = peer.line().decode().strip().split()
                self.assertEqual(command[:3], ["BREWCTL/1", "upload", "image"])
                self.assertEqual(command[3], "RAMP.JPG")
                length = int(command[4])
                self.assertLessEqual(length, limit)
                peer.write(f"BREWCTL/1 READY command=upload chunk=4096 bytes={length}\n".encode())
                received = bytearray()
                while len(received) < length:
                    received.extend(peer.read(min(4096, length - len(received))))
                    peer.write(f"BREWCTL/1 ACK command=upload received={len(received)}\n".encode())
                self.assertTrue(received.startswith(b"\xff\xd8"))
                self.assertEqual(zlib.crc32(received), int(command[5], 16))
                peer.write(b"BREWCTL/1 DONE command=upload status=ok\n")

            result = self.client(["put-image", source], serve)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            self.assertIn(b"limit=57344 transcoded=true", result.stdout)


if __name__ == "__main__":
    unittest.main()

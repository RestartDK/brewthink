import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import struct
import subprocess
import tempfile
import unittest
import zlib

ROOT = Path(__file__).resolve().parent.parent
FAKE_TOOL = r'''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as log:
    log.write(json.dumps([Path(sys.argv[0]).name, *args]) + "\n")
if Path(sys.argv[0]).name == "esptool":
    if "flash-id" in args:
        print("Manufacturer: 85\nDevice: 2018")
    elif "image-info" in args:
        if os.environ.get("INVALID_IMAGE") == "1":
            raise SystemExit("invalid image")
        print("ESP32-C3 Image Header\nFlash size: 16MB\nFlash freq: 80m\nFlash mode: DIO\nChip ID: 5 (ESP32-C3)\nChecksum: aa (valid)\nValidation hash: aa (valid)\nApplication Information\nProject name: brewthink")
    else:
        raise SystemExit("unexpected esptool command")
elif args[0] == "board-info":
    print("Chip type: esp32c3\nFlash size: " + os.environ.get("FAKE_SIZE", "16MB") + "\nCrystal frequency: 40 MHz\nSecure Boot: Disabled\nFlash Encryption: Disabled")
    if "MUTATE_SOURCE" in os.environ:
        Path(os.environ["MUTATE_SOURCE"]).write_bytes(b"changed")
elif args[0] == "write-bin":
    offset, source = args[-2:]
    if os.environ.get("FAIL_WRITE") == "1":
        raise SystemExit(1)
    with open(os.environ["FAKE_FLASH"], "r+b") as flash:
        flash.seek(int(offset, 0))
        flash.write(Path(source).read_bytes())
    if "--monitor" in args:
        with open(os.environ["FAKE_LOG"], "a") as log:
            log.write(json.dumps(["espflash", "monitor"]) + "\n")
elif args[0] == "read-flash":
    offset, size, destination = args[-3:]
    with open(os.environ["FAKE_FLASH"], "rb") as flash:
        flash.seek(int(offset, 0))
        data = flash.read(int(size, 0))
    if os.environ.get("CORRUPT_READBACK") == "1":
        data = bytes([data[0] ^ 1]) + data[1:]
    Path(destination).write_bytes(data)
elif args[0] == "erase-region":
    offset, size = map(lambda value: int(value, 0), args[-2:])
    with open(os.environ["FAKE_FLASH"], "r+b") as flash:
        flash.seek(offset)
        flash.write(b"\xff" * size)
elif args[0] not in ("monitor", "reset"):
    raise SystemExit("unexpected espflash command")
'''


def ota(sequence, state=0xFFFFFFFF):
    data = bytearray(b"\xff" * 0x2000)
    struct.pack_into("<I", data, 0, sequence)
    struct.pack_into("<II", data, 24, state, zlib.crc32(data[:4], 0xFFFFFFFF) & 0xFFFFFFFF)
    return data


class FlashSafetyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        shutil.copytree(ROOT / "scripts", self.root / "scripts")
        (self.root / "docs").mkdir()
        shutil.copy(ROOT / "docs/x4-stock-partition-table.csv", self.root / "docs")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for tool in ("espflash", "esptool"):
            executable = self.bin / tool
            executable.write_text(FAKE_TOOL)
            executable.chmod(0o755)
        self.log = self.root / "commands.jsonl"
        self.flash = self.root / "flash.bin"
        self.flash.write_bytes(b"\xff" * 0x1000000)
        self.image = self.root / "image.bin"
        self.image.write_bytes(b"initial")
        elf = self.root / "target/riscv32imc-unknown-none-elf/release/brewthink"
        elf.parent.mkdir(parents=True)
        elf.write_bytes(b"test-elf")
        self.backup = self.root / "stock.bin"
        stock = bytearray(b"\xff" * 0x1000000)
        stock[0xE000:0x10000] = ota(1)
        stock[0x10000:0x10007] = b"stock!!"
        self.backup.write_bytes(stock)
        self.env = {**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}",
                    "ESPFLASH_PORT": "FAKE-PORT", "FAKE_FLASH": str(self.flash),
                    "FAKE_LOG": str(self.log)}
        self.env.pop("ESPTOOL_PORT", None)

    def run_script(self, name, *args, **env):
        return subprocess.run(["bash", str(self.root / "scripts" / name), *args],
                              env={**self.env, **env}, input="", text=True,
                              capture_output=True, timeout=20)

    def commands(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def assert_success(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def stock_args(self):
        return ("--stock-flash-backup", str(self.backup), "--backup-sha256",
                hashlib.sha256(self.backup.read_bytes()).hexdigest(), "--yes")

    def test_monitor_follows_readback(self):
        result = self.run_script("flash-app1-and-readback.sh", "--image", str(self.image), "--yes", "--monitor")
        self.assert_success(result)
        actions = [command[1] for command in self.commands() if command[0] == "espflash"]
        self.assertLess(actions.index("read-flash"), actions.index("monitor"))

    def test_reviewed_image_is_not_replaced_by_a_concurrent_build(self):
        result = self.run_script("flash-app1-and-readback.sh", "--image", str(self.image), "--yes",
                                 MUTATE_SOURCE=str(self.image))
        self.assert_success(result)
        with self.flash.open("rb") as flash:
            flash.seek(0x650000)
            self.assertEqual(flash.read(7), b"initial")

    def test_failed_readback_never_monitors_or_resets(self):
        result = self.run_script("flash-app1-and-readback.sh", "--image", str(self.image), "--yes", "--monitor",
                                 CORRUPT_READBACK="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[1] in ("monitor", "reset") for command in self.commands()))

    def test_failed_write_never_reads_back(self):
        result = self.run_script("flash-app1-and-readback.sh", "--image", str(self.image), "--yes", FAIL_WRITE="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[1] in ("read-flash", "monitor", "reset") for command in self.commands()))

    def test_erase_is_disabled_even_with_yes(self):
        self.assertNotEqual(self.run_script("erase-app1.sh", "--yes").returncode, 0)
        self.assertEqual(self.commands(), [])

    def test_wrong_hardware_never_writes(self):
        result = self.run_script("flash-app1-and-readback.sh", "--image", str(self.image), "--yes", FAKE_SIZE="4MB")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[1] == "write-bin" for command in self.commands()))

    def test_missing_port_never_probes_hardware(self):
        result = self.run_script("flash-app1-and-readback.sh", "--image", str(self.image), "--yes", ESPFLASH_PORT="")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[0] == "espflash" for command in self.commands()))

    def test_wrong_backup_digest_never_writes(self):
        result = self.run_script("restore-stock-app0.sh", "--stock-flash-backup", str(self.backup),
                                 "--backup-sha256", "0" * 64, "--yes")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[1] == "write-bin" for command in self.commands()))

    def test_write_boundary_rejects_protected_offset(self):
        result = subprocess.run(["bash", "-c", 'source "$1"; private_workspace; explicit_port; write_and_verify 0x0 7 "$2"',
                                 "test", str(self.root / "scripts/common.sh"), str(self.image)],
                                env=self.env, capture_output=True, text=True, timeout=20)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.commands(), [])

    def test_restore_requires_reviewed_digest(self):
        result = self.run_script("restore-stock-state.sh", "--stock-flash-backup", str(self.backup), "--yes")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[1] == "write-bin" for command in self.commands()))

    def test_stock_restore_preserves_app1(self):
        with self.flash.open("r+b") as flash:
            flash.seek(0x650000)
            flash.write(b"keep-app1")
        result = self.run_script("restore-stock-state.sh", *self.stock_args())
        self.assert_success(result)
        writes = [command[-2] for command in self.commands() if command[1] == "write-bin"]
        self.assertEqual(writes, ["0x10000", "0xE000"])
        with self.flash.open("rb") as flash:
            flash.seek(0x650000)
            self.assertEqual(flash.read(9), b"keep-app1")
        self.assertFalse(any(command[1] == "erase-region" for command in self.commands()))

    def test_stock_restore_rejects_app1_selection_before_any_write(self):
        with self.backup.open("r+b") as backup:
            backup.seek(0xE000)
            backup.write(ota(2))
        result = self.run_script("restore-stock-state.sh", *self.stock_args())
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[1] == "write-bin" for command in self.commands()))

    def test_stock_restore_readback_failure_never_changes_boot_selection(self):
        result = self.run_script("restore-stock-state.sh", *self.stock_args(), CORRUPT_READBACK="1")
        self.assertNotEqual(result.returncode, 0)
        writes = [command[-2] for command in self.commands() if command[1] == "write-bin"]
        self.assertEqual(writes, ["0x10000"])
        self.assertFalse(any(command[1] == "reset" for command in self.commands()))

    def ota_args(self, sequence=1):
        backup = self.root / "otadata.bin"
        backup.write_bytes(ota(sequence))
        return ("--backup", str(backup), "--backup-sha256",
                hashlib.sha256(backup.read_bytes()).hexdigest(), "--yes")

    def test_first_switch_checks_live_app1(self):
        result = self.run_script("switch-boot-app1.sh", *self.ota_args(), "--image", str(self.image))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[1] == "write-bin" for command in self.commands()))

    def test_first_switch_writes_only_second_ota_sector(self):
        with self.flash.open("r+b") as flash:
            flash.seek(0x650000)
            flash.write(self.image.read_bytes())
            flash.seek(0xE000)
            flash.write(ota(1))
        result = self.run_script("switch-boot-app1.sh", *self.ota_args(), "--image", str(self.image))
        self.assert_success(result)
        writes = [command[-2] for command in self.commands() if command[1] == "write-bin"]
        self.assertEqual(writes, ["0xF000"])
        with self.flash.open("rb") as flash:
            flash.seek(0xE000)
            data = flash.read(0x2000)
        self.assertEqual(SELECTED_ENTRY(data), (2, "app1"))
        self.assertEqual(data[:0x1000], ota(1)[:0x1000])

    def test_restore_otadata_rejects_wrong_slot(self):
        result = self.run_script("restore-otadata.sh", *self.ota_args(2), "--expect-slot", "app0")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[1] == "write-bin" for command in self.commands()))

    def test_restore_otadata_rejects_invalid_target_image(self):
        result = self.run_script("restore-otadata.sh", *self.ota_args(), "--expect-slot", "app0", INVALID_IMAGE="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(command[1] == "write-bin" for command in self.commands()))

    def test_restore_otadata_writes_only_metadata(self):
        result = self.run_script("restore-otadata.sh", *self.ota_args(), "--expect-slot", "app0")
        self.assert_success(result)
        writes = [command[-2] for command in self.commands() if command[1] == "write-bin"]
        self.assertEqual(writes, ["0xE000"])

    def test_restore_app0_writes_only_app0(self):
        result = self.run_script("restore-stock-app0.sh", *self.stock_args())
        self.assert_success(result)
        writes = [command[-2] for command in self.commands() if command[1] == "write-bin"]
        self.assertEqual(writes, ["0x10000"])

    def test_backup_does_not_overwrite_latest(self):
        directory = self.root / "backup/otadata"
        directory.mkdir(parents=True)
        latest = directory / "otadata-latest.bin"
        latest.write_bytes(b"preserve")
        with self.flash.open("r+b") as flash:
            flash.seek(0xE000)
            flash.write(ota(1))
        self.assert_success(self.run_script("backup-otadata.sh"))
        self.assert_success(self.run_script("backup-otadata.sh"))
        self.assertEqual(latest.read_bytes(), b"preserve")
        self.assertEqual(len(list(directory.glob("*.sha256"))), 2)


SELECTED_ENTRY = runpy.run_path(str(ROOT / "scripts/inspect-otadata.py"))["selected_entry"]


class OtadataTests(unittest.TestCase):
    def test_stock_and_development_sequences(self):
        self.assertEqual(SELECTED_ENTRY(ota(1)), (1, "app0"))
        self.assertEqual(SELECTED_ENTRY(ota(2)), (2, "app1"))

    def test_both_sectors_use_the_larger_sequence(self):
        data = ota(3)
        data[0x1000:] = ota(2)[:0x1000]
        self.assertEqual(SELECTED_ENTRY(data), (3, "app0"))

    def test_rejects_unconfirmed_and_invalid_states(self):
        for state in (0, 1, 3, 4, 5):
            with self.subTest(state=state), self.assertRaises(ValueError):
                SELECTED_ENTRY(ota(1, state))
        self.assertEqual(SELECTED_ENTRY(ota(1, 2)), (1, "app0"))

    def test_rejects_corruption_and_empty_selection(self):
        for data in (b"", b"\xff" * 0x2000, ota(0), ota(0xFFFFFFFF)):
            with self.subTest(size=len(data)), self.assertRaises(ValueError):
                SELECTED_ENTRY(data)
        data = ota(1)
        data[28] ^= 1
        with self.assertRaises(ValueError):
            SELECTED_ENTRY(data)


if __name__ == "__main__":
    unittest.main()

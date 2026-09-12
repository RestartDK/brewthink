import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
PROVED = b"proved reader ELF"
REPORT = '{"verdict":"PASS_LIMITED","whole_program_bound":false}\n'
FAKE_TOOL = f"#!{sys.executable}\n" + r'''
import json, os, sys
from pathlib import Path
name, args = Path(sys.argv[0]).name, sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as log:
    log.write(json.dumps([name, *args]) + "\n")
if name == "python3":
    assert Path(args[0]).name == "check-reader-memory.py"
    evidence = Path(args[1])
    if "--verify-elf" in args:
        assert Path(args[-1]) == evidence / "reader.elf"
        assert Path(args[-1]).read_bytes() == b"proved reader ELF"
        if os.environ.get("FAIL_VERIFY"):
            raise SystemExit("proof verification failed")
    else:
        if os.environ.get("FAIL_PRODUCER"):
            raise SystemExit("proof production failed")
        evidence.mkdir()
        if not os.environ.get("MISSING_ELF"):
            (evidence / "reader.elf").write_bytes(b"proved reader ELF")
        (evidence / "stack.json").write_text('{"verdict":"PASS_LIMITED","whole_program_bound":false}\n')
elif name == "cargo":
    if args[0] == "check":
        assert os.environ["BREWTHINK_DIAGNOSTIC_STAGE"] == "reader-app"
        assert os.environ["BREWTHINK_PREVIOUS_FRAME_STORAGE"] == "host-ram"
        raise SystemExit("reader-app requires controller-ram previous-frame storage")
    assert args[0] == "build"
    assert os.environ.get("BREWTHINK_DIAGNOSTIC_STAGE") != "reader-app"
    Path(os.environ["FAKE_TARGET_ELF"]).write_bytes(b"unproved diagnostic ELF")
elif name == "espflash":
    assert args[0] == "save-image", "hardware operations are forbidden in this test"
    if os.environ.get("FAIL_SAVE"):
        raise SystemExit("image generation failed")
    Path(args[-1]).write_bytes(Path(args[-2]).read_bytes())
elif name == "esptool":
    assert args[2] == "image-info", "hardware operations are forbidden in this test"
    print("ESP32-C3 Image Header\nFlash size: 16MB\nFlash freq: 80m\nFlash mode: DIO\nChip ID: 5 (ESP32-C3)\nChecksum: aa (valid)\nValidation hash: aa (valid)\nApplication Information\nProject name: brewthink")
else:
    raise SystemExit("unexpected tool")
'''


class ReaderImageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        shutil.copytree(ROOT / "scripts", self.root / "scripts")
        (self.root / "docs").mkdir()
        shutil.copyfile(ROOT / "docs/x4-stock-partition-table.csv", self.root / "docs/x4-stock-partition-table.csv")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for name in ("cargo", "python3", "espflash", "esptool"):
            executable = self.bin / name
            executable.write_text(FAKE_TOOL)
            executable.chmod(0o755)
        self.target = self.root / "target/riscv32imc-unknown-none-elf/release/brewthink"
        self.target.parent.mkdir(parents=True)
        self.target.write_bytes(b"unproved diagnostic ELF")
        self.image = self.root / "reader.bin"
        self.log = self.root / "commands.jsonl"
        self.environment = {**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}",
                            "FAKE_LOG": str(self.log), "FAKE_TARGET_ELF": str(self.target)}
        for name in tuple(self.environment):
            if name.startswith("BREWTHINK_"):
                self.environment.pop(name)

    def run_script(self, script="build-reader-app1.sh", *args, **environment):
        return subprocess.run(["bash", str(self.root / "scripts" / script), *args],
                              env={**self.environment, **environment}, capture_output=True,
                              text=True, timeout=20)

    def commands(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def test_builder_packages_the_proved_elf_and_verifies_before_copying_its_report(self):
        result = self.run_script("build-reader-app1.sh", str(self.image))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.image.read_bytes(), PROVED)
        self.assertEqual(Path(str(self.image) + ".reader-stack.json").read_text(), REPORT)
        commands = self.commands()
        self.assertEqual([command[0] for command in commands], ["python3", "espflash", "esptool", "python3"])
        evidence = Path(commands[0][2])
        self.assertEqual(Path(commands[1][-2]), evidence / "reader.elf")
        self.assertEqual(commands[-1][-2:], ["--verify-elf", str(evidence / "reader.elf")])
        self.assertNotEqual(self.target.read_bytes(), PROVED)

    def test_failed_production_or_missing_proved_elf_never_generates_an_image(self):
        for failure in ("FAIL_PRODUCER", "MISSING_ELF"):
            with self.subTest(failure=failure):
                self.log.unlink(missing_ok=True)
                result = self.run_script("build-reader-app1.sh", str(self.image), **{failure: "1"})
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.image.exists())
                self.assertFalse(any(command[0] == "espflash" for command in self.commands()))

    def test_failed_image_generation_never_verifies_or_publishes_a_report(self):
        result = self.run_script("build-reader-app1.sh", str(self.image), FAIL_SAVE="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any("--verify-elf" in command for command in self.commands()))
        self.assertFalse(Path(str(self.image) + ".reader-stack.json").exists())

    def test_failed_post_image_verification_never_publishes_a_report(self):
        result = self.run_script("build-reader-app1.sh", str(self.image), FAIL_VERIFY="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.image.exists())
        self.assertFalse(Path(str(self.image) + ".reader-stack.json").exists())

    def test_generic_reader_entry_rejects_changed_features_before_any_tool_runs(self):
        result = self.run_script("build-app1-image.sh", str(self.image),
                                 BREWTHINK_DIAGNOSTIC_STAGE="reader-app",
                                 BREWTHINK_CARGO_FEATURES="device-reader,epub")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.commands(), [])

    def test_ci_retains_the_reader_report_after_later_diagnostic_builds(self):
        result = self.run_script("check-firmware.sh")
        self.assertEqual(result.returncode, 0, result.stderr)
        report = self.root / "artifacts/ci/reader-stack.json"
        self.assertEqual(report.read_text(), REPORT)
        self.assertEqual((self.root / "artifacts/ci/reader-app-controller-ram.bin").read_bytes(), PROVED)
        self.assertNotEqual(self.target.read_bytes(), PROVED)
        self.assertEqual(len([command for command in self.commands() if command[0] == "python3"]), 2)

    def test_ci_stops_without_a_reader_report_if_proof_verification_fails(self):
        result = self.run_script("check-firmware.sh", FAIL_VERIFY="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "artifacts/ci/reader-stack.json").exists())


if __name__ == "__main__":
    unittest.main()

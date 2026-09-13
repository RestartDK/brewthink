import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent


class WorkspaceOwnershipTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        version = subprocess.check_output(["rustc", "-vV"], text=True)
        cls.host = next(line.removeprefix("host: ") for line in version.splitlines() if line.startswith("host: "))

    def compile(self, body, error=None):
        target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "workspace-contract-tests"
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as directory:
            source = Path(directory) / "contract.rs"
            source.write_text(
                f'#[path = "{ROOT / "src/scratch.rs"}"] mod scratch;\n'
                'unsafe fn zero<T>(pointer: *mut T) { unsafe { pointer.write_bytes(0, 1) } }\n'
                + body
            )
            result = subprocess.run([
                "rustc", "--edition=2024", "--crate-name=workspace_contract",
                "--target", self.host, "--emit=obj", "-o", str(Path(directory) / "contract.o"), str(source),
            ], text=True, capture_output=True, timeout=30)
            if error is None:
                self.assertEqual(result.returncode, 0, result.stderr)
            else:
                self.assertNotEqual(result.returncode, 0, "misuse unexpectedly compiled")
                self.assertIn(error, result.stderr)

    def test_valid_typed_then_byte_reuse_compiles(self):
        self.compile('fn main() { let mut s = scratch::Scratch::<16>::new(); let p = unsafe { s.initialize(zero::<[u8; 16]>) }; p[0] = 1; s.bytes()[0] = 2; }')

    def test_oversized_value_is_rejected_at_compile_time(self):
        self.compile('fn main() { let mut s = scratch::Scratch::<8>::new(); unsafe { s.initialize(zero::<[u8; 16]>); } }', 'E0080')

    def test_overaligned_value_is_rejected_at_compile_time(self):
        self.compile('#[repr(align(16))] struct Aligned(u8); fn main() { let mut s = scratch::Scratch::<64>::new(); unsafe { s.initialize(zero::<Aligned>); } }', 'E0080')

    def test_drop_value_cannot_be_silently_overwritten(self):
        self.compile('struct Owned; impl Drop for Owned { fn drop(&mut self) {} } fn main() { let mut s = scratch::Scratch::<16>::new(); unsafe { s.initialize(zero::<Owned>); } }', 'E0080')

    def test_typed_reference_cannot_survive_byte_reuse(self):
        self.compile('fn main() { let mut s = scratch::Scratch::<16>::new(); let p = unsafe { s.initialize(zero::<[u8; 16]>) }; s.bytes()[0] = 2; p[0] = 1; }', 'E0499')


if __name__ == "__main__":
    unittest.main()

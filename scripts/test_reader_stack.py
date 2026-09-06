import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("reader_stack", Path(__file__).with_name("check-reader-stack.py"))
stack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stack)


class ReaderStackTests(unittest.TestCase):
    def test_two_part_riscv_prologue(self):
        body = [
            "4204e38a: 7111 addi sp, sp, -0x100",
            "4204e38c: df86 sw ra, 0xfc(sp)",
            "4204e3a8: 6625 lui a2, 0x9",
            "4204e3aa: 19060613 addi a2, a2, 0x190",
            "4204e3ae: 40c10133 sub sp, sp, a2",
            "4204e3ee: 8502 jr a0",
        ]
        self.assertEqual(stack.stack_frame(body), 37520)

    def test_rejects_the_measured_startup_overflow(self):
        with self.assertRaisesRegex(ValueError, "budget exceeded"):
            stack.check({"task": 37536, "library": 41872, "effects": 0}, 62280)

    def test_accepts_separate_entry_frames(self):
        required = stack.check({"task": 1104, "library": 41872, "effects": 37504}, 62280)
        self.assertEqual(required, 51168)

    def test_unknown_stack_adjustments_fail_closed(self):
        with self.assertRaisesRegex(ValueError, "unknown stack adjustment"):
            stack.stack_frame(["4204e3ae: 40c10133 sub sp, sp, a2"])
        with self.assertRaisesRegex(ValueError, "no stack frame"):
            stack.stack_frame(["4204e3ae: 8082 ret"])


if __name__ == "__main__":
    unittest.main()

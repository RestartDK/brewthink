import os
from pathlib import Path
import pty
import select
import subprocess
import sys
import unittest

CLI = Path(__file__).with_name("grayscale-bench.py")


class GrayscaleBenchTests(unittest.TestCase):
    def exchange(self, reply, command="four"):
        master, slave = pty.openpty()
        child = subprocess.Popen(
            [sys.executable, str(CLI), "--port", os.ttyname(slave), "--timeout", "1", command],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        try:
            self.assertTrue(select.select([master], [], [], 5)[0])
            request = b""
            while not request.endswith(b"\n"):
                request += os.read(master, 128)
            self.assertEqual(request, f"BREWGRAY/1 {command}\n".encode())
            os.write(master, reply)
            stdout, stderr = child.communicate(timeout=5)
            return child.returncode, stdout, stderr
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
            os.close(master)
            os.close(slave)

    def test_requires_matching_terminal_response(self):
        code, stdout, _ = self.exchange(
            b"unrelated startup log\nBREWGRAY/1 DONE command=status status=ok\n"
            b"BREWGRAY/1 START command=four attempt=1\n"
            b"BREWGRAY/1 DONE command=four status=ok optical=unmeasured\n"
        )
        self.assertEqual(code, 0)
        self.assertIn(b"optical=unmeasured", stdout)

    def test_blocked_and_error_are_not_success(self):
        for status in ["blocked", "error"]:
            with self.subTest(status=status):
                code, _, _ = self.exchange(f"BREWGRAY/1 DONE command=four status={status}\n".encode())
                self.assertNotEqual(code, 0)

    def test_other_commands_do_not_complete_request(self):
        code, _, stderr = self.exchange(b"BREWGRAY/1 DONE command=status status=ok\n")
        self.assertNotEqual(code, 0)
        self.assertIn(b"no retry attempted", stderr)

    def test_transition_probe_has_an_explicit_name(self):
        code, stdout, _ = self.exchange(
            b"BREWGRAY/1 DONE command=sixteen-probe status=ok optical=unmeasured\n",
            command="sixteen-probe",
        )
        self.assertEqual(code, 0)
        self.assertIn(b"optical=unmeasured", stdout)

    def test_eight_repeat_has_an_explicit_name(self):
        code, stdout, _ = self.exchange(
            b"BREWGRAY/1 DONE command=eight-repeat status=ok optical=unmeasured\n",
            command="eight-repeat",
        )
        self.assertEqual(code, 0)
        self.assertIn(b"optical=unmeasured", stdout)

    def test_unknown_depth_rejected_before_opening_device(self):
        result = subprocess.run(
            [sys.executable, str(CLI), "--port", "/not/a/device", "eight"],
            capture_output=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"invalid choice", result.stderr)


if __name__ == "__main__":
    unittest.main()

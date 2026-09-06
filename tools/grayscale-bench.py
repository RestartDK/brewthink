#!/usr/bin/env python3
"""Trigger one diagnostic pattern. No flash writes or automatic retries."""

import argparse
import os
import select
import termios
import time
import tty


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", required=True)
    parser.add_argument("--timeout", type=float, default=90)
    parser.add_argument("command", choices=["status", "white", "black", "binary", "four", "sixteen-probe", "eight-repeat"])
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("timeout must be positive")
    fd = os.open(args.port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    original = termios.tcgetattr(fd)
    try:
        tty.setraw(fd, termios.TCSANOW)
        termios.tcflush(fd, termios.TCIFLUSH)
        pending = f"BREWGRAY/1 {args.command}\n".encode()
        deadline = time.monotonic() + args.timeout
        while pending:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([], [fd], [], remaining)[1]:
                raise TimeoutError("write timed out; do not retry without inspecting the device")
            try:
                count = os.write(fd, pending)
            except BlockingIOError:
                continue
            if not count:
                raise ConnectionError("reader disconnected")
            pending = pending[count:]
        buffer = b""
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([fd], [], [], remaining)[0]:
                raise TimeoutError("response timed out; physical result is unknown; no retry attempted")
            try:
                chunk = os.read(fd, 4096)
            except BlockingIOError:
                continue
            if not chunk:
                raise ConnectionError("reader disconnected")
            buffer += chunk
            while b"\n" in buffer:
                raw, buffer = buffer.split(b"\n", 1)
                marker = raw.find(b"BREWGRAY/1 ")
                if marker < 0:
                    continue
                line = raw[marker:].decode("ascii", errors="replace").strip()
                print(line, flush=True)
                if line.startswith(f"BREWGRAY/1 DONE command={args.command} "):
                    if "status=ok" not in line.split():
                        raise RuntimeError(line)
                    return
                if line.startswith("BREWGRAY/1 DONE command=parse "):
                    raise RuntimeError(line)
            if len(buffer) > 4096:
                raise RuntimeError("unbounded response line")
    finally:
        try:
            termios.tcsetattr(fd, termios.TCSANOW, original)
        finally:
            os.close(fd)


if __name__ == "__main__":
    main()

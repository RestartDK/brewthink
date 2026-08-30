#!/usr/bin/env python3

import argparse
import glob
import os
import select
import sys
import time


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", default=os.environ.get("ESPFLASH_PORT"))
    return parser.parse_args()


def find_port(requested: str | None) -> str | None:
    if requested and os.path.exists(requested):
        return requested
    ports = sorted(glob.glob("/dev/cu.usbmodem*"))
    if requested:
        return ports[0] if len(ports) == 1 else None
    return ports[0] if ports else None


def open_read_only_monitor(port: str) -> int:
    return os.open(port, os.O_RDONLY | os.O_NONBLOCK | os.O_NOCTTY)


def emit_bench_lines(buffer: bytearray) -> None:
    while True:
        newline = buffer.find(b"\n")
        if newline < 0:
            return
        line = bytes(buffer[:newline]).rstrip(b"\r")
        del buffer[: newline + 1]
        marker = line.rfind(b"bench:")
        if marker >= 0:
            print(line[marker:].decode("ascii", errors="replace"), flush=True)


def monitor(requested_port: str | None) -> None:
    descriptor: int | None = None
    active_port: str | None = None
    buffer = bytearray()

    while True:
        if descriptor is None:
            port = find_port(requested_port)
            if port is None:
                time.sleep(0.25)
                continue
            try:
                descriptor = open_read_only_monitor(port)
                active_port = port
                buffer.clear()
                print(f"host: connected port={port}", flush=True)
            except OSError:
                descriptor = None
                time.sleep(0.25)
                continue

        try:
            readable, _, _ = select.select([descriptor], [], [], 0.25)
            if not readable:
                if active_port is not None and not os.path.exists(active_port):
                    raise OSError("serial device disappeared")
                continue
            chunk = os.read(descriptor, 512)
            if chunk:
                buffer.extend(chunk)
                emit_bench_lines(buffer)
            elif active_port is not None and not os.path.exists(active_port):
                raise OSError("serial device disappeared")
        except OSError:
            os.close(descriptor)
            descriptor = None
            buffer.clear()
            print(f"host: disconnected port={active_port}", flush=True)
            active_port = None


def main() -> int:
    args = parse_args()
    try:
        monitor(args.port)
    except KeyboardInterrupt:
        print("host: stopped", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())

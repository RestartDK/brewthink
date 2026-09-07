#!/usr/bin/env python3

import argparse
from pathlib import Path
import struct
import zlib


def selected_entry(data: bytes) -> tuple[int, str]:
    if len(data) != 0x2000:
        raise ValueError("otadata must contain exactly two 4096-byte sectors")
    entries = []
    for offset in (0, 0x1000):
        sector = data[offset:offset + 0x1000]
        if sector == b"\xff" * 0x1000:
            continue
        sequence, = struct.unpack_from("<I", sector)
        state, checksum = struct.unpack_from("<II", sector, 24)
        if sequence in (0, 0xFFFFFFFF):
            raise ValueError("invalid OTA sequence")
        if checksum != zlib.crc32(sector[:4], 0xFFFFFFFF) & 0xFFFFFFFF:
            raise ValueError("invalid OTA checksum")
        if state not in (2, 0xFFFFFFFF):
            raise ValueError("refusing unconfirmed, invalid, or aborted OTA state")
        entries.append(sequence)
    if not entries:
        raise ValueError("otadata has no confirmed boot selection")
    if len(entries) == 2 and entries[0] == entries[1]:
        raise ValueError("ambiguous duplicate OTA sequence")
    sequence = max(entries)
    return sequence, "app0" if sequence % 2 else "app1"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("path", type=Path)
    parser.add_argument("--expect-slot", choices=("app0", "app1"))
    parser.add_argument("--expect-sequence", type=int)
    args = parser.parse_args()
    try:
        sequence, slot = selected_entry(args.path.read_bytes())
        if args.expect_slot is not None and slot != args.expect_slot:
            raise ValueError(f"expected {args.expect_slot}, found {slot}")
        if args.expect_sequence is not None and sequence != args.expect_sequence:
            raise ValueError(f"expected sequence {args.expect_sequence}, found {sequence}")
    except (OSError, ValueError) as error:
        parser.exit(1, f"error: {error}\n")
    print(f"sequence={sequence} slot={slot}")


if __name__ == "__main__":
    main()

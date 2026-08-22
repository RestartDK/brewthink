#!/usr/bin/env bash
# Read-only backup of the X4 otadata partition.
# This writes only a local file under ignored backup/; it does not write device flash.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

require_cmd espflash
require_cmd python3
check_partition_table_constants

PORT_ARGS=()
if [[ -n "${ESPFLASH_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPFLASH_PORT")
elif [[ -n "${ESPTOOL_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPTOOL_PORT")
else
  echo "error: refusing to read otadata without an explicit port" >&2
  echo "set ESPFLASH_PORT=/dev/cu.usbmodemXXXX and retry" >&2
  exit 1
fi

STAMP="$(date +%Y%m%d-%H%M%S)"
OUT_DIR="$ROOT_DIR/backup/otadata"
OUT="$OUT_DIR/otadata-$STAMP.bin"
LATEST="$OUT_DIR/otadata-latest.bin"
INFO="$OUT.txt"

mkdir -p "$OUT_DIR"

cat <<EOF
Backing up otadata with a read-only flash command.
Port:        ${ESPFLASH_PORT:-${ESPTOOL_PORT:-}}
Offset:      $OTADATA_OFFSET_HEX
Size:        $OTADATA_SIZE_HEX ($OTADATA_SIZE bytes)
Output:      $OUT

This does not write device flash.
EOF

espflash read-flash \
  --chip "$CHIP" \
  "${PORT_ARGS[@]}" \
  "$OTADATA_OFFSET_HEX" \
  "$OTADATA_SIZE_HEX" \
  "$OUT"

SIZE="$(file_size "$OUT")"
if (( SIZE != OTADATA_SIZE )); then
  echo "error: otadata backup has unexpected size: $SIZE bytes" >&2
  exit 1
fi

SHA="$(sha256_file "$OUT")"
cp "$OUT" "$LATEST"

python3 - "$OUT" > "$INFO" <<'PY'
from pathlib import Path
import struct, sys, zlib

path = Path(sys.argv[1])
data = path.read_bytes()
print(f"file: {path}")
print(f"size: {len(data)} bytes")
print()
print("otadata entries:")
for sector in (0, 1):
    off = sector * 0x1000
    entry = data[off:off + 32]
    seq = int.from_bytes(entry[0:4], "little")
    crc = int.from_bytes(entry[28:32], "little")
    erased = all(b == 0xFF for b in entry)
    calc = zlib.crc32(entry[0:4], 0xFFFFFFFF) & 0xFFFFFFFF
    valid_crc = crc == calc
    valid = (not erased) and seq != 0 and valid_crc
    if valid:
        slot = "app0" if seq % 2 == 1 else "app1"
        meaning = f"valid seq={seq} -> {slot}"
    elif erased or seq == 0:
        meaning = "empty/unselected"
    else:
        meaning = f"not a valid select entry (seq={seq}, crc=0x{crc:08X}, expected=0x{calc:08X})"
    print(f"  sector {sector} @ +0x{off:04X}: {meaning}")
PY

cat >> "$INFO" <<EOF
sha256: $SHA
EOF

cat <<EOF

OK: otadata backup complete
backup:  $OUT
latest:  $LATEST
sha256:  $SHA
summary: $INFO

Decode summary:
EOF
sed 's/^/  /' "$INFO"

cat <<EOF

Restore command, if needed later:
  ESPFLASH_PORT=${ESPFLASH_PORT:-${ESPTOOL_PORT:-/dev/cu.usbmodemXXXX}} espflash write-bin --chip $CHIP --port ${ESPFLASH_PORT:-${ESPTOOL_PORT:-/dev/cu.usbmodemXXXX}} $OTADATA_OFFSET_HEX "$OUT"
EOF

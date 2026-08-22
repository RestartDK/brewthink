#!/usr/bin/env bash
# Erase the app1 partition only, then verify it reads back as 0xFF.
# WARNING: this is an erase operation. Do not run while app1 is the only bootable slot.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

YES=0

while (($#)); do
  case "$1" in
    --yes|-y)
      YES=1
      shift
      ;;
    --help|-h)
      cat <<EOF
Usage: $0 [--yes]

Erases app1 only:
  address: $APP1_OFFSET_HEX
  size:    $APP1_SIZE_HEX ($APP1_SIZE bytes)

This is destructive to the Brewthink image in app1.
Restore/select stock app0 before running this.
EOF
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

require_cmd espflash
require_cmd python3
check_partition_table_constants

PORT_ARGS=()
if [[ -n "${ESPFLASH_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPFLASH_PORT")
elif [[ -n "${ESPTOOL_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPTOOL_PORT")
else
  echo "error: refusing to erase app1 without an explicit port" >&2
  echo "set ESPFLASH_PORT=/dev/cu.usbmodemXXXX and retry" >&2
  exit 1
fi

READBACK="$(mktemp)"
trap 'rm -f "$READBACK"' EXIT

cat <<EOF

APP1 ERASE REVIEW
=================
Will erase range: $APP1_OFFSET_HEX..$(fmt_hex $((APP1_OFFSET + APP1_SIZE - 1)))
Erase size:       $APP1_SIZE_HEX ($APP1_SIZE bytes)

This script erases only app1.
It does NOT write app0, otadata, bootloader, partition table, NVS, or filesystem.

WARNING: If otadata selects app1 after this, the device will not boot Brewthink.
Restore otadata/app0 first if you are trying to return to stock behavior.
EOF

if (( YES == 0 )); then
  echo
  read -r -p "Type exactly 'erase app1' to continue: " CONFIRM
  if [[ "$CONFIRM" != "erase app1" ]]; then
    echo "aborted: confirmation did not match"
    exit 1
  fi
fi

printf '\n== Erasing app1 only ==\n'
espflash erase-region --chip "$CHIP" "${PORT_ARGS[@]}" "$APP1_OFFSET_HEX" "$APP1_SIZE_HEX"

printf '\n== Reading back app1 to verify erased state ==\n'
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" "$APP1_OFFSET_HEX" "$APP1_SIZE_HEX" "$READBACK"

python3 - "$READBACK" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
data = path.read_bytes()
for i, b in enumerate(data):
    if b != 0xFF:
        raise SystemExit(f"app1 not fully erased: first non-0xFF at offset 0x{i:X}, byte=0x{b:02X}")
PY

cat <<EOF

OK: app1 erase verified; all readback bytes are 0xFF
EOF

#!/usr/bin/env bash
# Restore the stock app0 partition from the private full-flash backup.
# This writes only app0 at 0x10000, then readback-verifies it.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

BACKUP="$STOCK_FLASH_BACKUP"
YES=0

usage() {
  cat <<EOF
Usage: $0 [--stock-flash-backup PATH] [--yes]

Restores stock app0 only:
  source slice: offset $APP0_OFFSET_HEX, size $APP0_SIZE_HEX from the private full-flash backup
  target:       $APP0_OFFSET_HEX on the device

Default full-flash backup:
  $BACKUP
EOF
}

while (($#)); do
  case "$1" in
    --stock-flash-backup)
      BACKUP="${2:?missing value for --stock-flash-backup}"
      shift 2
      ;;
    --yes|-y)
      YES=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

require_cmd espflash
require_cmd python3
require_cmd cmp
check_partition_table_constants

PORT_ARGS=()
if [[ -n "${ESPFLASH_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPFLASH_PORT")
elif [[ -n "${ESPTOOL_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPTOOL_PORT")
else
  echo "error: refusing to write app0 without an explicit port" >&2
  echo "set ESPFLASH_PORT=/dev/cu.usbmodemXXXX and retry" >&2
  exit 1
fi

if [[ ! -f "$BACKUP" ]]; then
  echo "error: private full-flash backup not found: $BACKUP" >&2
  exit 1
fi

BACKUP_SIZE="$(file_size "$BACKUP")"
if (( BACKUP_SIZE != FULL_FLASH_SIZE )); then
  echo "error: full-flash backup has unexpected size: $BACKUP_SIZE bytes, expected $FULL_FLASH_SIZE" >&2
  exit 1
fi

OUT_DIR="$ROOT_DIR/artifacts/restore"
APP0_BIN="$OUT_DIR/stock-app0-from-backup.bin"
READBACK="$OUT_DIR/stock-app0-readback.bin"
mkdir -p "$OUT_DIR"

python3 - "$BACKUP" "$APP0_BIN" "$APP0_OFFSET" "$APP0_SIZE" <<'PY'
from pathlib import Path
import sys
backup = Path(sys.argv[1])
out = Path(sys.argv[2])
offset = int(sys.argv[3])
size = int(sys.argv[4])
with backup.open("rb") as f:
    f.seek(offset)
    data = f.read(size)
if len(data) != size:
    raise SystemExit(f"short read from backup: got {len(data)}, expected {size}")
out.write_bytes(data)
PY

cat <<EOF

STOCK APP0 RESTORE REVIEW
=========================
Will write:     $APP0_BIN
Source backup:  $BACKUP
SHA-256:        $(sha256_file "$APP0_BIN")
Write address:  $APP0_OFFSET_HEX
Write size:     $APP0_SIZE_HEX ($APP0_SIZE bytes)
Write range:    $(fmt_hex "$APP0_OFFSET")..$(fmt_hex $((APP0_OFFSET + APP0_SIZE - 1)))

This script writes only stock app0.
It does NOT write app1, otadata, bootloader, partition table, NVS, or filesystem.
EOF

if (( YES == 0 )); then
  echo
  read -r -p "Type exactly 'restore stock app0' to continue: " CONFIRM
  if [[ "$CONFIRM" != "restore stock app0" ]]; then
    echo "aborted: confirmation did not match"
    exit 1
  fi
fi

printf '\n== Restoring stock app0 ==\n'
espflash write-bin --chip "$CHIP" "${PORT_ARGS[@]}" "$APP0_OFFSET_HEX" "$APP0_BIN"

printf '\n== Reading back app0 for verification ==\n'
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" "$APP0_OFFSET_HEX" "$APP0_SIZE_HEX" "$READBACK"

if ! cmp -s "$APP0_BIN" "$READBACK"; then
  echo "error: app0 readback differs from stock backup slice" >&2
  echo "expected sha256: $(sha256_file "$APP0_BIN")" >&2
  echo "readback sha256: $(sha256_file "$READBACK")" >&2
  exit 1
fi

cat <<EOF

OK: stock app0 restore/readback verified
app0 sha256: $(sha256_file "$APP0_BIN")
EOF

#!/usr/bin/env bash
# Return this X4 to the verified stock boot state without writing the whole flash.
# Sequence is intentionally no-reset until the end:
#   1. restore stock app0 from private full-flash backup
#   2. erase app1
#   3. restore original otadata backup selecting stock app0
#   4. reset once at the end
# This does NOT write bootloader, partition table, NVS, filesystem, or coredump.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

STOCK_BACKUP="$STOCK_FLASH_BACKUP"
OTADATA_BACKUP="$ROOT_DIR/backup/otadata/otadata-latest.bin"
YES=0

usage() {
  cat <<EOF
Usage: $0 [--stock-flash-backup PATH] [--otadata-backup PATH] [--yes]

Restores stock-like boot state by writing only:
  app0:     $APP0_OFFSET_HEX..$(fmt_hex $((APP0_OFFSET + APP0_SIZE - 1))) from private stock backup
  app1:     erase $APP1_OFFSET_HEX..$(fmt_hex $((APP1_OFFSET + APP1_SIZE - 1)))
  otadata:  $OTADATA_OFFSET_HEX..$(fmt_hex $((OTADATA_OFFSET + OTADATA_SIZE - 1))) from backup

It does NOT write bootloader, partition table, NVS, filesystem, or coredump.
EOF
}

while (($#)); do
  case "$1" in
    --stock-flash-backup)
      STOCK_BACKUP="${2:?missing value for --stock-flash-backup}"
      shift 2
      ;;
    --otadata-backup)
      OTADATA_BACKUP="${2:?missing value for --otadata-backup}"
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
  echo "error: refusing stock-state restore without an explicit port" >&2
  echo "set ESPFLASH_PORT=/dev/cu.usbmodemXXXX and retry" >&2
  exit 1
fi

if [[ ! -f "$STOCK_BACKUP" ]]; then
  echo "error: private stock full-flash backup not found: $STOCK_BACKUP" >&2
  exit 1
fi
if [[ ! -f "$OTADATA_BACKUP" ]]; then
  echo "error: otadata backup not found: $OTADATA_BACKUP" >&2
  exit 1
fi
if (( $(file_size "$STOCK_BACKUP") != FULL_FLASH_SIZE )); then
  echo "error: stock full-flash backup has unexpected size" >&2
  exit 1
fi
if (( $(file_size "$OTADATA_BACKUP") != OTADATA_SIZE )); then
  echo "error: otadata backup has unexpected size" >&2
  exit 1
fi

OUT_DIR="$ROOT_DIR/artifacts/restore-stock-state"
APP0_BIN="$OUT_DIR/stock-app0-from-backup.bin"
APP0_READBACK="$OUT_DIR/stock-app0-readback.bin"
APP1_READBACK="$OUT_DIR/app1-erased-readback.bin"
OTADATA_READBACK="$OUT_DIR/otadata-readback.bin"
mkdir -p "$OUT_DIR"

python3 - "$STOCK_BACKUP" "$APP0_BIN" "$APP0_OFFSET" "$APP0_SIZE" <<'PY'
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
    raise SystemExit(f"short app0 read from backup: {len(data)} != {size}")
out.write_bytes(data)
PY

cat <<EOF

STOCK STATE RESTORE REVIEW
==========================
Will restore stock app0 from:
  $STOCK_BACKUP
Extracted app0 image:
  $APP0_BIN
App0 SHA-256:
  $(sha256_file "$APP0_BIN")

Will erase app1:
  $APP1_OFFSET_HEX..$(fmt_hex $((APP1_OFFSET + APP1_SIZE - 1))) ($APP1_SIZE bytes)

Will restore otadata from:
  $OTADATA_BACKUP
Otadata SHA-256:
  $(sha256_file "$OTADATA_BACKUP")

This script does NOT write bootloader, partition table, NVS, filesystem, or coredump.
It keeps the chip in bootloader/no-reset mode between steps and resets once at the end.
EOF

if (( YES == 0 )); then
  echo
  read -r -p "Type exactly 'restore stock state' to continue: " CONFIRM
  if [[ "$CONFIRM" != "restore stock state" ]]; then
    echo "aborted: confirmation did not match"
    exit 1
  fi
fi

printf '\n== 1/6 Restoring stock app0, no reset ==\n'
espflash write-bin --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$APP0_OFFSET_HEX" "$APP0_BIN"

printf '\n== 2/6 Reading back app0 ==\n'
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$APP0_OFFSET_HEX" "$APP0_SIZE_HEX" "$APP0_READBACK"
cmp -s "$APP0_BIN" "$APP0_READBACK"

printf '\n== 3/6 Erasing app1, no reset ==\n'
espflash erase-region --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$APP1_OFFSET_HEX" "$APP1_SIZE_HEX"

printf '\n== 4/6 Reading back app1 erased state ==\n'
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$APP1_OFFSET_HEX" "$APP1_SIZE_HEX" "$APP1_READBACK"
python3 - "$APP1_READBACK" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
for i, b in enumerate(path.read_bytes()):
    if b != 0xFF:
        raise SystemExit(f"app1 not fully erased: first non-0xFF at offset 0x{i:X}, byte=0x{b:02X}")
PY

printf '\n== 5/6 Restoring otadata, no reset ==\n'
espflash write-bin --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$OTADATA_OFFSET_HEX" "$OTADATA_BACKUP"

printf '\n== 6/6 Reading back otadata ==\n'
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$OTADATA_OFFSET_HEX" "$OTADATA_SIZE_HEX" "$OTADATA_READBACK"
cmp -s "$OTADATA_BACKUP" "$OTADATA_READBACK"

printf '\n== Final reset ==\n'
espflash reset --chip "$CHIP" "${PORT_ARGS[@]}" || true

cat <<EOF

OK: stock state restore verified
app0 restored sha256:    $(sha256_file "$APP0_BIN")
app1 erased:             verified all 0xFF
otadata restored sha256: $(sha256_file "$OTADATA_BACKUP")
EOF

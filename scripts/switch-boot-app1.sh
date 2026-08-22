#!/usr/bin/env bash
# Guarded first boot switch to Brewthink in app1.
# Writes only otadata sector 1 at 0xF000 with ota_seq=2, then readback-verifies it.
# This changes the boot target to app1. It does not write app0 or the app image.

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

Requires:
  ESPFLASH_PORT=/dev/cu.usbmodemXXXX
  artifacts/brewthink-app1.bin already built/checked/written to app1
  backup/otadata/otadata-latest.bin already created

Writes only otadata sector 1:
  address: 0xF000
  size:    0x1000
  effect:  select app1 on next boot
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
require_cmd cmp
check_partition_table_constants

PORT_ARGS=()
if [[ -n "${ESPFLASH_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPFLASH_PORT")
elif [[ -n "${ESPTOOL_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPTOOL_PORT")
else
  echo "error: refusing to write otadata without an explicit port" >&2
  echo "set ESPFLASH_PORT=/dev/cu.usbmodemXXXX and retry" >&2
  exit 1
fi

OTADATA_BACKUP="$ROOT_DIR/backup/otadata/otadata-latest.bin"
if [[ ! -f "$OTADATA_BACKUP" ]]; then
  echo "error: missing otadata backup: $OTADATA_BACKUP" >&2
  echo "run scripts/backup-otadata.sh first" >&2
  exit 1
fi

"$ROOT_DIR/scripts/check-app1-image.sh" "$DEFAULT_IMAGE" "${DEFAULT_IMAGE}.image-info.txt"

OUT_DIR="$ROOT_DIR/artifacts/otadata"
SECTOR_BIN="$OUT_DIR/select-app1-sector.bin"
READBACK="$OUT_DIR/select-app1-sector.readback.bin"
mkdir -p "$OUT_DIR"

python3 - "$SECTOR_BIN" <<'PY'
from pathlib import Path
import sys, zlib
out = Path(sys.argv[1])
sector = bytearray([0xFF]) * 0x1000
seq = (2).to_bytes(4, "little")
crc = zlib.crc32(seq, 0xFFFFFFFF) & 0xFFFFFFFF
sector[0:4] = seq
sector[28:32] = crc.to_bytes(4, "little")
out.write_bytes(sector)
print(f"wrote {out}")
print(f"seq=2 crc=0x{crc:08X}")
PY

EXPECTED_SHA="$(sha256_file "$SECTOR_BIN")"

cat <<EOF

OTADATA WRITE REVIEW
====================
Will write:       $SECTOR_BIN
SHA-256:          $EXPECTED_SHA
Write address:    0xF000
Write size:       0x1000 (4096 bytes)
Write range:      0x00F000..0x00FFFF
Effect:           bootloader should select app1 on next reset

This script will NOT write:
  bootloader, partition table, nvs, app0, app1 image bytes, spiffs/LittleFS, or coredump.

Restore original boot selection with:
  ESPFLASH_PORT=${ESPFLASH_PORT:-${ESPTOOL_PORT:-/dev/cu.usbmodemXXXX}} espflash write-bin --chip $CHIP --port ${ESPFLASH_PORT:-${ESPTOOL_PORT:-/dev/cu.usbmodemXXXX}} $OTADATA_OFFSET_HEX "$OTADATA_BACKUP"
EOF

if (( YES == 0 )); then
  echo
  read -r -p "Type exactly 'select app1' to continue: " CONFIRM
  if [[ "$CONFIRM" != "select app1" ]]; then
    echo "aborted: confirmation did not match"
    exit 1
  fi
fi

printf '\n== Writing otadata sector 1 to select app1 ==\n'
espflash write-bin --chip "$CHIP" "${PORT_ARGS[@]}" 0xF000 "$SECTOR_BIN"

printf '\n== Reading back otadata sector 1 ==\n'
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" 0xF000 0x1000 "$READBACK"

READBACK_SHA="$(sha256_file "$READBACK")"
if ! cmp -s "$SECTOR_BIN" "$READBACK"; then
  echo "error: otadata sector readback differs" >&2
  echo "expected sha256: $EXPECTED_SHA" >&2
  echo "readback sha256: $READBACK_SHA" >&2
  exit 1
fi

cat <<EOF

OK: otadata sector 1 write/readback verified
sector sha256: $EXPECTED_SHA
readback sha:  $READBACK_SHA
boot target:   app1 should be selected now

Next monitor command:
  ESPFLASH_PORT=${ESPFLASH_PORT:-${ESPTOOL_PORT:-/dev/cu.usbmodemXXXX}} espflash monitor --chip $CHIP --port ${ESPFLASH_PORT:-${ESPTOOL_PORT:-/dev/cu.usbmodemXXXX}} --log-format defmt --elf "$ELF"
EOF

#!/usr/bin/env bash
# Restore a previously backed-up otadata partition to 0xE000.
# This changes boot selection metadata only. It does not write app0/app1 image bytes.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

BACKUP="$ROOT_DIR/backup/otadata/otadata-latest.bin"
YES=0

usage() {
  cat <<EOF
Usage: $0 [--backup PATH] [--yes]

Restores otadata only:
  address: $OTADATA_OFFSET_HEX
  size:    $OTADATA_SIZE_HEX ($OTADATA_SIZE bytes)

Default backup:
  $BACKUP
EOF
}

while (($#)); do
  case "$1" in
    --backup)
      BACKUP="${2:?missing value for --backup}"
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

if [[ ! -f "$BACKUP" ]]; then
  echo "error: backup file not found: $BACKUP" >&2
  exit 1
fi

SIZE="$(file_size "$BACKUP")"
if (( SIZE != OTADATA_SIZE )); then
  echo "error: otadata backup has unexpected size: $SIZE bytes, expected $OTADATA_SIZE" >&2
  exit 1
fi

READBACK="$(mktemp)"
trap 'rm -f "$READBACK"' EXIT

cat <<EOF

OTADATA RESTORE REVIEW
======================
Will write:     $BACKUP
SHA-256:        $(sha256_file "$BACKUP")
Write address:  $OTADATA_OFFSET_HEX
Write size:     $OTADATA_SIZE_HEX ($OTADATA_SIZE bytes)
Write range:    $(fmt_hex "$OTADATA_OFFSET")..$(fmt_hex $((OTADATA_OFFSET + OTADATA_SIZE - 1)))

This script writes only otadata boot-selection metadata.
It does NOT write app0, app1, bootloader, partition table, NVS, or filesystem.
EOF

if (( YES == 0 )); then
  echo
  read -r -p "Type exactly 'restore otadata' to continue: " CONFIRM
  if [[ "$CONFIRM" != "restore otadata" ]]; then
    echo "aborted: confirmation did not match"
    exit 1
  fi
fi

printf '\n== Restoring otadata ==\n'
espflash write-bin --chip "$CHIP" "${PORT_ARGS[@]}" "$OTADATA_OFFSET_HEX" "$BACKUP"

printf '\n== Reading back otadata ==\n'
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" "$OTADATA_OFFSET_HEX" "$OTADATA_SIZE_HEX" "$READBACK"

if ! cmp -s "$BACKUP" "$READBACK"; then
  echo "error: otadata readback differs from backup" >&2
  echo "backup sha256:   $(sha256_file "$BACKUP")" >&2
  echo "readback sha256: $(sha256_file "$READBACK")" >&2
  exit 1
fi

cat <<EOF

OK: otadata restore/readback verified
restored: $BACKUP
sha256:  $(sha256_file "$BACKUP")
EOF

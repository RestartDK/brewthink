#!/usr/bin/env bash

set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

BACKUP="$STOCK_FLASH_BACKUP"
BACKUP_SHA=""
YES=0
while (($#)); do
  case "$1" in
    --stock-flash-backup) BACKUP="${2:?missing backup}"; shift 2 ;;
    --backup-sha256) BACKUP_SHA="${2:?missing digest}"; shift 2 ;;
    --yes|-y) YES=1; shift ;;
    --help|-h)
      echo "Usage: $0 [--stock-flash-backup PATH] --backup-sha256 SHA256 [--yes]"
      echo 'Restores only app0 from a reviewed full stock backup. Boot selection remains unchanged.'
      exit 0 ;;
    *) echo "error: unknown argument: $1" >&2; exit 1 ;;
  esac
done

private_workspace
prepare_stock_backup "$BACKUP" "$BACKUP_SHA"
probe_x4
cat <<EOF
STOCK APP0 WRITE REVIEW
Source: $BACKUP
Full backup SHA-256: $BACKUP_SHA
Read-only app0 snapshot: $WORK_DIR/app0.bin
App0 SHA-256: $(sha256_file "$WORK_DIR/app0.bin")
Byte and sector range: $APP0_OFFSET_HEX..$(fmt_hex $((APP0_OFFSET + APP0_SIZE - 1))) ($APP0_SIZE bytes)
All other partitions, including app1 and otadata, remain unchanged.
EOF
confirm_write 'restore stock app0'
write_and_verify "$APP0_OFFSET_HEX" "$APP0_SIZE_HEX" "$WORK_DIR/app0.bin"
echo 'OK: stock app0 write/readback verified'
espflash reset --chip "$CHIP" "${PORT_ARGS[@]}"

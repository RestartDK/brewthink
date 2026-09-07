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
      echo 'Restores app0 and its original otadata from the reviewed full stock backup. Leaves app1 intact.'
      exit 0 ;;
    *) echo "error: unknown argument: $1" >&2; exit 1 ;;
  esac
done

private_workspace
prepare_stock_backup "$BACKUP" "$BACKUP_SHA"
probe_x4
cat <<EOF
STOCK RESTORE REVIEW
Source: $BACKUP
Full backup SHA-256: $BACKUP_SHA
App0 range: $APP0_OFFSET_HEX..$(fmt_hex $((APP0_OFFSET + APP0_SIZE - 1))) ($APP0_SIZE bytes)
App0 SHA-256: $(sha256_file "$WORK_DIR/app0.bin")
Otadata range: $OTADATA_OFFSET_HEX..$(fmt_hex $((OTADATA_OFFSET + OTADATA_SIZE - 1))) ($OTADATA_SIZE bytes)
Otadata SHA-256: $(sha256_file "$WORK_DIR/otadata.bin")
Both payloads are read-only snapshots from the same verified backup. Otadata selects app0 at sequence 1.
App0 readback must pass before otadata is written. Reset follows both readbacks.
No app1 erase occurs. All other partitions remain unchanged.
EOF
confirm_write 'restore stock state'
write_and_verify "$APP0_OFFSET_HEX" "$APP0_SIZE_HEX" "$WORK_DIR/app0.bin"
write_and_verify "$OTADATA_OFFSET_HEX" "$OTADATA_SIZE_HEX" "$WORK_DIR/otadata.bin"
echo 'OK: stock app0 and boot selection verified; app1 preserved'
espflash reset --chip "$CHIP" "${PORT_ARGS[@]}"

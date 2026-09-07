#!/usr/bin/env bash

set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

BACKUP=""
BACKUP_SHA=""
SLOT=""
YES=0
while (($#)); do
  case "$1" in
    --backup) BACKUP="${2:?missing backup}"; shift 2 ;;
    --backup-sha256) BACKUP_SHA="${2:?missing digest}"; shift 2 ;;
    --expect-slot) SLOT="${2:?missing slot}"; shift 2 ;;
    --yes|-y) YES=1; shift ;;
    --help|-h)
      echo "Usage: $0 --backup PATH --backup-sha256 SHA256 --expect-slot app0|app1 [--yes]"
      echo 'Validates the selected slot image, then restores and verifies only otadata.'
      exit 0 ;;
    *) echo "error: unknown argument: $1" >&2; exit 1 ;;
  esac
done
case "$SLOT" in
  app0) SLOT_OFFSET="$APP0_OFFSET_HEX"; SLOT_SIZE="$APP0_SIZE_HEX" ;;
  app1) SLOT_OFFSET="$APP1_OFFSET_HEX"; SLOT_SIZE="$APP1_SIZE_HEX" ;;
  *) echo 'error: --expect-slot app0 or app1 is required' >&2; exit 1 ;;
esac
private_workspace
snapshot_file "$BACKUP" "$WORK_DIR/otadata.bin"
verify_backup "$WORK_DIR/otadata.bin" "$BACKUP_SHA" "$OTADATA_SIZE"
python3 "$ROOT_DIR/scripts/inspect-otadata.py" "$WORK_DIR/otadata.bin" --expect-slot "$SLOT"
probe_x4
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$SLOT_OFFSET" "$SLOT_SIZE" "$WORK_DIR/selected-app.bin"
verify_app_image "$WORK_DIR/selected-app.bin"
cat <<EOF
OTADATA WRITE REVIEW
Source: $BACKUP
Read-only snapshot: $WORK_DIR/otadata.bin
SHA-256: $BACKUP_SHA
Byte and sector range: $OTADATA_OFFSET_HEX..$(fmt_hex $((OTADATA_OFFSET + OTADATA_SIZE - 1))) ($OTADATA_SIZE bytes)
Selected slot: $SLOT. Its current image passed local checksum and hash inspection.
No application image or other partition will be written.
EOF
confirm_write 'restore otadata'
write_and_verify "$OTADATA_OFFSET_HEX" "$OTADATA_SIZE_HEX" "$WORK_DIR/otadata.bin"
echo 'OK: otadata write/readback verified'
espflash reset --chip "$CHIP" "${PORT_ARGS[@]}"

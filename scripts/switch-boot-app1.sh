#!/usr/bin/env bash

set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

IMAGE="$DEFAULT_IMAGE"
BACKUP=""
BACKUP_SHA=""
YES=0
while (($#)); do
  case "$1" in
    --image) IMAGE="${2:?missing image}"; shift 2 ;;
    --backup) BACKUP="${2:?missing backup}"; shift 2 ;;
    --backup-sha256) BACKUP_SHA="${2:?missing digest}"; shift 2 ;;
    --yes|-y) YES=1; shift ;;
    --help|-h)
      echo "Usage: $0 --backup PATH --backup-sha256 SHA256 [--image PATH] [--yes]"
      echo 'First switch only: verifies live app1 bytes and sequence-1 stock otadata, then writes sector 1 at 0xF000.'
      exit 0 ;;
    *) echo "error: unknown argument: $1" >&2; exit 1 ;;
  esac
done

private_workspace
snapshot_file "$IMAGE" "$WORK_DIR/app1.bin"
snapshot_file "$BACKUP" "$WORK_DIR/otadata.bin"
verify_backup "$WORK_DIR/otadata.bin" "$BACKUP_SHA" "$OTADATA_SIZE"
python3 "$ROOT_DIR/scripts/inspect-otadata.py" "$WORK_DIR/otadata.bin" --expect-slot app0 --expect-sequence 1
"$ROOT_DIR/scripts/check-app1-image.sh" "$WORK_DIR/app1.bin" "$WORK_DIR/image-info.txt"
python3 - "$WORK_DIR" <<'PY'
from pathlib import Path
import sys
import zlib
workspace = Path(sys.argv[1])
backup = (workspace / "otadata.bin").read_bytes()
if backup[0x1000:] != b"\xff" * 0x1000:
    raise SystemExit("error: first boot switch requires an erased second OTA sector")
sector = bytearray(b"\xff" * 0x1000)
sector[:4] = (2).to_bytes(4, "little")
sector[28:32] = (zlib.crc32(sector[:4], 0xFFFFFFFF) & 0xFFFFFFFF).to_bytes(4, "little")
path = workspace / "select-app1.bin"
path.write_bytes(sector)
path.chmod(0o400)
PY
probe_x4
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$APP1_OFFSET_HEX" "$(file_size "$WORK_DIR/app1.bin")" "$WORK_DIR/app1-readback.bin"
if ! cmp -s "$WORK_DIR/app1.bin" "$WORK_DIR/app1-readback.bin"; then
  echo 'error: live app1 differs from the reviewed image' >&2
  exit 1
fi
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$OTADATA_OFFSET_HEX" "$OTADATA_SIZE_HEX" "$WORK_DIR/otadata-readback.bin"
if ! cmp -s "$WORK_DIR/otadata.bin" "$WORK_DIR/otadata-readback.bin"; then
  echo 'error: live otadata differs from the reviewed first-switch backup' >&2
  exit 1
fi
cat <<EOF
FIRST APP1 BOOT SELECTION REVIEW
Read-only sector snapshot: $WORK_DIR/select-app1.bin
Sector SHA-256: $(sha256_file "$WORK_DIR/select-app1.bin")
Byte and sector range: 0x00F000..0x00FFFF (4096 bytes)
App1 image SHA-256: $(sha256_file "$WORK_DIR/app1.bin")
Previous otadata SHA-256: $BACKUP_SHA
Live bytes match both reviewed inputs. Only OTA sector 1 will change, to sequence 2 selecting app1.
EOF
confirm_write 'select app1'
write_and_verify 0xF000 0x1000 "$WORK_DIR/select-app1.bin"
echo 'OK: app1 boot selection write/readback verified'
espflash reset --chip "$CHIP" "${PORT_ARGS[@]}"

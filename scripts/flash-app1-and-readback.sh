#!/usr/bin/env bash

set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

IMAGE="$DEFAULT_IMAGE"
YES=0
MONITOR=0
while (($#)); do
  case "$1" in
    --image) IMAGE="${2:?missing image}"; shift 2 ;;
    --yes|-y) YES=1; shift ;;
    --monitor) MONITOR=1; shift ;;
    --help|-h)
      echo "Usage: $0 [--image PATH] [--yes] [--monitor]"
      echo "Writes only app1 at $APP1_OFFSET_HEX, verifies readback, then resets and optionally monitors."
      echo 'Requires an explicit ESPFLASH_PORT or ESPTOOL_PORT. Boot selection is unchanged.'
      exit 0 ;;
    *) echo "error: unknown argument: $1" >&2; exit 1 ;;
  esac
done

private_workspace
SOURCE_IMAGE="$IMAGE"
IMAGE="$WORK_DIR/app1.bin"
snapshot_file "$SOURCE_IMAGE" "$IMAGE"
"$ROOT_DIR/scripts/check-app1-image.sh" "$IMAGE" "$WORK_DIR/image-info.txt"
if (( MONITOR == 1 )); then
  snapshot_file "$ELF" "$WORK_DIR/monitor.elf"
fi
SIZE="$(file_size "$IMAGE")"
ERASE_SIZE="$(round_up_to_sector "$SIZE")"
IMAGE_SHA="$(sha256_file "$IMAGE")"
probe_x4

cat <<EOF
APP1 WRITE REVIEW
Source: $SOURCE_IMAGE
Read-only snapshot: $IMAGE
SHA-256: $IMAGE_SHA
Byte range: $APP1_OFFSET_HEX..$(fmt_hex $((APP1_OFFSET + SIZE - 1))) ($SIZE bytes)
Sector range: $APP1_OFFSET_HEX..$(fmt_hex $((APP1_OFFSET + ERASE_SIZE - 1)))
Partition limit: $APP1_OFFSET_HEX..$(fmt_hex $((APP1_OFFSET + APP1_SIZE - 1)))
All other partitions, including app0 and otadata, remain unchanged.
Readback precedes reset and monitoring. A failed readback leaves the chip in download mode.
EOF
confirm_write 'write app1'
write_and_verify "$APP1_OFFSET_HEX" "$SIZE" "$IMAGE"
printf 'OK: app1 write/readback verified, SHA-256 %s\n' "$IMAGE_SHA"
espflash reset --chip "$CHIP" "${PORT_ARGS[@]}"
if (( MONITOR == 1 )); then
  espflash monitor --chip "$CHIP" "${PORT_ARGS[@]}" --log-format defmt --elf "$WORK_DIR/monitor.elf"
fi

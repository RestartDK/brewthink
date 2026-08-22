#!/usr/bin/env bash
# Validate that a local Brewthink app image is safe-shaped for the X4 app1 slot.
# Read-only: this script does not talk to hardware and does not write flash.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

IMAGE="${1:-$DEFAULT_IMAGE}"
INFO_FILE="${2:-}"

require_cmd esptool
check_partition_table_constants

if [[ ! -f "$IMAGE" ]]; then
  echo "error: image not found: $IMAGE" >&2
  echo "hint: run scripts/build-app1-image.sh first" >&2
  exit 1
fi

SIZE="$(file_size "$IMAGE")"
ERASE_SIZE="$(round_up_to_sector "$SIZE")"

if (( SIZE <= 0 )); then
  echo "error: image is empty: $IMAGE" >&2
  exit 1
fi

if (( SIZE > APP1_SIZE )); then
  echo "error: image is larger than app1 partition" >&2
  echo "image size: $SIZE bytes" >&2
  echo "app1 size:  $APP1_SIZE bytes ($APP1_SIZE_HEX)" >&2
  exit 1
fi

if (( ERASE_SIZE > APP1_SIZE )); then
  echo "error: sector-rounded write would exceed app1 partition" >&2
  echo "image size:          $SIZE bytes" >&2
  echo "sector-rounded size: $ERASE_SIZE bytes" >&2
  echo "app1 size:           $APP1_SIZE bytes ($APP1_SIZE_HEX)" >&2
  exit 1
fi

TMP_INFO=""
if [[ -n "$INFO_FILE" ]]; then
  mkdir -p "$(dirname "$INFO_FILE")"
else
  TMP_INFO="$(mktemp)"
  INFO_FILE="$TMP_INFO"
fi
trap '[[ -z "${TMP_INFO:-}" ]] || rm -f "$TMP_INFO"' EXIT

esptool --chip "$CHIP" image-info "$IMAGE" > "$INFO_FILE"

require_info() {
  local pattern="$1"
  local description="$2"
  if ! grep -Eq "$pattern" "$INFO_FILE"; then
    echo "error: image-info missing expected $description" >&2
    echo "pattern: $pattern" >&2
    echo "full image-info:" >&2
    sed 's/^/  /' "$INFO_FILE" >&2
    exit 1
  fi
}

require_info '^ESP32-C3 Image Header$' 'ESP32-C3 image header'
require_info '^Flash size:[[:space:]]+16MB$' '16MB flash-size header'
require_info '^Flash freq:[[:space:]]+80m$' '80MHz flash-frequency header'
require_info '^Flash mode:[[:space:]]+DIO$' 'DIO flash-mode header'
require_info '^Chip ID:[[:space:]]+5 \(ESP32-C3\)$' 'ESP32-C3 chip ID'
require_info '^Checksum: .*\(valid\)$' 'valid image checksum'
require_info '^Validation hash: .*\(valid\)$' 'valid validation hash'
require_info '^Application Information$' 'ESP app descriptor/application information'
require_info '^Project name:[[:space:]]+brewthink$' 'brewthink app descriptor project name'

START=$APP1_OFFSET
END=$((APP1_OFFSET + SIZE - 1))
ERASE_END=$((APP1_OFFSET + ERASE_SIZE - 1))

cat <<EOF
OK: app1 image passed local checks
image:              $IMAGE
sha256:             $(sha256_file "$IMAGE")
image size:         $SIZE bytes
app1 partition:     $APP1_OFFSET_HEX..$(fmt_hex $((APP1_OFFSET + APP1_SIZE - 1))) ($APP1_SIZE bytes)
write byte range:   $(fmt_hex "$START")..$(fmt_hex "$END")
sector erase range: $(fmt_hex "$START")..$(fmt_hex "$ERASE_END")
image-info:         $INFO_FILE
EOF

#!/usr/bin/env bash
# Build Brewthink and generate a local ESP-IDF-format app image for the X4 app1 slot.
# Safe/read-only: this script writes only local files under artifacts/.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

IMAGE="${1:-$DEFAULT_IMAGE}"
INFO_FILE="${IMAGE}.image-info.txt"

require_cmd cargo
require_cmd espflash
require_cmd esptool
check_partition_table_constants

mkdir -p "$(dirname "$IMAGE")"

cd "$ROOT_DIR"

echo "Building release ELF for $CHIP..."
if [[ -n "${BREWTHINK_CARGO_FEATURES:-}" ]]; then
  cargo build --release --features "$BREWTHINK_CARGO_FEATURES"
else
  cargo build --release
fi

if [[ ! -f "$ELF" ]]; then
  echo "error: expected ELF not found after build: $ELF" >&2
  exit 1
fi

echo "Generating app1 image: $IMAGE"
espflash save-image \
  --chip "$CHIP" \
  --flash-mode dio \
  --flash-freq 80mhz \
  --flash-size 16mb \
  --xtal-freq "$EXPECTED_XTAL_FREQ" \
  --target-app-partition app1 \
  --partition-table "$PARTITION_TABLE" \
  --partition-table-offset "$PARTITION_TABLE_OFFSET" \
  "$ELF" \
  "$IMAGE"

"$ROOT_DIR/scripts/check-app1-image.sh" "$IMAGE" "$INFO_FILE"

cat <<EOF

Built app1 image only. No hardware was touched.
Next read-only inspection command:
  esptool --chip $CHIP image-info "$IMAGE"
EOF

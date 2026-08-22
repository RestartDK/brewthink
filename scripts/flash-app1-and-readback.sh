#!/usr/bin/env bash
# Guarded Xteink X4 app1-only flash workflow.
# This is the only script that writes hardware flash. It writes exactly one app image to app1
# at 0x650000, then reads back the same byte count and compares SHA-256 hashes.
# It does NOT write bootloader, partition table, NVS, filesystem, app0, or otadata.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

IMAGE="$DEFAULT_IMAGE"
YES=0
MONITOR=0

usage() {
  cat <<EOF
Usage: $0 [--image PATH] [--yes] [--monitor]

Environment:
  ESPFLASH_PORT=/dev/cu.usbmodemXXXX   Required explicit port for espflash/esptool
  ESPTOOL_PORT=/dev/cu.usbmodemXXXX    Optional fallback explicit port

This writes only app1:
  offset: $APP1_OFFSET_HEX
  size:   image byte length, must be <= $APP1_SIZE_HEX

It intentionally does not modify otadata, so stock app0 remains the selected boot slot.
EOF
}

while (($#)); do
  case "$1" in
    --image)
      IMAGE="${2:?missing value for --image}"
      shift 2
      ;;
    --yes|-y)
      YES=1
      shift
      ;;
    --monitor)
      MONITOR=1
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
require_cmd esptool
require_cmd cmp
check_partition_table_constants

PORT_ARGS=()
if [[ -n "${ESPFLASH_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPFLASH_PORT")
elif [[ -n "${ESPTOOL_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPTOOL_PORT")
else
  echo "error: refusing to write flash without an explicit port" >&2
  echo "set ESPFLASH_PORT=/dev/cu.usbmodemXXXX and retry" >&2
  exit 1
fi

if [[ ! -f "$IMAGE" ]]; then
  echo "error: image not found: $IMAGE" >&2
  echo "hint: run scripts/build-app1-image.sh first" >&2
  exit 1
fi

"$ROOT_DIR/scripts/check-app1-image.sh" "$IMAGE" "${IMAGE}.image-info.txt"

SIZE="$(file_size "$IMAGE")"
ERASE_SIZE="$(round_up_to_sector "$SIZE")"
START=$APP1_OFFSET
END=$((APP1_OFFSET + SIZE - 1))
ERASE_END=$((APP1_OFFSET + ERASE_SIZE - 1))
IMAGE_SHA="$(sha256_file "$IMAGE")"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
BOARD_INFO="$TMP_DIR/board-info.txt"
FLASH_ID="$TMP_DIR/flash-id.txt"
READBACK="$TMP_DIR/app1-readback.bin"

cat <<EOF

About to probe connected X4 with read-only commands.
Port: ${ESPFLASH_PORT:-${ESPTOOL_PORT:-auto-detect}}
EOF

espflash board-info --chip "$CHIP" "${PORT_ARGS[@]}" > "$BOARD_INFO"
esptool --chip "$CHIP" "${PORT_ARGS[@]}" flash-id > "$FLASH_ID"

require_probe() {
  local file="$1"
  local pattern="$2"
  local description="$3"
  if ! grep -Eiq "$pattern" "$file"; then
    echo "error: hardware probe did not confirm $description" >&2
    echo "pattern: $pattern" >&2
    echo "probe output:" >&2
    sed 's/^/  /' "$file" >&2
    exit 1
  fi
}

require_probe "$BOARD_INFO" 'Chip type:[[:space:]]+esp32c3' 'ESP32-C3 chip'
require_probe "$BOARD_INFO" 'Flash size:[[:space:]]+16MB' '16MB flash'
require_probe "$BOARD_INFO" 'Crystal frequency:[[:space:]]+40 MHz' '40 MHz crystal'
require_probe "$BOARD_INFO" 'Secure Boot:[[:space:]]+Disabled' 'disabled Secure Boot'
require_probe "$BOARD_INFO" 'Flash Encryption:[[:space:]]+Disabled' 'disabled Flash Encryption'

# esptool output varies slightly by version, so accept either compact ID or split fields.
if ! grep -Eiq '(852018|Manufacturer:[[:space:]]*85|Manufacturer:[[:space:]]*0x85)' "$FLASH_ID"; then
  echo "error: flash-id did not confirm expected manufacturer 0x85" >&2
  sed 's/^/  /' "$FLASH_ID" >&2
  exit 1
fi
if ! grep -Eiq '(852018|Device:[[:space:]]*2018|Device:[[:space:]]*0x2018)' "$FLASH_ID"; then
  echo "error: flash-id did not confirm expected device 0x2018" >&2
  sed 's/^/  /' "$FLASH_ID" >&2
  exit 1
fi

cat <<EOF

HARDWARE WRITE REVIEW
=====================
Will write:         $IMAGE
Image SHA-256:      $IMAGE_SHA
Image size:         $SIZE bytes
Write command:      espflash write-bin --chip $CHIP ${ESPFLASH_PORT:+--port $ESPFLASH_PORT }$APP1_OFFSET_HEX "$IMAGE"
App1 partition:     $APP1_OFFSET_HEX..$(fmt_hex $((APP1_OFFSET + APP1_SIZE - 1))) ($APP1_SIZE bytes)
Write byte range:   $(fmt_hex "$START")..$(fmt_hex "$END")
Sector erase range: $(fmt_hex "$START")..$(fmt_hex "$ERASE_END")

This script will NOT write:
  bootloader, partition table, nvs, otadata, app0, spiffs/LittleFS, or coredump.

Important consequence:
  otadata remains unchanged, so the stock app0 slot should remain selected at reboot.
EOF

if (( YES == 0 )); then
  echo
  read -r -p "Type exactly 'write app1' to continue: " CONFIRM
  if [[ "$CONFIRM" != "write app1" ]]; then
    echo "aborted: confirmation did not match"
    exit 1
  fi
fi

WRITE_ARGS=(write-bin --chip "$CHIP" "${PORT_ARGS[@]}")
if (( MONITOR == 1 )); then
  WRITE_ARGS+=(--monitor --log-format defmt --elf "$ELF")
fi
WRITE_ARGS+=("$APP1_OFFSET_HEX" "$IMAGE")

printf '\n== Writing app1 only ==\n'
espflash "${WRITE_ARGS[@]}"

printf '\n== Reading back app1 bytes for verification ==\n'
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" "$APP1_OFFSET_HEX" "$SIZE" "$READBACK"

READBACK_SHA="$(sha256_file "$READBACK")"
if ! cmp -s "$IMAGE" "$READBACK"; then
  echo "error: readback differs from image" >&2
  echo "image sha256:    $IMAGE_SHA" >&2
  echo "readback sha256: $READBACK_SHA" >&2
  exit 1
fi

cat <<EOF

OK: app1 write/readback verified
image sha256:    $IMAGE_SHA
readback sha256: $READBACK_SHA
verified range:  $(fmt_hex "$START")..$(fmt_hex "$END")

otadata was not modified by this script; stock app0 should still be the selected boot slot.
EOF

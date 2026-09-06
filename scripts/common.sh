#!/usr/bin/env bash
# Shared constants/helpers for Brewthink X4 tooling.
# Source this file from scripts; do not execute it directly.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

APP_NAME="brewthink"
CHIP="esp32c3"
CHIP_LABEL="ESP32-C3"
EXPECTED_CHIP_ID="5"
EXPECTED_FLASH_SIZE="16MB"
EXPECTED_FLASH_MODE="DIO"
EXPECTED_FLASH_FREQ="80m"
EXPECTED_XTAL_FREQ="40mhz"
EXPECTED_JEDEC_MANUFACTURER="0x85"
EXPECTED_JEDEC_DEVICE="0x2018"
EXPECTED_JEDEC_COMPACT="852018"

PARTITION_TABLE="$ROOT_DIR/docs/x4-stock-partition-table.csv"
PARTITION_TABLE_OFFSET="0x8000"
OTADATA_OFFSET_HEX="0xE000"
OTADATA_SIZE_HEX="0x2000"
OTADATA_OFFSET=$((OTADATA_OFFSET_HEX))
OTADATA_SIZE=$((OTADATA_SIZE_HEX))
APP0_OFFSET_HEX="0x10000"
APP0_SIZE_HEX="0x640000"
APP0_OFFSET=$((APP0_OFFSET_HEX))
APP0_SIZE=$((APP0_SIZE_HEX))
APP1_OFFSET_HEX="0x650000"
APP1_SIZE_HEX="0x640000"
APP1_OFFSET=$((APP1_OFFSET_HEX))
APP1_SIZE=$((APP1_SIZE_HEX))
FULL_FLASH_SIZE_HEX="0x1000000"
FULL_FLASH_SIZE=$((FULL_FLASH_SIZE_HEX))
FLASH_SECTOR_SIZE=$((0x1000))

ELF="$ROOT_DIR/target/riscv32imc-unknown-none-elf/release/$APP_NAME"
DEFAULT_IMAGE="$ROOT_DIR/artifacts/$APP_NAME-app1.bin"
STOCK_FLASH_BACKUP="$ROOT_DIR/backup/x4-stock.bin"

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: required command not found: $cmd" >&2
    exit 1
  fi
}

file_size() {
  local path="$1"
  if stat -f%z "$path" >/dev/null 2>&1; then
    stat -f%z "$path"
  else
    stat -c%s "$path"
  fi
}

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  else
    shasum -a 256 "$path" | awk '{print $1}'
  fi
}

round_up_to_sector() {
  local size="$1"
  echo $(( ((size + FLASH_SECTOR_SIZE - 1) / FLASH_SECTOR_SIZE) * FLASH_SECTOR_SIZE ))
}

fmt_hex() {
  printf '0x%06X' "$1"
}

check_partition_table_constants() {
  if [[ ! -f "$PARTITION_TABLE" ]]; then
    echo "error: missing partition table: $PARTITION_TABLE" >&2
    exit 1
  fi

  if ! grep -Eq '^otadata,data,ota,0xe000,0x2000,' "$PARTITION_TABLE"; then
    echo "error: $PARTITION_TABLE does not contain expected otadata row:" >&2
    echo "       otadata,data,ota,0xe000,0x2000," >&2
    exit 1
  fi

  if ! grep -Eq '^app0,app,ota_0,0x10000,0x640000,' "$PARTITION_TABLE"; then
    echo "error: $PARTITION_TABLE does not contain expected app0 row:" >&2
    echo "       app0,app,ota_0,0x10000,0x640000," >&2
    exit 1
  fi

  if ! grep -Eq '^app1,app,ota_1,0x650000,0x640000,' "$PARTITION_TABLE"; then
    echo "error: $PARTITION_TABLE does not contain expected app1 row:" >&2
    echo "       app1,app,ota_1,0x650000,0x640000," >&2
    exit 1
  fi
}

private_workspace() {
  umask 077
  WORK_DIR="$(mktemp -d)"
  trap 'rm -rf "$WORK_DIR"' EXIT
}

snapshot_file() {
  local source="$1" destination="$2"
  cp "$source" "$destination"
  chmod 400 "$destination"
}

verify_backup() {
  local path="$1" expected_sha="$2" expected_size="$3"
  if [[ ! "$expected_sha" =~ ^[[:xdigit:]]{64}$ ]]; then
    echo 'error: provide the reviewed backup SHA-256 with --backup-sha256' >&2
    exit 1
  fi
  if [[ "$(sha256_file "$path")" != "$(printf '%s' "$expected_sha" | tr '[:upper:]' '[:lower:]')" ]] ||
     (( $(file_size "$path") != expected_size )); then
    echo 'error: backup does not match the reviewed SHA-256 and size' >&2
    exit 1
  fi
}

explicit_port() {
  local port="${ESPFLASH_PORT:-${ESPTOOL_PORT:-}}"
  if [[ -z "$port" ]]; then
    echo 'error: an explicit ESPFLASH_PORT or ESPTOOL_PORT is required' >&2
    exit 1
  fi
  PORT_ARGS=(--port "$port")
}

probe_x4() {
  require_cmd espflash
  require_cmd esptool
  require_cmd cmp
  check_partition_table_constants
  explicit_port
  espflash board-info --chip "$CHIP" "${PORT_ARGS[@]}" > "$WORK_DIR/board-info.txt"
  esptool --chip "$CHIP" "${PORT_ARGS[@]}" flash-id > "$WORK_DIR/flash-id.txt"
  local pattern
  for pattern in \
    'Chip type:[[:space:]]+esp32c3' \
    'Flash size:[[:space:]]+16MB' \
    'Crystal frequency:[[:space:]]+40 MHz' \
    'Secure Boot:[[:space:]]+Disabled' \
    'Flash Encryption:[[:space:]]+Disabled'; do
    if ! grep -Eiq "$pattern" "$WORK_DIR/board-info.txt"; then
      echo "error: hardware probe did not confirm $pattern" >&2
      exit 1
    fi
  done
  if ! grep -Eiq '^Manufacturer:[[:space:]]*(0x)?85[[:space:]]*$' "$WORK_DIR/flash-id.txt" ||
     ! grep -Eiq '^Device:[[:space:]]*(0x)?2018[[:space:]]*$' "$WORK_DIR/flash-id.txt"; then
    echo 'error: hardware probe did not confirm the expected flash ID' >&2
    exit 1
  fi
}

verify_app_image() {
  esptool --chip "$CHIP" image-info "$1" > "$WORK_DIR/app-image-info.txt"
  grep -Eq '^ESP32-C3 Image Header$' "$WORK_DIR/app-image-info.txt"
  grep -Eq '^Checksum: .*\(valid\)$' "$WORK_DIR/app-image-info.txt"
  grep -Eq '^Validation hash: .*\(valid\)$' "$WORK_DIR/app-image-info.txt"
}

write_and_verify() {
  local offset="$1" size="$2" image="$3"
  local limit
  case "$offset" in
    "$APP1_OFFSET_HEX") limit="$APP1_SIZE" ;;
    "$APP0_OFFSET_HEX") limit="$APP0_SIZE" ;;
    "$OTADATA_OFFSET_HEX") limit="$OTADATA_SIZE" ;;
    0xF000) limit="$FLASH_SECTOR_SIZE" ;;
    *) echo 'error: refusing a write to a protected offset' >&2; exit 1 ;;
  esac
  if (( size <= 0 || size > limit || $(file_size "$image") != size )) ||
     { [[ "$offset" != "$APP1_OFFSET_HEX" ]] && (( size != limit )); }; then
    echo 'error: payload size does not match the reviewed partition range' >&2
    exit 1
  fi
  local readback="$WORK_DIR/readback.bin"
  espflash write-bin --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$offset" "$image"
  espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" --after no-reset "$offset" "$size" "$readback"
  if ! cmp -s "$image" "$readback"; then
    echo 'error: readback differs; leaving the chip in download mode' >&2
    exit 1
  fi
}

prepare_stock_backup() {
  require_cmd python3
  require_cmd esptool
  snapshot_file "$1" "$WORK_DIR/stock.bin"
  verify_backup "$WORK_DIR/stock.bin" "$2" "$FULL_FLASH_SIZE"
  python3 - "$WORK_DIR" "$APP0_OFFSET" "$APP0_SIZE" "$OTADATA_OFFSET" "$OTADATA_SIZE" <<'PY'
from pathlib import Path
import sys
workspace = Path(sys.argv[1])
data = (workspace / "stock.bin").read_bytes()
for name, offset, size in (("app0.bin", int(sys.argv[2]), int(sys.argv[3])),
                           ("otadata.bin", int(sys.argv[4]), int(sys.argv[5]))):
    path = workspace / name
    path.write_bytes(data[offset:offset + size])
    path.chmod(0o400)
PY
  python3 "$ROOT_DIR/scripts/inspect-otadata.py" "$WORK_DIR/otadata.bin" --expect-slot app0 --expect-sequence 1
  verify_app_image "$WORK_DIR/app0.bin"
}

confirm_write() {
  local phrase="$1"
  if (( YES == 0 )); then
    read -r -p "Type exactly '$phrase' to continue: " CONFIRM
    if [[ "$CONFIRM" != "$phrase" ]]; then
      echo 'aborted: confirmation did not match' >&2
      exit 1
    fi
  fi
}

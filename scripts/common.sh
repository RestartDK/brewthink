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

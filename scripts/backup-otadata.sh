#!/usr/bin/env bash

set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

private_workspace
probe_x4
OUT_DIR="$ROOT_DIR/backup/otadata"
mkdir -p "$OUT_DIR"
OUT="$(mktemp "$OUT_DIR/otadata-$(date +%Y%m%d-%H%M%S)-XXXXXX")"
printf 'Read-only backup: offset %s, size %s, output %s\n' "$OTADATA_OFFSET_HEX" "$OTADATA_SIZE_HEX" "$OUT"
espflash read-flash --chip "$CHIP" "${PORT_ARGS[@]}" "$OTADATA_OFFSET_HEX" "$OTADATA_SIZE_HEX" "$OUT"
if (( $(file_size "$OUT") != OTADATA_SIZE )); then
  echo "error: short backup retained at $OUT" >&2
  exit 1
fi
sha256_file "$OUT" > "$OUT.sha256"
chmod 400 "$OUT" "$OUT.sha256"
printf 'Backup retained at %s. No latest alias was replaced.\n' "$OUT"
python3 "$ROOT_DIR/scripts/inspect-otadata.py" "$OUT"

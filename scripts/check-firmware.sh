#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

while read -r stage features storage; do
  printf '\n== %s (%s) ==\n' "$stage" "$storage"
  BREWTHINK_DIAGNOSTIC_STAGE="$stage" \
  BREWTHINK_CARGO_FEATURES="${features//none/}" \
  BREWTHINK_PREVIOUS_FRAME_STORAGE="$storage" \
    scripts/build-app1-image.sh "artifacts/ci/$stage-$storage.bin"

  if [[ "$stage" == reader-app ]]; then
    python3 scripts/check-reader-stack.py target/riscv32imc-unknown-none-elf/release/brewthink
  fi
done <<'CONFIGURATIONS'
heartbeat none controller-ram
heartbeat none host-ram
storage-usb sd-diagnostic controller-ram
storage-write-test sd-write-diagnostic controller-ram
reader-app device-reader controller-ram
grayscale-bench grayscale-bench controller-ram
CONFIGURATIONS

REJECTION_LOG="artifacts/ci/reader-host-ram-rejection.txt"
if BREWTHINK_DIAGNOSTIC_STAGE=reader-app \
   BREWTHINK_PREVIOUS_FRAME_STORAGE=host-ram \
     cargo check --locked --bin brewthink --features device-reader > "$REJECTION_LOG" 2>&1; then
  echo 'error: reader-app with host-ram must not build' >&2
  exit 1
fi
grep -F 'reader-app requires controller-ram previous-frame storage' "$REJECTION_LOG"

#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_IMAGE="${1:-$ROOT_DIR/artifacts/brewthink-storage-write-test-app1.bin}"

BREWTHINK_DIAGNOSTIC_STAGE=storage-write-test \
BREWTHINK_CARGO_FEATURES=sd-write-diagnostic \
  "$ROOT_DIR/scripts/build-app1-image.sh" "$OUTPUT_IMAGE"

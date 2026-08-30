#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INPUT_ARG="${1:?usage: scripts/build-image-app1.sh INPUT_IMAGE [OUTPUT_APP1_IMAGE]}"
OUTPUT_IMAGE="${2:-$ROOT_DIR/artifacts/brewthink-image-app1.bin}"
INPUT_DIR="$(cd "$(dirname "$INPUT_ARG")" && pwd -P)"
INPUT_IMAGE="$INPUT_DIR/$(basename "$INPUT_ARG")"
PREVIEW="${OUTPUT_IMAGE%.bin}.pbm"

if [[ ! -f "$INPUT_IMAGE" ]]; then
  echo "error: input image not found: $INPUT_IMAGE" >&2
  exit 1
fi

BREWTHINK_DISPLAY_STAGE=image \
BREWTHINK_DISPLAY_ROTATION="${BREWTHINK_DISPLAY_ROTATION:-270}" \
BREWTHINK_IMAGE_PATH="$INPUT_IMAGE" \
BREWTHINK_IMAGE_SCALE="${BREWTHINK_IMAGE_SCALE:-contain}" \
BREWTHINK_IMAGE_DITHER="${BREWTHINK_IMAGE_DITHER:-ordered}" \
BREWTHINK_IMAGE_EXPORT="$PREVIEW" \
BREWTHINK_IMAGE_BUILD_ID="$$-$RANDOM" \
  "$ROOT_DIR/scripts/build-app1-image.sh" "$OUTPUT_IMAGE"

printf '\nPBM preview: %s\n' "$PREVIEW"

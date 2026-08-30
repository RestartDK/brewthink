#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INPUT_ARG="${1:?usage: scripts/build-image-app1.sh INPUT_IMAGE [OUTPUT_APP1_IMAGE]}"
INPUT_DIR="$(cd "$(dirname "$INPUT_ARG")" && pwd -P)"
INPUT_IMAGE="$INPUT_DIR/$(basename "$INPUT_ARG")"
OUTPUT_IMAGE="${2:-$ROOT_DIR/artifacts/brewthink-image-app1.bin}"
FRAME="${OUTPUT_IMAGE%.bin}.frame.bin"
PREVIEW="${OUTPUT_IMAGE%.bin}.pbm"
ROTATION="${BREWTHINK_DISPLAY_ROTATION:-270}"
SCALE="${BREWTHINK_IMAGE_SCALE:-contain}"
DITHER="${BREWTHINK_IMAGE_DITHER:-ordered}"
HOST_TARGET="${HOST_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"

if [[ ! -f "$INPUT_IMAGE" ]]; then
  echo "error: input image not found: $INPUT_IMAGE" >&2
  exit 1
fi

case "$ROTATION" in
  0 | 180)
    WIDTH=800
    HEIGHT=480
    ;;
  90 | 270)
    WIDTH=480
    HEIGHT=800
    ;;
  *)
    echo "error: unsupported display rotation: $ROTATION" >&2
    exit 1
    ;;
esac

cd "$ROOT_DIR"

cargo run \
  --quiet \
  --release \
  --config 'unstable.build-std=["std","panic_abort"]' \
  --target "$HOST_TARGET" \
  --features host-image-tools \
  --bin prepare-image \
  -- \
  "$INPUT_IMAGE" \
  "$FRAME" \
  "$PREVIEW" \
  "$WIDTH" \
  "$HEIGHT" \
  "$SCALE" \
  "$DITHER"

BREWTHINK_DIAGNOSTIC_STAGE=display-image \
BREWTHINK_DISPLAY_ROTATION="$ROTATION" \
BREWTHINK_IMAGE_FRAME="$FRAME" \
  "$ROOT_DIR/scripts/build-app1-image.sh" "$OUTPUT_IMAGE"

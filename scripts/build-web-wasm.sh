#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE=debug
CARGO_PROFILE=()

if [[ "${1:-}" == "--release" ]]; then
  PROFILE=release
  CARGO_PROFILE=(--release)
elif [[ $# -ne 0 ]]; then
  echo "usage: scripts/build-web-wasm.sh [--release]" >&2
  exit 1
fi

command -v cargo >/dev/null || {
  echo "error: cargo is required" >&2
  exit 1
}
command -v wasm-bindgen >/dev/null || {
  echo "error: wasm-bindgen-cli is required" >&2
  exit 1
}

cd "$ROOT_DIR"

RUSTFLAGS="${RUSTFLAGS:-} -C panic=abort" cargo build \
  --config 'unstable.build-std=["std","panic_abort"]' \
  --target wasm32-unknown-unknown \
  --features web-sim \
  --bin web-sim \
  "${CARGO_PROFILE[@]}"

GENERATED_DIR="$ROOT_DIR/web/src/generated"
TEMP_DIR="$(mktemp -d "$ROOT_DIR/target/web-wasm-bindgen.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT

wasm-bindgen \
  --target web \
  --out-dir "$TEMP_DIR" \
  --out-name brewthink_web \
  "target/wasm32-unknown-unknown/$PROFILE/web-sim.wasm"

rm -rf "$GENERATED_DIR"
mv "$TEMP_DIR" "$GENERATED_DIR"
trap - EXIT

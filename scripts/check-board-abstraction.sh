#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="${HOST_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"

cd "$ROOT_DIR"

cargo fmt --all -- --check
cargo test --locked --lib --target "$HOST_TARGET"
cargo test --locked --lib --features device-reader,sd-write-diagnostic,epub,grayscale-bench --target "$HOST_TARGET"
cargo test --locked --features device-control --bin device-control --target "$HOST_TARGET"
cargo clippy --locked --target "$HOST_TARGET" \
  --features host-image-tools,device-control,device-reader,epub \
  --lib --bin prepare-image --bin device-control --bin inspect-epub --bin inspect-device-epub \
  -- -D warnings
cargo check --locked --lib --target wasm32-unknown-unknown

for features in '' sd-write-diagnostic sd-diagnostic device-reader grayscale-bench; do
  cargo clippy --locked --lib --bin brewthink --features "$features" -- -D warnings
done

cargo build --locked --target "$HOST_TARGET" \
  --features device-control,host-image-tools --bin device-control --bin prepare-image
DEVICE_CONTROL_BIN="target/$HOST_TARGET/debug/device-control" \
PREPARE_IMAGE_BIN="target/$HOST_TARGET/debug/prepare-image" \
  python3 -m unittest discover -s tools -p 'test_*.py'
python3 -m unittest discover -s scripts -p 'test_*.py'
scripts/check-firmware.sh

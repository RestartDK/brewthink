#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="${HOST_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"

cd "$ROOT_DIR"

printf '== Formatting ==\n'
cargo fmt --all -- --check

printf '\n== Host library tests (%s) ==\n' "$HOST_TARGET"
cargo test \
  --config 'unstable.build-std=["std","panic_abort"]' \
  --lib \
  --target "$HOST_TARGET"

printf '\n== Host SD write diagnostic tests (%s) ==\n' "$HOST_TARGET"
cargo test \
  --config 'unstable.build-std=["std","panic_abort"]' \
  --features sd-write-diagnostic \
  --lib \
  --target "$HOST_TARGET"

printf '\n== Host image tool clippy ==\n'
cargo clippy \
  --config 'unstable.build-std=["std","panic_abort"]' \
  --features host-image-tools \
  --bin prepare-image \
  --target "$HOST_TARGET" \
  -- -D warnings

printf '\n== WASM-compatible library check ==\n'
cargo check --lib --target wasm32-unknown-unknown

printf '\n== Embedded target check ==\n'
cargo check

printf '\n== Embedded target clippy ==\n'
cargo clippy --lib --bin brewthink -- -D warnings

printf '\n== Embedded SD write diagnostic Clippy ==\n'
cargo clippy --lib --bin brewthink --features sd-write-diagnostic -- -D warnings

printf '\n== ESP32-C3 app1 image ==\n'
scripts/build-app1-image.sh

#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="${HOST_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"

cd "$ROOT_DIR"

printf '== Formatting ==\n'
cargo fmt --all -- --check

printf '\n== Host board-spec tests (%s) ==\n' "$HOST_TARGET"
cargo test --lib --target "$HOST_TARGET"

printf '\n== WASM-compatible library check ==\n'
cargo check --lib --target wasm32-unknown-unknown

printf '\n== Embedded target check ==\n'
cargo check

printf '\n== Embedded target clippy ==\n'
cargo clippy --all-features --workspace -- -D warnings

printf '\n== ESP32-C3 app1 image ==\n'
scripts/build-app1-image.sh

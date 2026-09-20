#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="${HOST_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"
cd "$ROOT_DIR"

host() {
  cargo fmt --all -- --check
  cargo test --locked --lib --target "$HOST_TARGET"
  cargo test --locked --lib --features device-reader,sd-write-diagnostic,epub,grayscale-bench,web-sim --target "$HOST_TARGET"
  cargo test --locked --features device-control --bin device-control --target "$HOST_TARGET"
  cargo clippy --locked --target "$HOST_TARGET" \
    --features host-image-tools,device-control,device-reader,epub,web-sim \
    --lib --bin prepare-image --bin device-control --bin inspect-epub --bin inspect-device-epub \
    -- -D warnings
  cargo fmt --manifest-path host/Cargo.toml -- --check
  cargo test --locked --manifest-path host/Cargo.toml --target "$HOST_TARGET"
  cargo clippy --locked --manifest-path host/Cargo.toml --target "$HOST_TARGET" --all-targets -- -D warnings
}

firmware() {
  for features in '' sd-write-diagnostic sd-diagnostic device-reader grayscale-bench; do
    cargo clippy --locked --lib --bin brewthink --features "$features" -- -D warnings
  done
  scripts/check-firmware.sh
}

web() {
  bash scripts/check-simulator-parity.sh
  (cd web && bun run test:e2e && bun run test:e2e:dev)
}

case "${1:-host}" in
  host) host ;;
  firmware) firmware ;;
  web) web ;;
  all) host; firmware; web ;;
  *) echo 'usage: scripts/check.sh [host|firmware|web|all]' >&2; exit 2 ;;
esac

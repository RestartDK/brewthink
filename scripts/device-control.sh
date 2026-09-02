#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST="$(rustc -vV | awk '/^host:/ {print $2}')"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"

cd "$ROOT_DIR"
cargo build --quiet --target "$HOST" --features device-control --bin device-control
exec "$TARGET_DIR/$HOST/debug/device-control" "$@"

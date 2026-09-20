#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"
HOST="$(rustc -vV | awk '/^host:/ { print $2 }')"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/host/target}"
cargo build --quiet --locked --release --manifest-path host/Cargo.toml --target "$HOST" \
  --target-dir "$TARGET_DIR" --bin host-tool
exec "$TARGET_DIR/$HOST/release/host-tool" --root "$ROOT_DIR" "$@"

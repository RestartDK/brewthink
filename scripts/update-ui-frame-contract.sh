#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="${HOST_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"

cd "$ROOT_DIR"

BLESS_UI_FRAMES=1 cargo test \
  --config 'unstable.build-std=["std","panic_abort"]' \
  --lib ui_contract:: \
  --target "$HOST_TARGET"

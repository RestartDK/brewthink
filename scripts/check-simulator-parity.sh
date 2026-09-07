#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="${HOST_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"
cd "$ROOT_DIR"

python3 scripts/generate-simulator-fixtures.py
cargo test --locked --lib --features web-sim --target "$HOST_TARGET"
cargo clippy --locked --lib --features web-sim --target "$HOST_TARGET" -- -D warnings
cargo build --locked --bin simulator-oracle --features device-reader --target "$HOST_TARGET"
for fixture in text jpeg; do
  "target/$HOST_TARGET/debug/simulator-oracle" \
    "artifacts/simulator-parity/$fixture.epub" "artifacts/simulator-parity/$fixture"
done
cargo clippy --locked --bin web-sim --features web-sim --target wasm32-unknown-unknown -- -D warnings
cd web
bunx tsc --noEmit --strict --target ES2022 --module ESNext --moduleResolution Bundler --types node \
  parity-tests/reader.spec.ts playwright.parity.config.ts
bun run build
bunx playwright test --config playwright.parity.config.ts

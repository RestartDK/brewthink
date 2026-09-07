#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST_TARGET="${HOST_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"
cd "$ROOT_DIR"

ARTIFACTS="artifacts/simulator-parity"
python3 scripts/generate-simulator-fixtures.py "$ARTIFACTS/fixtures"
diff -qr web/tests/fixtures/parity "$ARTIFACTS/fixtures"
cargo test --locked --lib --features web-sim --target "$HOST_TARGET"
cargo clippy --locked --lib --features web-sim --target "$HOST_TARGET" -- -D warnings
cargo build --locked --bin simulator-oracle --features device-reader --target "$HOST_TARGET"
cargo clippy --locked --bin simulator-oracle --features device-reader --target "$HOST_TARGET" -- -D warnings
ORACLE="target/$HOST_TARGET/debug/simulator-oracle"
for fixture in text jpeg no-cover frame-limit shelf-only-cover shelf-limit oversized-cover \
  compressed-oversized-cover unsupported-cover broken-cover broken-jpeg malformed-nav no-nav ncx; do
  "$ORACLE" "$ARTIFACTS/fixtures/$fixture.epub" "$ARTIFACTS/$fixture"
  printf 'Native oracle: %s\n' "$fixture"
done
for rejection in malformed:Malformed oversized-chapter:ResourceTooLarge too-many-chapters:TooManySpineItems; do
  fixture="${rejection%%:*}"
  reason="${rejection#*:}"
  if "$ORACLE" "$ARTIFACTS/fixtures/$fixture.epub" "$ARTIFACTS/$fixture" > "$ARTIFACTS/$fixture.log" 2>&1; then
    printf 'error: native oracle accepted %s\n' "$fixture" >&2
    exit 1
  fi
  grep -q "$reason" "$ARTIFACTS/$fixture.log"
done
cargo clippy --locked --bin web-sim --features web-sim --target wasm32-unknown-unknown -- -D warnings
cd web
bun run build
bunx playwright test --config playwright.parity.config.ts

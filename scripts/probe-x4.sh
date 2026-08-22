#!/usr/bin/env bash
# Probe a connected Xteink X4/ESP32-C3 using read-only commands.
# This prints private device identifiers such as MAC address; do not commit raw output.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/common.sh
source "$ROOT_DIR/scripts/common.sh"

require_cmd espflash
require_cmd esptool

PORT_ARGS=()
if [[ -n "${ESPFLASH_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPFLASH_PORT")
elif [[ -n "${ESPTOOL_PORT:-}" ]]; then
  PORT_ARGS+=(--port "$ESPTOOL_PORT")
fi

cat <<EOF
Running read-only X4 probe.
WARNING: output may include private MAC/eFuse/device identifiers. Do not commit raw output.
Port: ${ESPFLASH_PORT:-${ESPTOOL_PORT:-auto-detect}}
EOF

echo
printf '== espflash board-info ==\n'
espflash board-info --chip "$CHIP" "${PORT_ARGS[@]}"

echo
printf '== esptool flash-id ==\n'
esptool --chip "$CHIP" "${PORT_ARGS[@]}" flash-id

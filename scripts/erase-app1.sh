#!/usr/bin/env bash

set -euo pipefail

echo 'error: flash erasure is prohibited by AGENTS.md; app1 must remain intact during stock recovery' >&2
exit 1

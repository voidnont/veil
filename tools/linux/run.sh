#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
EXE="$ROOT/tools/build/release/veil-browser"
LOGDIR="$ROOT/tools/logs"
mkdir -p "$LOGDIR"
if [[ ! -x "$EXE" ]]; then
  echo "Veil Browser has not been built yet. Run tools/linux/build.sh first." >&2
  exit 1
fi
exec "$EXE" "$@"

#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="${1:-$REPO_ROOT/.gecko-src}"

python3 "$REPO_ROOT/tools/gecko_manifest.py" \
  --repo-root "$REPO_ROOT" \
  --source "$SOURCE_DIR"

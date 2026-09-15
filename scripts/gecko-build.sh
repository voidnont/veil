#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="${1:-$REPO_ROOT/.gecko-src}"

if [[ ! -d "$SOURCE_DIR/.git" ]]; then
  echo "error: Gecko checkout not found at $SOURCE_DIR; run scripts/gecko-bootstrap.sh first" >&2
  exit 1
fi

python3 "$REPO_ROOT/tools/gecko_manifest.py" \
  --repo-root "$REPO_ROOT" \
  --source "$SOURCE_DIR" \
  --allow-dirty

case "$(uname -s)" in
  Darwin)
    MOZCONFIG="$REPO_ROOT/gecko/mozconfig.macos"
    ;;
  Linux)
    MOZCONFIG="$REPO_ROOT/gecko/mozconfig.linux"
    ;;
  *)
    echo "error: scripts/gecko-build.sh supports Linux and macOS; use gecko-build.ps1 on Windows" >&2
    exit 1
    ;;
esac

cp "$MOZCONFIG" "$SOURCE_DIR/.mozconfig"

cd "$SOURCE_DIR"
./mach --no-interactive bootstrap --application-choice="Firefox for Desktop"
./mach build
./mach package

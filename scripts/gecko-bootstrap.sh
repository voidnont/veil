#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="${1:-$REPO_ROOT/.gecko-src}"
UPSTREAM_URL="https://github.com/mozilla-firefox/firefox.git"
REVISION_FILE="$REPO_ROOT/gecko/REVISION"
SERIES_FILE="$REPO_ROOT/gecko/patches/series"

python3 "$REPO_ROOT/tools/gecko_manifest.py" --repo-root "$REPO_ROOT" --manifest-only
REVISION="$(tr -d '\r\n' < "$REVISION_FILE")"

if [[ -e "$SOURCE_DIR" && ! -d "$SOURCE_DIR/.git" ]]; then
  echo "error: $SOURCE_DIR exists but is not a Git checkout" >&2
  exit 1
fi

mkdir -p "$SOURCE_DIR"
if [[ ! -d "$SOURCE_DIR/.git" ]]; then
  git -C "$SOURCE_DIR" init
fi

git -C "$SOURCE_DIR" config core.longpaths true
if git -C "$SOURCE_DIR" remote get-url origin >/dev/null 2>&1; then
  git -C "$SOURCE_DIR" remote set-url origin "$UPSTREAM_URL"
else
  git -C "$SOURCE_DIR" remote add origin "$UPSTREAM_URL"
fi

git -C "$SOURCE_DIR" fetch --depth=1 origin "$REVISION"
git -C "$SOURCE_DIR" checkout --detach --force "$REVISION"
git -C "$SOURCE_DIR" reset --hard "$REVISION"
git -C "$SOURCE_DIR" clean -ffdx

python3 "$REPO_ROOT/tools/gecko_manifest.py" \
  --repo-root "$REPO_ROOT" \
  --source "$SOURCE_DIR"

while IFS= read -r raw || [[ -n "$raw" ]]; do
  line="${raw#"${raw%%[![:space:]]*}"}"
  line="${line%"${line##*[![:space:]]}"}"
  [[ -z "$line" || "$line" == \#* ]] && continue
  git -C "$SOURCE_DIR" apply --whitespace=nowarn "$REPO_ROOT/gecko/patches/$line"
done < "$SERIES_FILE"

python3 "$REPO_ROOT/tools/gecko_branding.py" \
  --repo-root "$REPO_ROOT" \
  --source "$SOURCE_DIR"

echo "Prepared Gecko $REVISION with Veil branding in $SOURCE_DIR"

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

cp "$REPO_ROOT/gecko/mozconfig.android" "$SOURCE_DIR/.mozconfig"

SETTINGS_FILE="$SOURCE_DIR/settings.gradle"
GRADLE_ROOT_NAME='rootProject.name = "veil-gecko-android"'
if ! grep -Fq "$GRADLE_ROOT_NAME" "$SETTINGS_FILE"; then
  printf '\n// Veil: Gradle 9 rejects hidden checkout directory names as root project names.\n%s\n' \
    "$GRADLE_ROOT_NAME" >> "$SETTINGS_FILE"
fi

cd "$SOURCE_DIR"
./mach --no-interactive bootstrap --application-choice="GeckoView/Firefox for Android"
./mach build
./mach package
./mach gradle geckoview:publishWithGeckoBinariesDebugPublicationToMavenRepository
./mach android archive-geckoview

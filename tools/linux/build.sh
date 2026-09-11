#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TARGET="$ROOT/tools/build"
LOGDIR="$ROOT/tools/logs"
mkdir -p "$LOGDIR"
cd "$ROOT"
export CARGO_TARGET_DIR="$TARGET"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust/Cargo not found. Install from https://rustup.rs/" >&2
  exit 1
fi

echo "== Veil Browser 0.8.0 Linux build =="
echo "Rust: $(rustc --version 2>/dev/null || true)"
echo "Cargo: $(cargo --version 2>/dev/null || true)"
echo "Build output: $TARGET"

echo "Running tests..."
cargo test 2>&1 | tee "$LOGDIR/veil-build.log"

echo "Building release binaries..."
cargo build --release --bins 2>&1 | tee -a "$LOGDIR/veil-build.log"

echo
echo "Build complete"
echo "Browser: tools/build/release/veil-browser"
echo "Engine:  tools/build/release/veil-engine"

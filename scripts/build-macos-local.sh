#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="debug"
CARGO_ARGS=()
TARGET_DIR="$ROOT/target/debug"
OUTPUT_DIR="$ROOT/dist/macos"

if [[ "${1:-}" == "--release" ]]; then
  PROFILE="release"
  CARGO_ARGS=(--release)
  TARGET_DIR="$ROOT/target/release"
elif [[ "${1:-}" != "" ]]; then
  echo "Usage: $0 [--release]" >&2
  exit 1
fi

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "$1 is required but was not found in PATH" >&2
    exit 1
  fi
}

require_cmd npm
require_cmd cargo

cd "$ROOT/ui"
npm ci
npm run build

cd "$ROOT"
if [[ "$PROFILE" == "release" ]]; then
  cargo build --release
else
  cargo build
fi

mkdir -p "$OUTPUT_DIR"
cp "$TARGET_DIR/rustproxy" "$OUTPUT_DIR/rustproxy"

if command -v file >/dev/null 2>&1; then
  file "$OUTPUT_DIR/rustproxy"
fi

echo "macOS $PROFILE binary written to: $OUTPUT_DIR/rustproxy"
echo "Run with: $OUTPUT_DIR/rustproxy $ROOT/config.yaml"

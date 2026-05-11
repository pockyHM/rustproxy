#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT/dist/linux-x86_64"
TARGET_TRIPLE="x86_64-unknown-linux-gnu"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "$1 is required but was not found in PATH" >&2
    exit 1
  fi
}

require_cmd npm
require_cmd cargo
require_cmd rustc

HOST_TRIPLE="$(rustc -vV | awk '/^host:/ { print $2 }')"
if [[ "$HOST_TRIPLE" != "$TARGET_TRIPLE" ]]; then
  echo "This native script must run on Linux x86_64 ($TARGET_TRIPLE)." >&2
  echo "Current Rust host target: $HOST_TRIPLE" >&2
  echo "For macOS-to-Linux builds, use scripts/build-linux-x86_64.sh or install a Linux cross linker." >&2
  exit 1
fi

cd "$ROOT/ui"
npm ci
npm run build

cd "$ROOT"
cargo build --release --target "$TARGET_TRIPLE"

mkdir -p "$OUTPUT_DIR"
cp "$ROOT/target/$TARGET_TRIPLE/release/rustproxy" "$OUTPUT_DIR/rustproxy"

if command -v file >/dev/null 2>&1; then
  file "$OUTPUT_DIR/rustproxy"
fi

echo "Linux x86_64 binary written to: $OUTPUT_DIR/rustproxy"

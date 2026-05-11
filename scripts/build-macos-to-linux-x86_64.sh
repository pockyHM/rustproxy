#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT/dist/linux-x86_64"
TARGET_TRIPLE="x86_64-unknown-linux-gnu"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "$1 is required but was not found in PATH" >&2
    return 1
  fi
}

missing_tools=0
for cmd in npm cargo rustup zig; do
  if ! require_cmd "$cmd"; then
    missing_tools=1
  fi
done

if ! require_cmd cargo-zigbuild; then
  missing_tools=1
fi

if [[ "$missing_tools" -ne 0 ]]; then
  cat >&2 <<'EOF'

Install missing tools with:
  brew install zig
  cargo install cargo-zigbuild
  rustup target add x86_64-unknown-linux-gnu

Node/npm and Rust must also be installed.
EOF
  exit 1
fi

if ! rustup target list --installed | grep -qx "$TARGET_TRIPLE"; then
  echo "Installing Rust target: $TARGET_TRIPLE"
  rustup target add "$TARGET_TRIPLE"
fi

cd "$ROOT/ui"
npm ci
npm run build

cd "$ROOT"
cargo zigbuild --release --target "$TARGET_TRIPLE"

mkdir -p "$OUTPUT_DIR"
cp "$ROOT/target/$TARGET_TRIPLE/release/rustproxy" "$OUTPUT_DIR/rustproxy"

if command -v file >/dev/null 2>&1; then
  file "$OUTPUT_DIR/rustproxy"
fi

echo "Linux x86_64 binary written to: $OUTPUT_DIR/rustproxy"

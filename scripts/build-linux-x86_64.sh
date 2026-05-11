#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE_NAME="rustproxy-linux-x86_64-builder"
OUTPUT_DIR="$ROOT/dist/linux-x86_64"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required to build the Linux x86_64 release" >&2
  exit 1
fi

docker build \
  --platform linux/amd64 \
  -f "$ROOT/Dockerfile.build-linux-x86_64" \
  -t "$IMAGE_NAME" \
  "$ROOT"

docker run --rm \
  --platform linux/amd64 \
  -v "$ROOT":/work \
  -w /work \
  "$IMAGE_NAME" \
  bash -lc '
    set -euo pipefail

    cd /work/ui
    npm ci
    npm run build

    cd /work
    cargo build --release --target x86_64-unknown-linux-gnu

    mkdir -p /work/dist/linux-x86_64
    cp /work/target/x86_64-unknown-linux-gnu/release/rustproxy /work/dist/linux-x86_64/rustproxy
    file /work/dist/linux-x86_64/rustproxy
  '

echo "Linux x86_64 binary written to: $OUTPUT_DIR/rustproxy"

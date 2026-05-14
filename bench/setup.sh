#!/usr/bin/env bash
set -euo pipefail

echo "=== Benchmark Setup ==="

# Install system deps
if command -v apt-get &>/dev/null; then
    sudo apt-get update -qq
    sudo apt-get install -y -qq nginx haproxy build-essential libssl-dev git > /dev/null
elif command -v yum &>/dev/null; then
    sudo yum install -y nginx haproxy gcc make openssl-devel git
else
    echo "Unsupported package manager. Install nginx, haproxy, gcc, make, openssl-dev manually."
    exit 1
fi

# Install wrk
if ! command -v wrk &>/dev/null; then
    echo "Building wrk..."
    cd /tmp && git clone https://github.com/wg/wrk.git 2>/dev/null || true
    cd /tmp/wrk && make -j$(nproc) && sudo cp wrk /usr/local/bin/
    echo "wrk installed."
fi

# Install wrk2
if ! command -v wrk2 &>/dev/null; then
    echo "Building wrk2..."
    cd /tmp && git clone https://github.com/giltene/wrk2.git 2>/dev/null || true
    cd /tmp/wrk2 && make -j$(nproc) && sudo cp wrk /usr/local/bin/wrk2
    echo "wrk2 installed."
fi

# Build rustproxy release
echo "Building rustproxy --release..."
cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
cargo build --release

echo "=== Setup Complete ==="

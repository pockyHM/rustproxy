# macOS to Linux x86_64 Zig Build Script Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a non-Docker macOS-to-Linux x86_64 build script using Zig and cargo-zigbuild.

**Architecture:** Keep the current Linux-native and Docker scripts unchanged. Add a dedicated macOS cross-build shell script that builds the embedded UI first, then uses `cargo zigbuild` to compile the Rust binary for `x86_64-unknown-linux-gnu`.

**Tech Stack:** Bash, npm, Rust/cargo, rustup, Zig, cargo-zigbuild

---

### Task 1: Add macOS Zig cross-build script

**Files:**
- Create: `scripts/build-macos-to-linux-x86_64.sh`

- [ ] **Step 1: Create the script**

```bash
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

if ! cargo zigbuild --version >/dev/null 2>&1; then
  echo "cargo-zigbuild is required but was not found" >&2
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
```

- [ ] **Step 2: Make it executable**

Run:

```bash
chmod +x scripts/build-macos-to-linux_x86_64.sh
```

If the command fails because of the underscore typo, use the correct file path:

```bash
chmod +x scripts/build-macos-to-linux-x86_64.sh
```

- [ ] **Step 3: Verify failure/help path if dependencies are missing**

Run:

```bash
scripts/build-macos-to-linux-x86_64.sh
```

Expected on a machine without all requirements: a clear missing-tool message and install commands.

Expected on a configured macOS machine: `dist/linux-x86_64/rustproxy` is produced.

---

## Self-Review Checklist

1. **Spec coverage:** The script checks required tools, installs the target if needed, builds UI, runs `cargo zigbuild`, and copies to `dist/linux-x86_64/rustproxy`.
2. **Placeholder scan:** No placeholders remain.
3. **Type consistency:** File names and target triple consistently use `build-macos-to-linux-x86_64.sh` and `x86_64-unknown-linux-gnu`.

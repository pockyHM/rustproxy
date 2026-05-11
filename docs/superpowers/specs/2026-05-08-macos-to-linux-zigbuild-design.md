# macOS to Linux x86_64 Zig Build Script Design

## Goal

Add a non-Docker build script for Apple Silicon macOS that produces a Linux x86_64 RustProxy binary with the embedded admin UI.

## Design

Create `scripts/build-macos-to-linux-x86_64.sh` as a dedicated macOS cross-build entrypoint. It will not replace the existing Linux-native or Docker build scripts.

The script will:
1. Resolve the repository root.
2. Require `npm`, `cargo`, `rustup`, `zig`, and `cargo-zigbuild`.
3. If a tool is missing, print the exact install commands:
   - `brew install zig`
   - `cargo install cargo-zigbuild`
   - `rustup target add x86_64-unknown-linux-gnu`
4. Ensure the Rust target `x86_64-unknown-linux-gnu` is installed.
5. Build the UI with `npm ci` and `npm run build` so `ui/dist` exists before Rust compilation.
6. Build Rust with `cargo zigbuild --release --target x86_64-unknown-linux-gnu`.
7. Copy the binary to `dist/linux-x86_64/rustproxy`.
8. Run `file` on the output when available.

## Scope

This script targets macOS-to-Linux x86_64 builds only. The existing scripts remain:
- `scripts/build-native-linux-x86_64.sh` for native Linux x86_64 builds.
- `scripts/build-linux-x86_64.sh` for Docker-backed builds.

## Verification

Run:

```bash
scripts/build-macos-to-linux-x86_64.sh
```

Expected output:

```text
dist/linux-x86_64/rustproxy
```

The output should be an ELF x86_64 Linux executable.

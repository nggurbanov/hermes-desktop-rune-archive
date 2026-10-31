#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
[ "$(uname -s)" = Darwin ] || { echo 'Requires macOS' >&2; exit 1; }
for cmd in cargo rustup node xcrun codesign ditto; do
  command -v "$cmd" >/dev/null || { echo "Missing prerequisite: $cmd" >&2; exit 1; }
done
out="${1:-$PWD/rune-macos-alpha}"
[ ! -e "$out" ] || { echo "Output already exists: $out" >&2; exit 1; }
mkdir -p "$out"
out="$(cd "$out" && pwd)"
export MACOSX_DEPLOYMENT_TARGET=13.0
export CARGO_TARGET_DIR="$PWD/apps/rune/target"
cargo +1.95.0 build --locked --release --manifest-path apps/rune/Cargo.toml
node apps/rune/scripts/package-macos.mjs --binary "$CARGO_TARGET_DIR/release/hermes-rune" --out "$out/Hermes.app" --version 0.1.0
codesign --force --sign - "$out/Hermes.app"
codesign --verify --strict --verbose=2 "$out/Hermes.app"
ditto -c -k --keepParent "$out/Hermes.app" "$out/Hermes-macos-alpha.zip"
shasum -a 256 "$out/Hermes-macos-alpha.zip"
file "$out/Hermes.app/Contents/MacOS/HermesDesktopRune"
otool -L "$out/Hermes.app/Contents/MacOS/HermesDesktopRune"

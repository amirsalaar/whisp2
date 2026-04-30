#!/bin/bash
# Build, sign, and launch Whisp.app for development.
# Tauri debug builds don't bind Info.plist to the binary, so codesign
# must be re-run after each build to embed the bundle ID + entitlements.

set -e
cd "$(dirname "$0")/.."

export PATH="$HOME/.cargo/bin:$HOME/.nvm/versions/node/v22.22.0/bin:/usr/local/bin:/usr/bin:/bin"

APP="target/debug/bundle/macos/Whisp2.app"
ENTITLEMENTS="src-tauri/entitlements.plist"

echo "==> Building Whisp.app (debug)..."
cargo tauri build --debug

echo "==> Signing with entitlements..."
codesign --force --deep --sign - --entitlements "$ENTITLEMENTS" "$APP"

echo "==> Launching $APP"
open "$APP"

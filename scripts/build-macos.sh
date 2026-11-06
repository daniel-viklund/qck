#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
export MACOSX_DEPLOYMENT_TARGET=13.0
cargo build --release --lib
app="${QCK_APP_OUTPUT:-dist/Qck.app}"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
swiftc -swift-version 5 -target "$(uname -m)-apple-macosx13.0" -O -import-objc-header native/macos/bridge.h native/macos/main.swift native/macos/ApplicationLauncher.swift native/macos/WindowAdjustment.swift target/release/libqck.a -framework AppKit -framework Carbon -framework ApplicationServices -o "$app/Contents/MacOS/qck"
cp native/macos/Info.plist "$app/Contents/Info.plist"
codesign --force --sign "${QCK_SIGN_IDENTITY:--}" "$app"
echo "Built $app"

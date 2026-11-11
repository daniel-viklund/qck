#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
test_dir=$(mktemp -d "${TMPDIR:-/tmp}/qck-reopen.XXXXXX")
trap 'rm -rf "$test_dir"' EXIT
fixture="$test_dir/QckReopenFixture.app"
mkdir -p "$fixture/Contents/MacOS"
cat > "$fixture/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.qck.reopen-test</string>
<key>CFBundleExecutable</key><string>fixture</string>
<key>CFBundleName</key><string>Qck Reopen Test</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
swiftc -swift-version 5 -target "$(uname -m)-apple-macosx13.0" -parse-as-library native/macos/tests/ReopenFixture.swift -o "$fixture/Contents/MacOS/fixture"
swiftc -swift-version 5 -target "$(uname -m)-apple-macosx13.0" -parse-as-library native/macos/ApplicationLauncher.swift native/macos/tests/ReopenTest.swift -o "$test_dir/test-reopen"
"$test_dir/test-reopen" "$fixture"

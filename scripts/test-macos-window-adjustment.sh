#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
test_dir=$(mktemp -d "${TMPDIR:-/tmp}/qck-window-adjustment.XXXXXX")
trap 'rm -rf "$test_dir"' EXIT
swiftc -swift-version 5 -parse-as-library native/macos/WindowAdjustment.swift native/macos/tests/WindowAdjustmentTest.swift -o "$test_dir/test-window-adjustment"
"$test_dir/test-window-adjustment"

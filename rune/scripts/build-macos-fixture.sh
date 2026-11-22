#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
bundle="$PWD/artifacts/Pickle Capture Fixture.app"
mkdir -p "$bundle/Contents/MacOS"
xcrun swiftc tests/native-fixtures/macos/CaptureFixture.swift \
  -target "$(uname -m)-apple-macosx14.0" -framework AppKit \
  -Xlinker -rpath -Xlinker /usr/lib/swift \
  -o "$bundle/Contents/MacOS/PickleCaptureFixture"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.pickle.reader.capture-fixture</string>
<key>CFBundleName</key><string>Pickle Capture Fixture</string>
<key>CFBundleExecutable</key><string>PickleCaptureFixture</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
codesign --verify --strict "$bundle"
printf 'Built controlled native test source: %s\n' "$bundle"

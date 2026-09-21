#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mode="${1:-debug}"
swift build -c "$mode"
mkdir -p dist
staging=$(mktemp -d "$PWD/dist/.pickle-build.XXXXXX")
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/Pickle.app/Contents/MacOS" "$staging/Pickle.app/Contents/Resources"
cp ".build/$mode/Pickle" "$staging/Pickle.app/Contents/MacOS/Pickle"
cp -R ".build/$mode/Pickle_PickleApp.bundle" "$staging/Pickle.app/Contents/Resources/"
cp -R browser-extension "$staging/Pickle.app/Contents/Resources/"
cp scripts/browser-host.py "$staging/Pickle.app/Contents/Resources/"
cp Resources/Info.plist "$staging/Pickle.app/Contents/Info.plist"
codesign --force --sign - --identifier com.pickle.reader "$staging/Pickle.app"
# Never overwrite an executable that a running instance may still have mapped.
if [ -d dist/Pickle.app ]; then mv dist/Pickle.app "$staging/Previous.app"; fi
mv "$staging/Pickle.app" dist/Pickle.app
printf 'Built %s/dist/Pickle.app\n' "$PWD"

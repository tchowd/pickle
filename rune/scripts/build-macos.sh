#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mode="${1:-debug}"
if [[ $# -gt 2 || ( "$mode" != debug && "$mode" != release ) ]]; then
  printf 'Usage: bash scripts/build-macos.sh [debug|release] [artifact-directory]\n' >&2
  exit 2
fi
artifact_directory="${2:-artifacts}"
if [[ "$artifact_directory" != /* ]]; then artifact_directory="$PWD/$artifact_directory"; fi
export MACOSX_DEPLOYMENT_TARGET=14.0
build_args=(--workspace --locked)
if [[ "$mode" == release ]]; then build_args+=(--release); fi
cargo build "${build_args[@]}"
mkdir -p -- "$artifact_directory"
artifact_directory=$(cd -- "$artifact_directory" && pwd -P)
build_target=$(rustc -vV | sed -n 's/^host: //p')
notice_args=(--target "$build_target" --output "$artifact_directory/notices-macos")
if [[ "$mode" == release ]]; then notice_args+=(--strict); fi
python3 packaging/generate-notices.py "${notice_args[@]}"
stage=$(mktemp -d "$artifact_directory/.pickle-stage.XXXXXX")
bundle="$stage/Pickle Rune.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$mode/pickle-app" "$bundle/Contents/MacOS/PickleRune"
cp packaging/macos/Info.plist "$bundle/Contents/Info.plist"
cp packaging/assets/Pickle.icns "$bundle/Contents/Resources/Pickle.icns"
cp packaging/THIRD-PARTY-NOTICES.md "$bundle/Contents/Resources/THIRD-PARTY-NOTICES.md"
cp "$artifact_directory/notices-macos/THIRD_PARTY_NOTICES.txt" "$bundle/Contents/Resources/THIRD_PARTY_NOTICES.txt"
cp "$artifact_directory/notices-macos/dependency-inventory.json" "$bundle/Contents/Resources/dependency-inventory.json"
cp -R ../browser-extension "$bundle/Contents/Resources/browser-extension"
if [[ -f "target/$mode/pickle-browser-host" ]]; then
  cp "target/$mode/pickle-browser-host" "$bundle/Contents/MacOS/pickle-browser-host"
fi
identity="${PICKLE_SIGN_IDENTITY:--}"
sign_args=(--force --sign "$identity")
if [[ "$identity" != - ]]; then sign_args+=(--options runtime --timestamp); fi
if [[ -f "$bundle/Contents/MacOS/pickle-browser-host" ]]; then codesign "${sign_args[@]}" "$bundle/Contents/MacOS/pickle-browser-host"; fi
codesign "${sign_args[@]}" "$bundle"
codesign --verify --strict --verbose=2 "$bundle"
# Retain the previous bundle instead of overwriting an executable a running process may map.
if [[ -d "$artifact_directory/Pickle Rune.app" ]]; then mv "$artifact_directory/Pickle Rune.app" "$stage/Previous.app"; fi
mv "$bundle" "$artifact_directory/Pickle Rune.app"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$artifact_directory/Pickle Rune.app" "$artifact_directory/PickleRune-macos.zip"
printf 'Built %s/Pickle Rune.app\n' "$artifact_directory"
printf 'Staging/previous bundle retained at %s\n' "$stage"
if [[ "$identity" == - ]]; then printf 'Development signing only. Developer ID signing and notarization are still required for release.\n'; fi

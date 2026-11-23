#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
case "$(uname -m)" in
  arm64) fixture_arch=arm64 ;;
  x86_64) fixture_arch=x86_64 ;;
  *) printf 'Unsupported Mac fixture architecture.\n' >&2; exit 1 ;;
esac
mkdir -p target/native-tests
xcrun swiftc -parse-as-library -O -target "$fixture_arch-apple-macosx14.0" \
  native/macos/PickleBridge.swift native/macos/PickleAudio.swift \
  native/macos/PickleInvocation.swift native/macos/tests/PageFixture.swift \
  -o target/native-tests/PageFixture
target/native-tests/PageFixture

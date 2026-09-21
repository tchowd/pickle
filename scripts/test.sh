#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
swift run PickleChecks
node Tests/BrowserExtensionTests/extract.test.js
python3 Tests/BrowserExtensionTests/host_test.py
./scripts/build-app.sh
./dist/Pickle.app/Contents/MacOS/Pickle --smoke-test

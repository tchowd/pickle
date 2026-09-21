#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
swift run PickleChecks
./scripts/build-app.sh
./dist/Pickle.app/Contents/MacOS/Pickle --smoke-test

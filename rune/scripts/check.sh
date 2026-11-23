#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo fmt --manifest-path tools/icon-builder/Cargo.toml --check
cargo clippy --manifest-path tools/icon-builder/Cargo.toml --target-dir target/icon-builder --locked --all-targets -- -D warnings
cargo test --manifest-path tools/icon-builder/Cargo.toml --target-dir target/icon-builder --locked
python3 packaging/test_notices.py
python3 packaging/test_windows_runtime.py
(
  cd ..
  node Tests/BrowserExtensionTests/extract.test.js
  python3 scripts/local-ai/test_gateway.py
  if [[ "$(uname -s)" == "Darwin" ]]; then
    python3 Tests/BrowserExtensionTests/host_test.py
    bash rune/native/macos/tests/run-page-fixture.sh
    bash rune/native/macos/tests/run-invocation-fixture.sh
  fi
)
mkdir -p artifacts
cargo run --locked -p pickle-app -- --scene sample --snap artifacts/reader.png --tree artifacts/reader-tree.json
cargo run --locked -p pickle-app -- --scene choosing --snap artifacts/chooser.png
cargo run --locked -p pickle-app -- --scene settings --snap artifacts/settings.png
cargo run --locked -p pickle-app -- --scene chart-bar --snap artifacts/chart-bar.png --tree artifacts/chart-bar-tree.json
cargo run --locked -p pickle-app -- --scene chart-flow --snap artifacts/chart-flow.png --tree artifacts/chart-flow-tree.json
cargo run --locked -p pickle-app -- --scene streaming --snap artifacts/streaming.png --tree artifacts/streaming-tree.json
cargo run --locked -p pickle-app -- --scene consent-online --snap artifacts/consent-online.png --tree artifacts/consent-online-tree.json

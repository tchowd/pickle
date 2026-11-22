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
(
  cd ..
  node Tests/BrowserExtensionTests/extract.test.js
  python3 scripts/local-ai/test_gateway.py
  if [[ "$(uname -s)" == "Darwin" ]]; then
    python3 Tests/BrowserExtensionTests/host_test.py
  fi
)
mkdir -p artifacts
cargo run --locked -p pickle-app -- --scene sample --snap artifacts/reader.png --tree artifacts/reader-tree.json
cargo run --locked -p pickle-app -- --scene choosing --snap artifacts/chooser.png
cargo run --locked -p pickle-app -- --scene settings --snap artifacts/settings.png

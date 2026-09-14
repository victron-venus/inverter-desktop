#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
pnpm install --frozen-lockfile --ignore-scripts
pnpm run format:check
pnpm run test:build-profiles
pnpm run test:mobile
pnpm run build:mobile
pnpm run build
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml --all-targets
python3 -m unittest discover -s scripts -p test_ios_version.py
python3 -m unittest discover -s tests -p test_mobile_native_boundary.py

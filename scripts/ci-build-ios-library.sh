#!/usr/bin/env bash
set -euo pipefail

# Raw cargo does not run Tauri's beforeBuildCommand. Build the frontend first
# and enable the production asset protocol in both validation and release IPAs.
pnpm run build
test -s dist/index.html
cargo build --locked --target aarch64-apple-ios --release \
  --features tauri/custom-protocol \
  --package inverter-dashboard --lib \
  --manifest-path src-tauri/Cargo.toml
mkdir -p src-tauri/gen/apple/Externals/arm64/release
cp src-tauri/target/aarch64-apple-ios/release/libinverter_dashboard_lib.a \
  src-tauri/gen/apple/Externals/arm64/release/libapp.a

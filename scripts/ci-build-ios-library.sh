#!/usr/bin/env bash
set -euo pipefail

# Raw cargo does not run Tauri's beforeBuildCommand. Build the frontend first
# and enable the production asset protocol in both validation and release IPAs.
pnpm run build:mobile
test -s dist/index.html

# Xcode 27's default SwiftPM backend internalizes the C entry points used by
# swift-rs. Keep the native backend for the Swift packages built by Cargo.
# Scope the shim to this build; other Swift commands retain their normal behavior.
xcode_major=$(xcodebuild -version | awk 'NR == 1 { split($2, version, "."); print version[1] }')
if (( xcode_major >= 27 )); then
  export INVERTER_REAL_SWIFT
  INVERTER_REAL_SWIFT=$(xcrun --find swift)
  swift_shim_dir=$(mktemp -d "${TMPDIR:-/tmp}/inverter-ios-swift.XXXXXX")
  trap 'rm -rf "$swift_shim_dir"' EXIT
  cat > "$swift_shim_dir/swift" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == build ]]; then
  shift
  exec "${INVERTER_REAL_SWIFT:?}" build --build-system native "$@"
fi
exec "${INVERTER_REAL_SWIFT:?}" "$@"
SH
  chmod +x "$swift_shim_dir/swift"
  export PATH="$swift_shim_dir:$PATH"
fi

cargo build --locked --target aarch64-apple-ios --release \
  --features tauri/custom-protocol \
  --package inverter-dashboard --lib \
  --manifest-path src-tauri/Cargo.toml
mkdir -p src-tauri/gen/apple/Externals/arm64/release
cp src-tauri/target/aarch64-apple-ios/release/libinverter_dashboard_lib.a \
  src-tauri/gen/apple/Externals/arm64/release/libapp.a

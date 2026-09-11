#!/usr/bin/env bash
# Build Inverter Dashboard for the iPhone iOS Simulator and launch it.
#
# Path chosen: Tauri CLI (`pnpm tauri ios run [DEVICE]`) after the same
# prereq / deps / ios-init flow as build-ios-local.sh. Tauri targets the
# iphonesimulator SDK and aarch64-apple-ios-sim (or x86_64-apple-ios on
# Intel), so no Apple Developer signing or IPA packaging is required.
set -uo pipefail

APPLE_DIR="src-tauri/gen/apple"
PROJECT_FILE="$APPLE_DIR/inverter-dashboard.xcodeproj/project.pbxproj"
DEPLOYMENT_TARGET="26.0"

usage() {
  echo "Usage: $0 [--clean] [--dev] [--update-deps] [--device \"iPhone 16\"]"
  echo "  --clean         Remove build artifacts before starting"
  echo "  --dev           Build/run debug instead of release"
  echo "  --update-deps   Update JS dependencies (pnpm update) before building"
  echo "  --device NAME   Simulator device name (default: DEVICE env or first available iPhone)"
  echo ""
  echo "Builds for the iOS Simulator (no Apple Developer signing required) and"
  echo "opens the app in Simulator via: pnpm tauri ios run \"<Device>\" [--release]"
  exit 1
}

CLEAN=false
RELEASE=true
UPDATE_DEPS=false
DEVICE_ARG=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --clean) CLEAN=true; shift ;;
    --dev)   RELEASE=false; shift ;;
    --update-deps) UPDATE_DEPS=true; shift ;;
    --device)
      if [[ $# -lt 2 ]]; then
        echo "  ✗ --device requires a simulator name"
        usage
      fi
      DEVICE_ARG="$2"
      shift 2
      ;;
    --help|-h) usage ;;
    *) usage ;;
  esac
done

# Prefer --device, then DEVICE env, then first available iPhone simulator.
resolve_device() {
  if [[ -n "$DEVICE_ARG" ]]; then
    echo "$DEVICE_ARG"
    return
  fi
  if [[ -n "${DEVICE:-}" ]]; then
    echo "$DEVICE"
    return
  fi
  local first
  first=$(xcrun simctl list devices available 2>/dev/null \
    | grep -E '^\s+iPhone ' \
    | head -1 \
    | sed -E 's/^[[:space:]]+//; s/ \([0-9A-Fa-f-]{36}\).*//')
  if [[ -z "$first" ]]; then
    echo ""
    return 1
  fi
  echo "$first"
}

boot_simulator() {
  local name="$1"
  local uuid
  uuid=$(xcrun simctl list devices available 2>/dev/null \
    | grep -F "$name (" \
    | head -1 \
    | sed -E 's/.*\(([0-9A-Fa-f-]{36})\).*/\1/')
  if [[ -z "$uuid" ]]; then
    echo "  ✗ Simulator device not found: $name"
    echo "  Available iPhones:"
    xcrun simctl list devices available 2>/dev/null | grep -E '^\s+iPhone ' || true
    exit 1
  fi
  local state
  state=$(xcrun simctl list devices 2>/dev/null | grep -F "$uuid" \
    | sed -E 's/.*\((Booted|Shutdown|Shutting Down|Creating|Booting)\).*/\1/' | head -1)
  if [[ "$state" != "Booted" ]]; then
    echo "  → Booting $name ($uuid)..."
    xcrun simctl boot "$uuid" 2>/dev/null || true
  else
    echo "  ✓ $name already booted ($uuid)"
  fi
  open -a Simulator
  # Give Simulator a moment to come up
  xcrun simctl bootstatus "$uuid" -b >/dev/null 2>&1 || true
}

echo "╔═════════════════════════════════════════════════════╗"
echo "║  Inverter Dashboard — iOS Simulator build script    ║"
echo "╠═════════════════════════════════════════════════════╣"
echo "║  What this script does:                             ║"
echo "║  1. Check prerequisites (Xcode, Rust iOS-sim target)║"
echo "║  2. Install JS dependencies (pnpm install)          ║"
echo "║  3. Init iOS project if needed                      ║"
echo "║  4. Select/boot an iPhone Simulator                 ║"
echo "║  5. Build & launch via: pnpm tauri ios run          ║"
echo "║     (iphonesimulator SDK — no signing / no IPA)     ║"
echo "╚═════════════════════════════════════════════════════╝"
echo ""

ARCH=$(uname -m)
if [[ "$ARCH" == "x86_64" ]]; then
  RUST_TARGET="x86_64-apple-ios"
else
  RUST_TARGET="aarch64-apple-ios-sim"
fi
echo "===> Host arch: $ARCH → Rust target: $RUST_TARGET"
echo ""

if [ "$CLEAN" = true ]; then
  echo "===> Cleaning project..."
  rm -rf "$APPLE_DIR/build" src-tauri/target
  echo "  ✓ Cleaned"
  echo ""
fi

# ---------- Xcode ----------
echo "===> Checking Xcode..."
if ! command -v xcodebuild >/dev/null 2>&1; then
  echo "  ✗ xcodebuild not found — install Xcode from App Store"
  exit 1
fi
XCODE_VER=$(xcodebuild -version | head -1)
echo "  ✓ $XCODE_VER"

if ! command -v xcrun >/dev/null 2>&1; then
  echo "  ✗ xcrun not found — install Xcode Command Line Tools"
  exit 1
fi
if ! xcrun simctl help >/dev/null 2>&1; then
  echo "  ✗ simctl unavailable — open Xcode once to finish setup"
  exit 1
fi
echo "  ✓ simctl available"

# ---------- Rust iOS Simulator target ----------
echo "===> Checking Rust iOS Simulator target..."
if ! rustup target list --installed | grep -q "^${RUST_TARGET}$"; then
  echo "  → Installing Rust target: $RUST_TARGET"
  rustup target add "$RUST_TARGET"
else
  echo "  ✓ Target $RUST_TARGET already installed"
fi

# ---------- JS Deps ----------
echo ""
if [ "$UPDATE_DEPS" = true ]; then
  echo "===> Updating dependencies..."
  pnpm update
  echo ""
  echo "===> Regenerating lockfile..."
  pnpm install
  echo ""
  echo "  ⚠  Commit pnpm-lock.yaml before building:"
  echo "      git add pnpm-lock.yaml && git commit -m 'chore: update deps'"
else
  echo "===> Installing dependencies (frozen lockfile)..."
  pnpm install --frozen-lockfile
fi

# ---------- iOS project init ----------
echo ""
echo "===> Initializing iOS project (if needed)..."
if [ ! -d "$APPLE_DIR" ]; then
  echo "  → Running: pnpm tauri ios init --ci"
  SERVER_ADDR_FILE='' pnpm tauri ios init --ci
else
  echo "  ✓ iOS project already exists"
fi

# ---------- Set deployment target ----------
echo ""
echo "===> Setting IPHONEOS_DEPLOYMENT_TARGET to $DEPLOYMENT_TARGET..."
if [ -f "$PROJECT_FILE" ]; then
  sed -i.bak "s/IPHONEOS_DEPLOYMENT_TARGET = [0-9.]*;/IPHONEOS_DEPLOYMENT_TARGET = ${DEPLOYMENT_TARGET};/" "$PROJECT_FILE"
  rm -f "$PROJECT_FILE.bak"
  echo "  ✓ Deployment target set"
else
  echo "  ! Project file not found at $PROJECT_FILE (tauri may recreate it)"
fi

# ---------- Device / Simulator ----------
echo ""
echo "===> Selecting iPhone Simulator..."
SIM_DEVICE=$(resolve_device) || true
if [[ -z "${SIM_DEVICE:-}" ]]; then
  echo "  ✗ No iPhone simulator found. Install one via Xcode → Settings → Platforms,"
  echo "    or pass --device \"iPhone 16\" / set DEVICE=..."
  exit 1
fi
echo "  ✓ Using device: $SIM_DEVICE"
boot_simulator "$SIM_DEVICE"

# ---------- Build & launch via Tauri ----------
echo ""
CONFIG_LABEL="release"
TAURI_ARGS=(ios run "$SIM_DEVICE" --no-watch)
if [ "$RELEASE" = true ]; then
  TAURI_ARGS+=(--release)
else
  CONFIG_LABEL="debug"
fi
echo "===> Building & launching on Simulator (${CONFIG_LABEL})..."
echo "  → pnpm tauri ${TAURI_ARGS[*]}"
echo ""

# Tauri handles cargo (sim target) + xcodebuild (-sdk iphonesimulator) + install/launch.
if ! pnpm tauri "${TAURI_ARGS[@]}"; then
  echo ""
  echo "  ✗ tauri ios run failed"
  echo "  Tip: try --clean, ensure Xcode platforms include iOS Simulator,"
  echo "       or run: pnpm tauri ios init --ci"
  exit 1
fi

echo ""
echo "========================================"
echo "  iOS Simulator launch complete!"
echo "  Device: $SIM_DEVICE"
echo "  Mode:   $CONFIG_LABEL"
echo "  Target: $RUST_TARGET (iphonesimulator)"
echo "========================================"
echo ""
echo "Re-run with a specific device:"
echo "  ./build-ios-simulator.sh --device \"$SIM_DEVICE\""
echo "  DEVICE=\"$SIM_DEVICE\" ./build-ios-simulator.sh --dev"
echo ""
date

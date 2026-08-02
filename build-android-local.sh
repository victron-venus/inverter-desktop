#!/usr/bin/env bash
set -uo pipefail

OUTPUT_DIR="dist/android"
ANDROID_DIR="src-tauri/gen/android"

# Signing configuration (can be set via env vars or .env.local)
ANDROID_KEYSTORE_PATH="${ANDROID_KEYSTORE_PATH:-}"
ANDROID_KEYSTORE_PASSWORD="${ANDROID_KEYSTORE_PASSWORD:-}"
ANDROID_KEY_ALIAS="${ANDROID_KEY_ALIAS:-}"
ANDROID_KEY_PASSWORD="${ANDROID_KEY_PASSWORD:-}"
SIGN_APK="${SIGN_APK:-false}"

# Version from Cargo.toml (source of truth for Tauri)
VERSION=$(grep '^version = ' src-tauri/Cargo.toml | head -1 | cut -d'"' -f2)

usage() {
  echo "Usage: $0 [--clean] [--dev] [--sign]"
  echo "  --clean    Remove node_modules and build artifacts before starting"
  echo "  --dev      Build debug APK instead of release"
  echo "  --sign     Sign the release APK (requires keystore env vars)"
  echo ""
  echo "Environment variables for signing:"
  echo "  ANDROID_KEYSTORE_PATH    Path to .keystore/.jks file"
  echo "  ANDROID_KEYSTORE_PASSWORD Keystore password"
  echo "  ANDROID_KEY_ALIAS        Key alias"
  echo "  ANDROID_KEY_PASSWORD     Key password"
  exit 1
}

CLEAN=false
RELEASE=true
while [[ $# -gt 0 ]]; do
  case "$1" in
    --clean) CLEAN=true; shift ;;
    --dev)   RELEASE=false; shift ;;
    --sign)  SIGN_APK=true; shift ;;
    --help)  usage ;;
    *)       usage ;;
  esac
done

echo "╔══════════════════════════════════════════════════════╗"
echo "║  Inverter Dashboard — Android build script          ║"
echo "╠══════════════════════════════════════════════════════╣"
echo "║  What this script does:                             ║"
echo "║  1. Check & auto-install missing prerequisites:     ║"
echo "║     • Java 17+ (brew install --cask temurin@21)    ║"
echo "║     • Android SDK + NDK (sdkmanager)               ║"
echo "║     • Rust Android targets (rustup)                ║"
echo "║  2. Install JS dependencies (pnpm install)          ║"
echo "║  3. Init Android project if needed                  ║"
echo "║  4. Build APK (and AAB for release)                 ║"
if [ "$RELEASE" = true ] && [ "$SIGN_APK" = true ]; then
echo "║  5. Sign & align APK                                ║"
fi
echo "╚══════════════════════════════════════════════════════╝"
echo ""

if [ "$CLEAN" = true ]; then
  echo "===> Cleaning project..."
  rm -rf node_modules pnpm-lock.yaml package-lock.json dist src-tauri/target
  echo "  ✓ Cleaned"
  echo ""
fi

if [ -f ".env.local" ]; then
  echo "===> Loading .env.local..."
  set -o allexport
  source .env.local
  set +o allexport
  echo "  ✓ Loaded"
  echo ""
fi

echo "===> Checking & installing prerequisites..."
echo "    (all missing tools will be installed automatically)"

command -v pnpm >/dev/null 2>&1 || { echo "  ✗ pnpm not found"; exit 1; }

# ---------- Java ----------
if ! command -v java >/dev/null 2>&1; then
  echo "  → Installing Java 21 (temurin@21)..."
  brew install --cask temurin@21
  # After cask install, find the JDK path
  JAVA_HOME=$(/usr/libexec/java_home -v 21 2>/dev/null || echo "")
  if [ -n "$JAVA_HOME" ]; then
    export JAVA_HOME
    echo "  ✓ JAVA_HOME=$JAVA_HOME"
  else
    echo "  ✗ Java installed but JAVA_HOME not found"
    echo "    Set it manually, e.g.:"
    echo '    export JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home'
    exit 1
  fi
else
  echo "  ✓ Java: $(java -version 2>&1 | head -1)"
fi

# ---------- Android SDK ----------
if [ -z "${ANDROID_HOME:-}" ]; then
  # Common SDK locations
  for dir in "$HOME/Library/Android/sdk" "/opt/homebrew/share/android-commandlinetools"; do
    if [ -d "$dir" ]; then
      ANDROID_HOME="$dir"
      break
    fi
  done
fi

if [ -z "${ANDROID_HOME:-}" ] || [ ! -d "$ANDROID_HOME" ]; then
  echo "  → Installing Android command-line tools..."
  brew install --cask android-commandlinetools
  ANDROID_HOME="/opt/homebrew/share/android-commandlinetools"
  export ANDROID_HOME
fi

SDKMAN="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"

if [ ! -f "$ANDROID_HOME/platforms/android-34/android.jar" ]; then
  echo "  → Installing Android SDK platform & build-tools..."
  yes | "$SDKMAN" --sdk_root="$ANDROID_HOME" \
    "platforms;android-34" \
    "build-tools;34.0.0" 2>&1 | tail -3 || true
  if [ ! -f "$ANDROID_HOME/platforms/android-34/android.jar" ]; then
    echo "  ✗ SDK install failed — check '$SDKMAN --list'"
    exit 1
  fi
fi

# ---------- NDK ----------
if [ -z "${NDK_HOME:-}" ]; then
  NDK_SEARCH=$(find "$ANDROID_HOME/ndk" -maxdepth 1 -type d -name "*" ! -path "$ANDROID_HOME/ndk" 2>/dev/null | head -1)
  if [ -n "$NDK_SEARCH" ]; then
    NDK_HOME="$NDK_SEARCH"
    export NDK_HOME
  fi
fi

if [ -z "${NDK_HOME:-}" ] || [ ! -d "$NDK_HOME" ]; then
  # Pick the latest NDK version available
  NDK_VERSION="ndk;27.0.12077973"
  LATEST_NDK=$(yes | "$SDKMAN" --sdk_root="$ANDROID_HOME" \
    --list 2>/dev/null | grep "^[[:space:]]*ndk;" | tail -1 | awk -F'|' '{print $1}' | tr -d ' ')
  if [ -n "$LATEST_NDK" ]; then
    NDK_VERSION="$LATEST_NDK"
  fi
  NDK_DIR=$(echo "$NDK_VERSION" | cut -d';' -f2)
  NDK_HOME="$ANDROID_HOME/ndk/$NDK_DIR"
  if [ ! -d "$NDK_HOME" ]; then
    echo "  → Installing Android NDK ($NDK_VERSION)..."
    yes | "$SDKMAN" --sdk_root="$ANDROID_HOME" \
      "$NDK_VERSION" 2>&1 | tail -3 || true
  fi
  if [ ! -d "$NDK_HOME" ]; then
    echo "  ✗ NDK install failed — check '$SDKMAN --list'"
    echo "  You can set NDK_VERSION manually, e.g.:"
    echo '    export NDK_VERSION=ndk;27.0.12077973'
    exit 1
  fi
  export NDK_HOME
fi

# Set for this session
export ANDROID_HOME
export ANDROID_SDK_ROOT="$ANDROID_HOME"
echo "  ✓ ANDROID_HOME=$ANDROID_HOME"
echo "  ✓ NDK_HOME=$NDK_HOME"

# ---------- Rust targets ----------
for target in aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android; do
  if ! rustup target list --installed | grep -q "$target"; then
    echo "  → Installing Rust target: $target"
    rustup target add "$target"
  fi
done
echo "  ✓ Rust Android targets installed"

echo ""
echo "===> Installing dependencies..."
pnpm install

echo ""
echo "===> Initializing Android project (if needed)..."
if [ ! -d "$ANDROID_DIR" ]; then
  pnpm tauri android init --ci
else
  echo "  ✓ Android project already exists"
fi

if [ "$RELEASE" = true ]; then
  echo ""
  echo "===> Building Android (release)..."
  pnpm tauri android build --ci

  # ---------- Sign APK if requested ----------
  if [ "$SIGN_APK" = true ]; then
    echo ""
    echo "===> Signing APK..."

    # APK output directory (needed for signing)
    APK_DIR="$ANDROID_DIR/app/build/outputs/apk"

    # Check for keystore config
    if [ -z "$ANDROID_KEYSTORE_PATH" ] || [ -z "$ANDROID_KEYSTORE_PASSWORD" ] || [ -z "$ANDROID_KEY_ALIAS" ] || [ -z "$ANDROID_KEY_PASSWORD" ]; then
      echo "  ✗ Missing signing config. Set env vars or use .env.local:"
      echo "    ANDROID_KEYSTORE_PATH"
      echo "    ANDROID_KEYSTORE_PASSWORD"
      echo "    ANDROID_KEY_ALIAS"
      echo "    ANDROID_KEY_PASSWORD"
      exit 1
    fi

    if [ ! -f "$ANDROID_KEYSTORE_PATH" ]; then
      echo "  ✗ Keystore not found at: $ANDROID_KEYSTORE_PATH"
      exit 1
    fi

    UNSIGNED_APK="$APK_DIR/universal/release/app-universal-release-unsigned.apk"
    SIGNED_APK="$APK_DIR/universal/release/Inverter.Desktop_${VERSION}_signed.apk"

    if [ ! -f "$UNSIGNED_APK" ]; then
      echo "  ✗ Unsigned APK not found at: $UNSIGNED_APK"
      exit 1
    fi

    echo "  → Aligning APK..."
    "$ANDROID_HOME/build-tools/34.0.0/zipalign" -v -p 4 "$UNSIGNED_APK" "$SIGNED_APK.aligned"

    echo "  → Signing APK..."
    "$ANDROID_HOME/build-tools/34.0.0/apksigner" sign \
      --ks "$ANDROID_KEYSTORE_PATH" \
      --ks-pass "pass:$ANDROID_KEYSTORE_PASSWORD" \
      --ks-key-alias "$ANDROID_KEY_ALIAS" \
      --key-pass "pass:$ANDROID_KEY_PASSWORD" \
      --out "$SIGNED_APK" \
      "$SIGNED_APK.aligned"

    echo "  → Verifying signature..."
    "$ANDROID_HOME/build-tools/34.0.0/apksigner" verify --verbose "$SIGNED_APK"

    rm -f "$SIGNED_APK.aligned"
    echo "  ✓ APK signed: $SIGNED_APK"
  fi
else
  echo ""
  echo "===> Building Android (debug)..."
  pnpm tauri android build --ci --debug
fi

echo ""
echo "===> Collecting artifacts..."
mkdir -p "$OUTPUT_DIR"

APK_DIR="$ANDROID_DIR/app/build/outputs/apk"
AAB_DIR="$ANDROID_DIR/app/build/outputs/bundle"

if [ "$RELEASE" = true ]; then
  if [ "$SIGN_APK" = true ] && [ -f "$APK_DIR/universal/release/Inverter.Desktop_${VERSION}_signed.apk" ]; then
    cp "$APK_DIR/universal/release/Inverter.Desktop_${VERSION}_signed.apk" "$OUTPUT_DIR/" && echo "  ✓ Signed APK copied"
  else
    cp "$APK_DIR/universal/release/app-universal-release-unsigned.apk" "$OUTPUT_DIR/Inverter.Desktop_${VERSION}-unsigned.apk" 2>/dev/null && echo "  ✓ APK copied" || echo "  ! No APK found"
  fi
  cp "$AAB_DIR/universalRelease/app-universal-release.aab" "$OUTPUT_DIR/Inverter.Desktop_${VERSION}.aab" 2>/dev/null && echo "  ✓ AAB copied" || echo "  ! No AAB found"
else
  cp "$APK_DIR/universal/debug/"*.apk "$OUTPUT_DIR/" 2>/dev/null && echo "  ✓ Debug APK copied" || echo "  ! No APK found"
fi

echo ""
echo "========================================"
echo "  Android build complete!"
echo "  Artifacts: $OUTPUT_DIR/"
ls -1 "$OUTPUT_DIR/"
echo "========================================"
date

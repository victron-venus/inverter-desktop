#!/usr/bin/env bash
#set -x
# gh workflow run Release --repo victron-venus/inverter-desktop --ref main
set -euo pipefail

OUTPUT_DIR="dist/android"
ANDROID_DIR="src-tauri/gen/android"
ANDROID_API=36
BUILD_TOOLS_VERSION=36.0.0
DEFAULT_NDK_VERSION=28.2.13676358

# Signing configuration (can be set via env vars or .env.local)
ANDROID_KEYSTORE_PATH="${ANDROID_KEYSTORE_PATH:-}"
ANDROID_KEYSTORE_PASSWORD="${ANDROID_KEYSTORE_PASSWORD:-}"
ANDROID_KEY_ALIAS="${ANDROID_KEY_ALIAS:-}"
ANDROID_KEY_PASSWORD="${ANDROID_KEY_PASSWORD:-}"
SIGN_APK="${SIGN_APK:-false}"

# Version from Cargo.toml (source of truth for Tauri)
VERSION=$(awk -F'"' '/^version = / { print $2; exit }' src-tauri/Cargo.toml)
if [ -z "$VERSION" ]; then
  echo "  ✗ No application version found in src-tauri/Cargo.toml" >&2
  exit 1
fi

require_artifact() {
  if [ ! -s "$1" ]; then
    echo "  ✗ Build did not produce a nonempty artifact: $1" >&2
    exit 1
  fi
}

usage() {
  echo "Usage: $0 [--clean] [--dev] [--sign] [--update-deps]"
  echo "  --clean    Remove node_modules and build artifacts before starting"
  echo "  --dev      Build debug APK instead of release"
  echo "  --sign     Sign the release APK (requires keystore env vars)"
  echo "  --update-deps  Update JS dependencies (pnpm update) before building"
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
UPDATE_DEPS=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --clean) CLEAN=true; shift ;;
    --dev)   RELEASE=false; shift ;;
    --sign)  SIGN_APK=true; shift ;;
    --update-deps) UPDATE_DEPS=true; shift ;;
    --help)  usage ;;
    *)       usage ;;
  esac
done

echo "╔══════════════════════════════════════════════════════╗"
echo "║  Inverter Dashboard — Android build script           ║"
echo "╠══════════════════════════════════════════════════════╣"
echo "║  What this script does:                              ║"
echo "║  1. Check & auto-install missing prerequisites:      ║"
echo "║     • Java 17+ (brew install --cask temurin@21)      ║"
echo "║     • Android SDK + NDK (sdkmanager)                 ║"
echo "║     • Rust Android targets (rustup)                  ║"
echo "║  2. Install JS dependencies (pnpm install)           ║"
echo "║  3. Init Android project if needed                   ║"
echo "║  4. Build APK (and AAB for release)                  ║"
if [ "$RELEASE" = true ] && [ "$SIGN_APK" = true ]; then
echo "║  5. Sign & align APK                                 ║"
fi
echo "╚══════════════════════════════════════════════════════╝"
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
fi

if [ "$CLEAN" = true ]; then
  echo "===> Cleaning project..."
  rm -rf node_modules dist src-tauri/target
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
  JAVA_VERSION=$(java -version 2>&1)
  echo "  ✓ Java: ${JAVA_VERSION%%$'\n'*}"
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

if [ ! -f "$ANDROID_HOME/platforms/android-$ANDROID_API/android.jar" ] || \
   [ ! -x "$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/zipalign" ] || \
   [ ! -x "$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/apksigner" ]; then
  echo "  → Installing Android SDK platform & build-tools..."
  # Keep sdkmanager's exit status; a closed stdin pipe may terminate yes normally.
  "$SDKMAN" --sdk_root="$ANDROID_HOME" \
    "platforms;android-$ANDROID_API" \
    "build-tools;$BUILD_TOOLS_VERSION" < <(yes) 2>&1 | tail -3
  if [ ! -f "$ANDROID_HOME/platforms/android-$ANDROID_API/android.jar" ] || \
     [ ! -x "$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/zipalign" ] || \
     [ ! -x "$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/apksigner" ]; then
    echo "  ✗ SDK install failed — check '$SDKMAN --list'"
    exit 1
  fi
fi

# ---------- NDK ----------
if [ -z "${NDK_HOME:-}" ]; then
  # Match the Play build instead of choosing an arbitrary installed NDK.
  NDK_HOME="$ANDROID_HOME/ndk/$DEFAULT_NDK_VERSION"
  if [ ! -d "$NDK_HOME" ]; then
    echo "  → Installing Android NDK (ndk;$DEFAULT_NDK_VERSION)..."
    "$SDKMAN" --sdk_root="$ANDROID_HOME" \
      "ndk;$DEFAULT_NDK_VERSION" < <(yes) 2>&1 | tail -3
  fi
fi
if [ ! -d "$NDK_HOME" ]; then
  echo "  ✗ NDK directory not found: $NDK_HOME" >&2
  echo "    Set NDK_HOME to an installed Android NDK directory." >&2
  exit 1
fi
export NDK_HOME

# Set for this session
export ANDROID_HOME
export ANDROID_SDK_ROOT="$ANDROID_HOME"
echo "  ✓ ANDROID_HOME=$ANDROID_HOME"
echo "  ✓ NDK_HOME=$NDK_HOME"

# ---------- Rust targets ----------
INSTALLED_TARGETS=$(rustup target list --installed)
for target in aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android; do
  if ! grep -Fxq "$target" <<< "$INSTALLED_TARGETS"; then
    echo "  → Installing Rust target: $target"
    rustup target add "$target"
  fi
done
echo "  ✓ Rust Android targets installed"

echo ""
echo "===> Installing dependencies..."
pnpm install --frozen-lockfile

echo ""
echo "===> Initializing Android project (if needed)..."
if [ ! -d "$ANDROID_DIR" ]; then
  pnpm tauri android init --ci
else
  echo "  ✓ Android project already exists"
fi

APK_DIR="$ANDROID_DIR/app/build/outputs/apk"
AAB_DIR="$ANDROID_DIR/app/build/outputs/bundle"
UNSIGNED_APK="$APK_DIR/universal/release/app-universal-release-unsigned.apk"
SIGNED_APK="$APK_DIR/universal/release/Inverter.Desktop_${VERSION}_signed.apk"
RELEASE_AAB="$AAB_DIR/universalRelease/app-universal-release.aab"

# Never accept outputs left by an earlier invocation if a build produces nothing.
if [ "$RELEASE" = true ]; then
  rm -f "$UNSIGNED_APK" "$SIGNED_APK" "$SIGNED_APK.aligned" "$RELEASE_AAB"
else
  rm -f "$APK_DIR/universal/debug/"*.apk
fi

if [ "$RELEASE" = true ]; then
  echo ""
  echo "===> Building Android (release)..."
  pnpm tauri android build --ci
  require_artifact "$UNSIGNED_APK"
  require_artifact "$RELEASE_AAB"

  # ---------- Sign APK if requested ----------
  if [ "$SIGN_APK" = true ]; then
    echo ""
    echo "===> Signing APK..."

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

    echo "  → Aligning APK..."
    "$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/zipalign" -v -P 16 4 "$UNSIGNED_APK" "$SIGNED_APK.aligned"

    echo "  → Signing APK..."
    "$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/apksigner" sign \
      --ks "$ANDROID_KEYSTORE_PATH" \
      --ks-pass "pass:$ANDROID_KEYSTORE_PASSWORD" \
      --ks-key-alias "$ANDROID_KEY_ALIAS" \
      --key-pass "pass:$ANDROID_KEY_PASSWORD" \
      --out "$SIGNED_APK" \
      "$SIGNED_APK.aligned"

    echo "  → Verifying signature..."
    "$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/apksigner" verify --verbose "$SIGNED_APK"
    "$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/zipalign" -c -P 16 4 "$SIGNED_APK"
    require_artifact "$SIGNED_APK"

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

if [ "$RELEASE" = true ]; then
  if [ "$SIGN_APK" = true ]; then
    cp "$SIGNED_APK" "$OUTPUT_DIR/"
    echo "  ✓ Signed APK copied"
  else
    cp "$UNSIGNED_APK" "$OUTPUT_DIR/Inverter.Desktop_${VERSION}-unsigned.apk"
    echo "  ✓ APK copied"
  fi
  cp "$RELEASE_AAB" "$OUTPUT_DIR/Inverter.Desktop_${VERSION}.aab"
  echo "  ✓ AAB copied"
else
  shopt -s nullglob
  DEBUG_APKS=("$APK_DIR/universal/debug/"*.apk)
  if [ "${#DEBUG_APKS[@]}" -eq 0 ]; then
    echo "  ✗ Build did not produce a debug APK" >&2
    exit 1
  fi
  for artifact in "${DEBUG_APKS[@]}"; do
    require_artifact "$artifact"
    cp "$artifact" "$OUTPUT_DIR/"
  done
  echo "  ✓ Debug APK copied"
fi

echo ""
echo "========================================"
echo "  Android build complete!"
echo "  Artifacts: $OUTPUT_DIR/"
ls -1 "$OUTPUT_DIR/"
echo "========================================"
date

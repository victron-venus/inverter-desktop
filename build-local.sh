#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Inverter Desktop"
BUNDLE_DIR="src-tauri/target/release/bundle"

UPDATE_DEPS=false
CLEAN=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --update-deps) UPDATE_DEPS=true; shift ;;
    --clean) CLEAN=true; shift ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

if [[ "$CLEAN" == true || "$UPDATE_DEPS" == true ]]; then
  echo "===> Cleaning project..."
  rm -rf node_modules dist src-tauri/target
  echo "  ✓ Cleaned"
fi

echo ""
if [[ "$UPDATE_DEPS" == true ]]; then
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

echo ""
echo "===> Building Tauri application..."
pnpm run tauri build -- --verbose

echo "===> Killing running instances of '${APP_NAME}'..."
pkill -f "${APP_NAME}" 2>/dev/null && echo "  ✓ Killed" || echo "  (not running)"

BUNDLE_ID="com.alvit.inverter-dashboard"
echo ""
echo "===> Clearing WKWebView NetworkCache..."
rm -rf "$HOME/Library/Caches/${BUNDLE_ID}/WebKit/NetworkCache"
echo "  ✓ $HOME/Library/Caches/${BUNDLE_ID}/WebKit/NetworkCache"

echo ""
echo "===> Installing ${APP_NAME} to /Applications..."
APP_BUNDLE="${BUNDLE_DIR}/macos/${APP_NAME}.app"
if [ -d "$APP_BUNDLE" ]; then
  rm -rf "/Applications/${APP_NAME}.app"
  cp -R "$APP_BUNDLE" "/Applications/${APP_NAME}.app"
  echo "  ✓ Installed to /Applications/${APP_NAME}.app"
else
  echo "  ✗ Bundle not found at ${APP_BUNDLE}"
  echo "    DMG available at: ${BUNDLE_DIR}/dmg/"
  exit 1
fi

echo ""
echo "========================================"
echo "  Build complete!"
echo "  App:  /Applications/${APP_NAME}.app"
echo "  DMG:  ${BUNDLE_DIR}/dmg/"
echo "========================================"

open -a "${APP_NAME}"
date

#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Inverter Desktop"
BUNDLE_DIR="src-tauri/target/release/bundle"

echo "===> Cleaning project..."
rm -rf node_modules dist src-tauri/target
echo "  ✓ Cleaned"

echo ""
echo "===> Installing dependencies using pnpm..."
# Using pnpm for consistent lockfile-based installs (matches CI)
pnpm install

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
# Also possible (drops localStorage theme/dismissed, not Application Support/config.json):
# rm -rf "$HOME/Library/WebKit/${BUNDLE_ID}"

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

# Launch the app
open -a "${APP_NAME}"
date

#!/usr/bin/env bash
set -e

echo "===> Installing dependencies..."
pnpm install --frozen-lockfile

echo "===> Building frontend..."
npm run build

echo "===> Building Tauri application..."
pnpm tauri build

echo "===> Build complete!"
echo "Frontend artifacts: dist/"
echo "Tauri installers: src-tauri/target/release/bundle/"

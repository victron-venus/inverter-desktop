#!/usr/bin/env bash
set -e

echo "===> Installing dependencies..."
pnpm install --frozen-lockfile

echo "===> Rust compile check"
cd ./src-tauri/
cargo check
cd ..

echo "===> TypeScript compile check"
npx vue-tsc --noEmit

echo "===> Building frontend..."
npm run build

echo "===> Building Tauri application..."
pnpm tauri build

echo "===> Build complete!"
echo "Frontend artifacts: dist/"
echo "Tauri installers: src-tauri/target/release/bundle/"
open ~/victron/inverter-desktop/src-tauri/target/release/bundle/dmg/

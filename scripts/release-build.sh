#!/usr/bin/env bash
# Build local artifacts without publishing or deploying. Dependencies must be installed.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="${1:?Usage: release-build.sh X.Y.Z [nightly|beta|rc|stable] (requires .release-plan.json)}"
CHANNEL="${2:-rc}"
if [ ! -f .release-plan.json ]; then
  echo "Use the release workflow to reserve a release plan, or pnpm tauri build for a local base-version build." >&2
  exit 1
fi
python3 scripts/check-release-version.py "$VERSION" "$CHANNEL"
python3 scripts/version_plan.py sync --plan .release-plan.json
export RELEASE_VERSION="$VERSION" RELEASE_CHANNEL="$CHANNEL"
mkdir -p release-output
pnpm tauri build --ci
python3 scripts/collect-release-desktop.py

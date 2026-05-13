#!/bin/bash
# Run Scorecard analysis locally without signing
# Use this for local development; GitHub Actions handles signing

set -e

REPO=${1:-"victron-venus/inverter-desktop"}
OUTPUT=${2:-"results.sarif"}

echo "Running Scorecard analysis for $REPO..."
echo "Results will be saved to $OUTPUT"
echo ""

# Run scorecard without signing
scorecard --repo="github.com/$REPO" \
  --format=sarif \
  --out="$OUTPUT" \
  --checks=Binary-Artifacts,CI-Tests,Code-Review,Dangerous-Workflow,Dependency-Review-Tool,Fuzzing,License,Maintained,SAST,Security-Policy,Token-Permissions,Vulnerabilities \
  --show-details || true

if [ -f "$OUTPUT" ]; then
  echo ""
  echo "✓ Analysis complete: $OUTPUT"
  echo "Note: Results are not signed locally. GitHub Actions will sign on push."
else
  echo ""
  echo "✗ Analysis failed"
  exit 1
fi

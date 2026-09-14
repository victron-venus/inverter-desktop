#!/usr/bin/env python3
"""Check base files and require a verified plan for a final stable build."""

# CLI filenames use hyphens to match their shell entry points.
# pylint: disable=invalid-name

import json
from pathlib import Path
import subprocess
import sys

from version_plan import check_base_versions, validate_plan


def check_release_version(root, version, channel):
    """Require consistent base fields and any saved plan's exact source identity."""
    if channel not in {"nightly", "beta", "rc", "stable"}:
        raise ValueError("Unsupported release channel")
    policy = json.loads((root / ".release-policy.json").read_text())
    if channel == "stable" and policy["versioning"]["promotion"] != "final-build":
        raise ValueError("Stable builds require the final-build promotion profile")
    check_base_versions(root, policy, version)
    plan_path = root / ".release-plan.json"
    if plan_path.exists():
        plan = json.loads(plan_path.read_text())
        sha = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True
        ).strip()
        validate_plan(plan, policy, sha)
        if plan["base_version"] != version or plan["channel"] != channel:
            raise ValueError("Release plan does not match requested version/channel")
    elif channel == "stable":
        raise ValueError("Stable final-build requires a verified .release-plan.json")
    print(f"Validated {version} for {channel}")


if __name__ == "__main__":
    check_release_version(Path(__file__).resolve().parents[1], *sys.argv[1:])

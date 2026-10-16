#!/usr/bin/env python3
"""Fail early when a final build cannot reproduce its accepted RC toolchain.

This read-only preflight does not replace the publisher's complete RC payload,
platform inventory, package-version, and toolchain verification.
"""

# CLI filenames use hyphens to match their shell entry points.
# pylint: disable=invalid-name
import argparse
import json
import os
import re
from pathlib import Path

import release_control as rc
import release_versioned as versioned
from release_state import StateGitHub, verify_reservation
from version_plan import validate_plan
from version_receipt import MAX_RECEIPT_BYTES, capture_toolchain


class ReadOnlyGitHub(StateGitHub):
    """Keep this optimization incapable of altering release state."""

    def request(self, path, method="GET", body=None, mode="json"):
        """Only allow the existing client's validated read operations."""
        rc.require(method == "GET" and body is None, "Preflight is read-only")
        return super().request(path, method, body, mode)


def read_asset(gh, inventory, name):
    """Read a bounded release metadata file and verify its current identity."""
    rc.require(name in inventory, "Required RC metadata asset is missing")
    asset = inventory[name]
    rc.require(0 < asset["size"] <= MAX_RECEIPT_BYTES, "RC metadata size is invalid")
    raw = gh.binary(f"releases/assets/{rc.positive(asset['id'], 'asset ID')}")
    rc.require(
        len(raw) == asset["size"] and asset.get("digest") == "sha256:" + rc.digest(raw),
        "RC metadata asset identity changed",
    )
    return raw


# Keep receipt, source and reserved-parent checks together at this read boundary.
# pylint: disable-next=too-many-arguments,too-many-positional-arguments,too-many-locals
def verify_platform(gh, plan, name, current, run_id, tag):
    """Compare one platform with the immutable, reserved RC evidence."""
    rc.require(
        isinstance(tag, str)
        and re.fullmatch("v" + rc.VERSION_PATTERN + r"-rc\.[1-9]\d*", tag, re.ASCII),
        "Final preflight requires an explicit RC tag",
    )
    rc.require(
        re.fullmatch(r"release-inputs-[a-zA-Z0-9._-]+\.json", name),
        "Invalid platform receipt name",
    )
    initial = versioned.published_plan_snapshot(gh, tag, plan["source_sha"], True)
    inventory = {item["name"]: item for item in initial[2]}
    raw = read_asset(gh, inventory, rc.MANIFEST)
    manifest = rc.validate_manifest(raw, gh.repo, tag)
    rc.require(
        manifest["source_sha"] == plan["source_sha"]
        and manifest["version"] == plan["base_version"]
        and manifest.get("version_plan", {}).get("promotion") == "final-build",
        "RC source, base version, or promotion differs from final plan",
    )
    parent = {
        "tag": tag,
        "manifest_sha256": rc.digest(raw),
        "source_sha": manifest["source_sha"],
        "run_id": manifest["run_id"],
    }
    verify_reservation(gh, plan, run_id, parent)
    rc.verify_evidence(gh, manifest, raw)
    source_run = gh.api(f"actions/runs/{manifest['run_id']}")
    rc.require(
        source_run.get("id") == manifest["run_id"]
        and source_run.get("head_sha") == plan["source_sha"]
        and source_run.get("run_attempt") == manifest["run_attempt"]
        and source_run.get("status") == "completed"
        and source_run.get("conclusion") == "success",
        "RC source run no longer matches accepted evidence",
    )
    selected = [item for item in manifest["build_receipts"] if item["name"] == name]
    rc.require(len(selected) == 1, "RC does not declare exactly one platform receipt")
    receipt_raw = read_asset(gh, inventory, name)
    rc.require(rc.digest(receipt_raw) == selected[0]["sha256"], "RC receipt changed")
    receipt = rc.parse_json(receipt_raw, "RC platform receipt")
    rc.require(
        isinstance(receipt, dict)
        and receipt.get("source_sha") == plan["source_sha"]
        and receipt.get("plan_sha256") == manifest["plan_sha256"]
        and isinstance(receipt.get("toolchain"), dict)
        and bool(receipt["toolchain"]),
        "RC platform receipt has invalid source, plan, or toolchain",
    )
    before = receipt["toolchain"]
    if before != current:
        fields = [path for path, _ in versioned.toolchain_changes(before, current)]
        raise rc.ReleaseError(
            "Final toolchain differs from accepted RC before package compilation: "
            + ", ".join(fields[:20])
            + ". No package was accepted. Standard hosted runner image revisions "
            "cannot be selected; investigate image availability before a new RC/final cycle."
        )
    rc.require(
        rc.snapshot_identity(
            versioned.published_plan_snapshot(gh, tag, plan["source_sha"], True)
        )
        == rc.snapshot_identity(initial),
        "RC changed during toolchain preflight",
    )
    return parent


def main():
    """Skip RC-only comparison for candidates; enforce it for final builds."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", required=True)
    args = parser.parse_args()
    policy = json.loads(Path(".release-policy.json").read_text(encoding="utf-8"))
    plan = validate_plan(
        json.loads(Path(".release-plan.json").read_text(encoding="utf-8")),
        policy,
        rc.checked_out_sha(),
    )
    current = capture_toolchain()
    if plan["channel"] != "stable":
        print(
            json.dumps({"status": "candidate_toolchain_recorded", "toolchain": current})
        )
        return
    rc.require(plan["promotion"] == "final-build", "Final-build plan required")
    gh = ReadOnlyGitHub(os.environ["GITHUB_REPOSITORY"])
    info = rc.repository_info(gh)
    rc.require(info.get("private") is False, "Release preflight is public-only")
    parent = verify_platform(
        gh,
        plan,
        args.receipt,
        current,
        rc.positive(os.environ["GITHUB_RUN_ID"], "run ID"),
        versioned.event_inputs().get("rc_tag"),
    )
    print(
        json.dumps(
            {
                "status": "toolchain_preflight_passed",
                "receipt": args.receipt,
                "parent": parent,
            }
        )
    )


if __name__ == "__main__":
    main()

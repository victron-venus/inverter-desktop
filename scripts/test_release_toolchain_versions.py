"""Verify early toolchain rejection without downloading or rebuilding packages."""

import copy
import importlib.util
import json
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "check_release_toolchain", Path(__file__).with_name("check-release-toolchain.py")
)
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


# Separate mutable snapshots let each test corrupt exactly one evidence boundary.
# pylint: disable-next=too-many-instance-attributes
class EarlyToolchainTests(unittest.TestCase):
    """The optimization must preserve the final exact-equality predicate."""

    def setUp(self):
        self.name = "release-inputs-ios.json"
        self.source = "a" * 40
        self.plan = {"source_sha": self.source, "base_version": "2.5.49"}
        self.toolchain = {
            "ImageVersion": "20260907.0351.1",
            "node": "v24.20.0",
            "python": "3.12.15",
            "rustc": "rustc 1.99.1",
            "platform": "darwin",
        }
        self.receipt = {
            "source_sha": self.source,
            "plan_sha256": "b" * 64,
            "toolchain": self.toolchain,
        }
        self.manifest = {
            "source_sha": self.source,
            "version": "2.5.49",
            "run_id": 42,
            "run_attempt": 1,
            "version_plan": {"promotion": "final-build"},
            "plan_sha256": "b" * 64,
            "build_receipts": [],
        }
        self.run = {
            "id": 42,
            "head_sha": self.source,
            "run_attempt": 1,
            "status": "completed",
            "conclusion": "success",
        }
        self.payloads = {}
        self.assets = []
        self.rebuild()
        self.snapshot = ({"ref": "refs/tags/v2.5.49-rc.2"}, {"id": 7}, self.assets)
        self.gh = type(
            "FakeGitHub",
            (),
            {
                "repo": "victron-venus/inverter-desktop",
                "binary": lambda _, path: self.payloads[path],
                "api": lambda _, path: self.run,
            },
        )()
        self.snap = self.enterContext(
            patch.object(
                CHECK.versioned, "published_plan_snapshot", return_value=self.snapshot
            )
        )
        self.enterContext(
            patch.object(
                CHECK.rc,
                "validate_manifest",
                side_effect=lambda raw, *_: json.loads(raw),
            )
        )
        self.reservation = self.enterContext(patch.object(CHECK, "verify_reservation"))
        self.evidence = self.enterContext(patch.object(CHECK.rc, "verify_evidence"))

    def rebuild(self):
        """Bind fixture metadata and bytes after a deliberate mutation."""
        raw = json.dumps(self.receipt).encode()
        self.manifest["build_receipts"] = [
            {"name": self.name, "sha256": CHECK.rc.digest(raw)}
        ]
        encoded = json.dumps(self.manifest).encode()
        self.payloads.update({"releases/assets/1": encoded, "releases/assets/2": raw})
        self.assets[:] = [
            {
                "name": name,
                "id": index,
                "size": len(value),
                "digest": "sha256:" + CHECK.rc.digest(value),
            }
            for index, name, value in [
                (1, CHECK.rc.MANIFEST, encoded),
                (2, self.name, raw),
            ]
        ]

    def verify(self, current=None, name=None, tag="v2.5.49-rc.2"):
        """Run the preflight with local payloads and no network operations."""
        return CHECK.verify_platform(
            self.gh,
            self.plan,
            name or self.name,
            self.toolchain if current is None else current,
            99,
            tag,
        )

    def test_exact_toolchain_passes_after_immutable_evidence_and_reservation(self):
        """Require both immutable evidence and the reserved final parent."""
        parent = self.verify()
        self.evidence.assert_called_once()
        self.reservation.assert_called_once_with(self.gh, self.plan, 99, parent)
        self.assertEqual(self.snap.call_count, 2)

    def test_actual_rollout_and_each_added_removed_or_changed_field_rejected(self):
        """Reject the observed rollout and every kind of dictionary drift."""
        changes = [
            {**self.toolchain, "ImageVersion": "20261013.0424.1", "node": "v24.21.0"},
            {**self.toolchain, "node": "v24.21.0"},
            {**self.toolchain, "future": "compiler"},
            {key: value for key, value in self.toolchain.items() if key != "python"},
            {**self.toolchain, "rustc": ["rustc 1.99.1"]},
        ]
        for current in changes:
            with (
                self.subTest(current=current),
                self.assertRaisesRegex(
                    CHECK.rc.ReleaseError, "before package compilation"
                ),
            ):
                self.verify(current)

    def test_tampered_asset_bytes_or_metadata_rejected(self):
        """Reject corrupt, oversized or absent metadata payloads."""
        for mutation in ("bytes", "size", "digest", "missing"):
            with self.subTest(mutation=mutation):
                self.rebuild()
                if mutation == "bytes":
                    self.payloads["releases/assets/2"] += b" "
                if mutation == "size":
                    self.assets[1]["size"] = 2_000_001
                if mutation == "digest":
                    self.assets[1]["digest"] = "sha256:" + "0" * 64
                if mutation == "missing":
                    self.assets.pop()
                with self.assertRaises(CHECK.rc.ReleaseError):
                    self.verify()

    def test_receipt_source_plan_and_empty_toolchain_rejected(self):
        """Bind the platform receipt to its exact source and version plan."""
        original = copy.deepcopy(self.receipt)
        for key, value in (
            ("source_sha", "c" * 40),
            ("plan_sha256", "d" * 64),
            ("toolchain", {}),
        ):
            with self.subTest(key=key):
                self.receipt = {**original, key: value}
                self.rebuild()
                with self.assertRaises(CHECK.rc.ReleaseError):
                    self.verify()

    def test_stale_or_failed_source_run_rejected(self):
        """Reject reruns, wrong commits and non-successful source runs."""
        original = self.run.copy()
        for key, value in (
            ("id", 43),
            ("head_sha", "c" * 40),
            ("run_attempt", 2),
            ("status", "in_progress"),
            ("conclusion", "failure"),
        ):
            with self.subTest(key=key):
                self.run = {**original, key: value}
                with self.assertRaises(CHECK.rc.ReleaseError):
                    self.verify()

    def test_changed_release_during_preflight_rejected(self):
        """Detect a release asset replacement during the comparison."""
        changed = copy.deepcopy(self.snapshot)
        changed[2][1]["id"] = 777
        self.snap.side_effect = [self.snapshot, changed]
        with self.assertRaisesRegex(CHECK.rc.ReleaseError, "changed during"):
            self.verify()

    def test_invalid_names_and_tags_rejected(self):
        """Limit the lookup to a platform receipt and an explicit RC tag."""
        for name in ("../receipt.json", "fuzz-report.json"):
            with self.subTest(name=name), self.assertRaises(CHECK.rc.ReleaseError):
                self.verify(name=name)
        for tag in ("main", "v2.5.49", "v2.5.49-rc.0", None):
            with self.subTest(tag=tag), self.assertRaises(CHECK.rc.ReleaseError):
                self.verify(tag=tag)

    def test_missing_immutable_evidence_or_reservation_is_fatal(self):
        """Do not proceed when either independent evidence check fails."""
        for mock in (self.evidence, self.reservation):
            with self.subTest(mock=mock):
                mock.side_effect = CHECK.rc.ReleaseError("missing evidence")
                with self.assertRaises(CHECK.rc.ReleaseError):
                    self.verify()
                mock.side_effect = None

    def test_transport_rejects_mutations(self):
        """The preflight client cannot call a mutating HTTP method."""
        gh = CHECK.ReadOnlyGitHub("victron-venus/inverter-desktop")
        for method in ("POST", "PUT", "DELETE"):
            with (
                self.subTest(method=method),
                self.assertRaisesRegex(CHECK.rc.ReleaseError, "read-only"),
            ):
                gh.request("releases", method, {})


if __name__ == "__main__":
    unittest.main()

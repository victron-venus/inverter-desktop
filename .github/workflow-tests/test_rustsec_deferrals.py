"""Prove that the temporary retirement exceptions cannot hide new risk."""

import importlib.util
from datetime import datetime, timedelta, timezone
from pathlib import Path
import shutil
import tempfile
import unittest

import yaml  # pylint: disable=import-error  # Installed by the workflow-contracts requirements.


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "rustsec_deferrals", ROOT / "scripts/check_rustsec_deferrals.py"
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("Cannot load RustSec deferral guard")
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)
NOW = datetime(2026, 10, 18, tzinfo=timezone.utc)


class RustSecDeferralTests(unittest.TestCase):
    """Use isolated lockfile/config copies and small RustSec-format records."""
    # Test names describe each assertion; helper methods have docstrings.
    # pylint: disable=missing-function-docstring

    def setUp(self):
        # unittest owns this context until the test and all its assertions finish.
        # pylint: disable=consider-using-with
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        (self.root / "src-tauri").mkdir()
        for name in ("Cargo.lock", "cargo-deny.toml", "deny.toml"):
            shutil.copyfile(ROOT / "src-tauri" / name, self.root / "src-tauri" / name)
        self.database = self.root / "databases/rustsec"
        for advisory_id, package in GUARD.DEFERRALS.items():
            path = self.database / "crates" / package / f"{advisory_id}.md"
            path.parent.mkdir(parents=True)
            path.write_text(
                f'```toml\n[advisory]\nid = "{advisory_id}"\n'
                f'package = "{package}"\ninformational = "unmaintained"\n'
                '[versions]\npatched = []\n```\n\n# Retirement notice\n'
            )

    def validate(self, now=NOW):
        """Run the real guard without network or the user's Cargo database."""
        GUARD.validate_deferrals(self.root, self.database, now)

    def rewrite(self, path, before, after):
        """Make a fixture mutation and assert its target exists."""
        original = path.read_text()
        self.assertIn(before, original)
        path.write_text(original.replace(before, after, 1))
        self.addCleanup(path.write_text, original)

    def test_current_reviewed_dependency_graph_passes(self):
        self.validate()

    def test_exact_utc_expiry_is_enforced(self):
        self.validate(GUARD.EXPIRES - timedelta(microseconds=1))
        for now in (GUARD.EXPIRES, GUARD.EXPIRES + timedelta(days=1)):
            with self.subTest(now=now), self.assertRaisesRegex(ValueError, "expired"):
                self.validate(now)

    def test_every_advisory_reclassification_fails(self):
        for advisory_id, package in GUARD.DEFERRALS.items():
            path = self.database / "crates" / package / f"{advisory_id}.md"
            original = path.read_text()
            for replacement in (
                'informational = "unsound"',
                'cvss = "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"',
                'informational = "unmaintained"\ncategories = ["memory-corruption"]',
                'informational = "unmaintained"\naliases = ["CVE-2026-12345"]',
                'informational = "unmaintained"\nwithdrawn = "2026-10-19"',
            ):
                path.write_text(original.replace('informational = "unmaintained"', replacement))
                with self.subTest(advisory=advisory_id, replacement=replacement):
                    with self.assertRaisesRegex(ValueError, "classification"):
                        self.validate()
            path.write_text(original)

    def test_available_upstream_remediation_requires_review(self):
        path = self.database / "crates/zvariant/RUSTSEC-2026-0370.md"
        original = path.read_text()
        for versions in ('patched = [">=5.12.1"]', 'patched = []\nunaffected = ["<5.0.0"]'):
            path.write_text(original.replace("patched = []", versions))
            with self.subTest(versions=versions), self.assertRaisesRegex(ValueError, "remediation"):
                self.validate()

    def test_wrong_or_missing_advisory_fails(self):
        path = self.database / "crates/zvariant/RUSTSEC-2026-0370.md"
        self.rewrite(path, 'package = "zvariant"', 'package = "unrelated"')
        with self.assertRaisesRegex(ValueError, "classification"):
            self.validate()
        path.unlink()
        with self.assertRaises(FileNotFoundError):
            self.validate()

    def test_dependency_version_change_requires_review(self):
        path = self.root / "src-tauri/Cargo.lock"
        for name, version in GUARD.LOCKED_VERSIONS.items():
            original = path.read_text()
            self.rewrite(
                path, f'name = "{name}"\nversion = "{version}"',
                f'name = "{name}"\nversion = "99.0.0"',
            )
            with self.subTest(package=name):
                with self.assertRaisesRegex(ValueError, "dependency change"):
                    self.validate()
            path.write_text(original)

    def test_additional_vulnerable_version_requires_review(self):
        path = self.root / "src-tauri/Cargo.lock"
        with path.open("a") as output:
            output.write('\n[[package]]\nname = "zvariant"\nversion = "4.0.0"\n')
        with self.assertRaisesRegex(ValueError, "dependency change"):
            self.validate()

    def test_dependency_source_change_requires_review(self):
        path = self.root / "src-tauri/Cargo.lock"
        self.rewrite(
            path,
            f'name = "zvariant"\nversion = "5.12.0"\nsource = "{GUARD.REGISTRY}"',
            'name = "zvariant"\nversion = "5.12.0"\nsource = "git+https://example.invalid"',
        )
        with self.assertRaisesRegex(ValueError, "dependency change"):
            self.validate()

    def test_broadened_or_missing_config_exception_fails(self):
        for name in ("cargo-deny.toml", "deny.toml"):
            path = self.root / "src-tauri" / name
            original = path.read_text()
            for replacement in ('"RUSTSEC-2099-0001",', ''):
                path.write_text(original.replace('"RUSTSEC-2026-0370",', replacement))
                with self.subTest(config=name, replacement=replacement):
                    with self.assertRaisesRegex(ValueError, "Unexpected advisory"):
                        self.validate()
            path.write_text(original)

    def test_database_selection_is_unambiguous(self):
        root = self.database.parent
        self.assertEqual(GUARD.find_database(root), self.database)
        shutil.copytree(self.database, root / "other")
        with self.assertRaisesRegex(ValueError, "exactly one"):
            GUARD.find_database(root)

    def test_malformed_advisory_header_fails(self):
        for source in ("# no metadata", "```toml\n[advisory]\n"):
            with self.subTest(source=source), self.assertRaises(ValueError):
                GUARD.advisory_metadata(source)

    def test_ci_uses_fresh_guarded_snapshot_for_all_checks(self):
        workflow = yaml.safe_load((ROOT / ".github/workflows/cargo-deny.yml").read_text())
        steps = workflow["jobs"]["cargo-deny"]["steps"]
        check = next(step for step in steps if step.get("name") == "Run cargo deny check")
        self.assertNotIn("if", check)
        self.assertNotIn("continue-on-error", check)
        self.assertEqual(check["run"].strip().splitlines(), [
            "cargo fetch --locked",
            "cargo deny --config cargo-deny.toml fetch all",
            "python3 ../scripts/check_rustsec_deferrals.py",
            "cargo deny --offline --locked --config cargo-deny.toml "
            "check advisories bans licenses sources",
        ])
        report = next(step for step in steps if step.get("name") == "Generate SARIF report")
        self.assertIn("cargo deny --offline --locked", report["run"])


if __name__ == "__main__":
    unittest.main()

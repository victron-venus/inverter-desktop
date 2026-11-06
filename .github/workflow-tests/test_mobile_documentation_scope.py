"""Keep iOS archives out of docs PRs while retaining Android source analysis."""

import importlib.util
import unittest
from pathlib import Path
from unittest.mock import patch

from test_metadata_events import definition, evaluate

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "mobile_change_scope", ROOT / "scripts/change_scope.py"
)
SCOPE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SCOPE)


def raw_change(path):
    """A complete regular-file Git raw diff record, without external Git state."""
    return (
        b":100644 100644 "
        + b"1" * 40
        + b" "
        + b"2" * 40
        + b" M\0"
        + path.encode()
        + b"\0"
    )


class MobileDocumentationScopeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.quality = definition(".github/workflows/quality-gate.yml")
        cls.mobile = definition(".github/workflows/mobile.yml")

    def requested(self, event, run, reason):
        condition = self.quality["jobs"]["check-2"]["with"]["run-ios-build"]
        # Reuse the existing restricted expression interpreter for scope outputs.
        condition = condition.replace("needs.scope.outputs.", "inputs.")
        return bool(
            evaluate(
                condition,
                {
                    "github": {"event_name": event},
                    "inputs": {"run": run, "reason": reason},
                },
            )
        )

    def ios_runs(self, event, requested):
        return bool(
            evaluate(
                self.mobile["jobs"]["ios"]["if"],
                {
                    "github": {"event_name": event},
                    "inputs": {"run-ios-build": requested},
                },
            )
        )

    def test_proven_documentation_pr_and_merge_group_skip_ios(self):
        docs = SCOPE.classify_diff(raw_change("docs/release-workflow.md"), set(), set())
        self.assertEqual(docs, {"run": False, "reason": "documentation-only"})
        for event in ("pull_request", "merge_group"):
            with self.subTest(event=event):
                requested = self.requested(
                    event, str(docs["run"]).lower(), docs["reason"]
                )
                self.assertFalse(requested)
                self.assertFalse(self.ios_runs(event, requested))

    def test_source_forced_and_classifier_errors_keep_ios(self):
        event = {"pull_request": {"base": {"sha": "1" * 40}, "head": {"sha": "2" * 40}}}
        with patch.object(SCOPE, "git", side_effect=OSError("unavailable")):
            error = SCOPE.classify(ROOT, "pull_request", event)
        decisions = [
            SCOPE.classify_diff(raw_change("src/main.ts"), set(), set()),
            SCOPE.classify_diff(b"", set(), set()),
            SCOPE.classify(ROOT, "pull_request", None, force=True),
            SCOPE.classify(ROOT, "pull_request", None),
            SCOPE.classify(ROOT, "workflow_dispatch", None),
            error,
        ]
        for decision in decisions:
            with self.subTest(decision=decision):
                self.assertTrue(decision["run"])
                requested = self.requested("pull_request", "true", decision["reason"])
                self.assertTrue(requested)
                self.assertTrue(self.ios_runs("pull_request", requested))

    def test_unproven_or_missing_scope_outputs_keep_ios(self):
        for run, reason in (
            (None, None),
            ("", ""),
            ("false", ""),
            ("false", "unknown"),
            ("unknown", "documentation-only"),
        ):
            with self.subTest(run=run, reason=reason):
                self.assertTrue(self.requested("pull_request", run, reason))

    def test_default_manual_and_release_qualification_remain_full(self):
        field = self.mobile["on"]["workflow_call"]["inputs"]["run-ios-build"]
        self.assertEqual((field["type"], field["default"]), ("boolean", "true"))
        self.assertTrue(self.ios_runs("workflow_call", field["default"] == "true"))
        self.assertTrue(self.ios_runs("workflow_dispatch", False))
        release = definition(".github/workflows/release-pipeline.yml")
        self.assertEqual(release["jobs"]["checks"]["with"]["force-full"], "true")
        scope = next(
            step
            for step in release["jobs"]["scope"]["steps"]
            if step.get("id") == "scope"
        )
        self.assertEqual(scope["env"]["FORCE_FULL"], "true")
        for event in ("push", "schedule", "workflow_dispatch"):
            decision = SCOPE.classify(
                ROOT, event, None, force=scope["env"]["FORCE_FULL"] == "true"
            )
            with self.subTest(event=event):
                self.assertTrue(decision["run"])
                requested = self.requested(event, "true", decision["reason"])
                self.assertTrue(self.ios_runs(event, requested))
        self.assertIn("scope", self.quality["jobs"]["check-2"]["needs"])
        self.assertNotIn("if", self.quality["jobs"]["check-2"])

    def test_android_manual_codeql_and_real_build_remain_required(self):
        android = self.mobile["jobs"]["android"]
        self.assertNotIn("if", android)
        self.assertEqual(android["permissions"]["security-events"], "write")
        steps = android["steps"]
        init = next(
            step
            for step in steps
            if step.get("name") == "Initialize Android source analysis"
        )
        build = next(step for step in steps if step.get("name") == "Build Android")
        analyze = next(
            step for step in steps if step.get("name") == "Analyze Android source"
        )
        self.assertEqual(
            init["with"], {"languages": "java-kotlin", "build-mode": "manual"}
        )
        self.assertIn("pnpm tauri android build --ci --apk", build["run"])
        self.assertEqual(analyze["with"]["category"], "/language:java-kotlin")
        self.assertLess(steps.index(init), steps.index(build))
        self.assertLess(steps.index(build), steps.index(analyze))
        for step in (init, build, analyze):
            self.assertNotIn("if", step)


if __name__ == "__main__":
    unittest.main()

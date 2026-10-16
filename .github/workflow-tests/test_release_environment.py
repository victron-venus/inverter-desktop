"""Keep release environment pins and early preflight wired into both adapters."""

import unittest
from pathlib import Path

import yaml


class ReleaseEnvironmentWorkflowTests(unittest.TestCase):
    """Check actual adapter wiring, not a separate workflow fixture."""

    def test_workflow_pins_and_checks_before_compilation(self):
        """Set up fixed tools before comparing and compare before package builds."""
        path = (
            Path(__file__).resolve().parents[2] / ".github/workflows/release-build.yml"
        )
        workflow = yaml.safe_load(path.read_text(encoding="utf-8"))
        self.assertNotIn("actions", workflow["permissions"])
        for job in workflow["jobs"].values():
            self.assertEqual(
                job["permissions"], {"contents": "read", "actions": "read"}
            )
            steps = job["steps"]
            configurations = {
                step.get("uses", "").split("@")[0]: step.get("with", {})
                for step in steps
                if "uses" in step
            }
            self.assertEqual(
                configurations["actions/setup-node"]["node-version"], "24.21.0"
            )
            self.assertEqual(
                configurations["actions/setup-python"]["python-version"],
                "${{ runner.os == 'Linux' && '3.12.15' || '3.12.10' }}",
            )
            self.assertEqual(configurations["pnpm/action-setup"]["version"], "10.34.7")
            self.assertEqual(
                configurations["dtolnay/rust-toolchain"]["toolchain"], "1.99.1"
            )
            preflight = next(
                i
                for i, step in enumerate(steps)
                if "check-release-toolchain.py" in step.get("run", "")
            )
            install = next(
                i
                for i, step in enumerate(steps)
                if step.get("run", "").startswith("pnpm install")
            )
            self.assertLess(preflight, install)
            for index, step in enumerate(steps):
                action = step.get("uses", "").split("@")[0]
                if action in {
                    "actions/setup-node",
                    "actions/setup-python",
                    "pnpm/action-setup",
                    "dtolnay/rust-toolchain",
                }:
                    self.assertLess(index, preflight)
                if any(
                    command in step.get("run", "")
                    for command in (
                        "pnpm tauri",
                        "build-release-plugins.py",
                        "ci-build-ios-library.sh",
                    )
                ):
                    self.assertLess(preflight, index)
            self.assertEqual(steps[preflight]["env"]["GH_TOKEN"], "${{ github.token }}")


if __name__ == "__main__":
    unittest.main()

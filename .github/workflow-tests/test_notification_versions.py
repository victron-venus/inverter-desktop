"""Keep both sides of the Tauri notification bridge compatible."""

from pathlib import Path
import tomllib
import unittest

import yaml  # pylint: disable=import-error


ROOT = Path(__file__).resolve().parents[2]


class NotificationVersionTests(unittest.TestCase):
    """Catch split dependency updates before the expensive Android build."""

    def test_locked_notification_major_minor_versions_match(self):
        """Tauri requires matching major/minor Rust and JavaScript plugins."""
        cargo = tomllib.loads((ROOT / "src-tauri/Cargo.lock").read_text())
        rust = [entry["version"] for entry in cargo["package"]
                if entry["name"] == "tauri-plugin-notification"]
        self.assertEqual(len(rust), 1)
        pnpm = yaml.safe_load((ROOT / "pnpm-lock.yaml").read_text())
        javascript = pnpm["importers"]["."]["dependencies"][
            "@tauri-apps/plugin-notification"]["version"]
        self.assertEqual(
            javascript.split(".")[:2], rust[0].split(".")[:2],
            "Update the Rust and JavaScript notification plugins together.",
        )


if __name__ == "__main__":
    unittest.main()

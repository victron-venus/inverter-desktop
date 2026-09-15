"""Built-in HA metadata and both publisher staging entry points."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_frigate_package import executable_fixture, packaging

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts/plugins"


class HomeAssistantPackageTests(unittest.TestCase):
    """Staging selects fixed metadata and never expands the signed payload."""

    def setUp(self):
        # TestCase owns cleanup even if staging or an assertion fails.
        # pylint: disable-next=consider-using-with
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.worker = self.root / "worker"
        self.worker.write_bytes(executable_fixture("aarch64-apple-darwin"))
        self.output = self.root / "package"

    def command(self, script, *arguments):
        """Run only the packaging Python CLI; the fixture is never executable."""
        return subprocess.run(
            [sys.executable, str(SCRIPTS / script), *map(str, arguments)],
            check=False, capture_output=True, text=True, timeout=10,
        )

    def test_ha_payload_and_opt_in_action_metadata_match_every_desktop_target(self):
        """The same fixed HA identity supports all matching native headers."""
        for target in packaging.TARGETS:
            with self.subTest(target=target):
                self.worker.write_bytes(executable_fixture(target))
                output = self.root / target
                packaging.prepare(self.worker, target, output, "home-assistant")
                manifest = json.loads((output / "manifest.json").read_text())
                binary = "inverter-home-assistant-worker"
                if "windows" in target:
                    binary += ".exe"
                self.assertEqual(manifest["plugin_id"], "inverter-desktop.home-assistant")
                self.assertEqual(manifest["version"], "0.7.0")
                self.assertEqual(manifest["host_api"], "^1.5")
                self.assertEqual(manifest["target"], target)
                self.assertEqual(manifest["entrypoint"], f"bin/{binary}")
                self.assertEqual(set(manifest["permissions"]), {
                    "plugin_configuration", "dashboard_contributions", "network_http",
                })
                self.assertNotIn("http_video", manifest)
                self.assertIsNone(manifest["signature"])
                self.assertEqual(manifest["inventory"], [])
                files = sorted(path.relative_to(output).as_posix()
                               for path in output.rglob("*") if path.is_file())
                self.assertEqual(files, ["manifest.json", f"payload/bin/{binary}"])
                self.assertEqual((output / "payload/bin" / binary).read_bytes(),
                                 self.worker.read_bytes())

    def test_settings_are_flat_and_status_only_needs_no_entity_watchlist(self):
        """The native editor can render fields without exposing a token default."""
        packaging.prepare(self.worker, "aarch64-apple-darwin", self.output, "home-assistant")
        schema = json.loads((self.output / "manifest.json").read_text())["config_schema"]
        self.assertEqual(set(schema["required"]), {"ha_base_url", "ha_token"})
        self.assertFalse(schema["additionalProperties"])
        fields = schema["properties"]
        self.assertEqual(set(fields), {
            "ha_base_url", "watch_entities", "action_entities", "media_player_entities",
            "binary_entities", "cover_entities", "number_entities",
            "cover_position_entities", "discovery_prefixes", "ha_token",
        })
        self.assertTrue(all(field["type"] == "string" for field in fields.values()))
        self.assertEqual(fields["ha_base_url"]["maxLength"], 2048)
        self.assertEqual(fields["watch_entities"]["default"], "")
        self.assertEqual(fields["action_entities"]["default"], "")
        self.assertEqual(fields["action_entities"]["maxLength"], 4096)
        self.assertNotIn("action_entities", schema["required"])
        self.assertEqual(fields["media_player_entities"]["default"], "")
        self.assertEqual(fields["media_player_entities"]["maxLength"], 4096)
        self.assertNotIn("media_player_entities", schema["required"])
        self.assertEqual(fields["binary_entities"]["default"], "")
        self.assertEqual(fields["binary_entities"]["maxLength"], 4096)
        self.assertNotIn("binary_entities", schema["required"])
        self.assertEqual(fields["cover_entities"]["default"], "")
        self.assertEqual(fields["cover_entities"]["maxLength"], 4096)
        self.assertNotIn("cover_entities", schema["required"])
        for key, limit in (("number_entities", 4096), ("cover_position_entities", 4096),
                           ("discovery_prefixes", 1024)):
            self.assertEqual(fields[key]["default"], "")
            self.assertIs(fields[key]["omitEmpty"], True)
            self.assertEqual(fields[key]["maxLength"], limit)
            self.assertNotIn(key, schema["required"])
        self.assertEqual({key for key, field in fields.items() if field.get("omitEmpty")}, {
            "number_entities", "cover_position_entities", "discovery_prefixes",
        })
        self.assertNotIn("writeOnly", fields["discovery_prefixes"])
        self.assertNotIn("minLength", fields["watch_entities"])
        self.assertTrue(fields["ha_token"]["writeOnly"])
        self.assertEqual(fields["ha_token"]["minLength"], 1)
        self.assertEqual(fields["ha_token"]["maxLength"], 4096)
        self.assertNotIn("default", fields["ha_token"])

    def test_plugin_choice_is_validated_before_reading_or_creating_output(self):
        """A CLI selection is not a manifest path or an arbitrary payload name."""
        with patch.object(packaging, "read_worker") as read:
            for plugin in ("../home-assistant", "unknown", "", "frigate-manifest.json"):
                with self.subTest(plugin=plugin), self.assertRaisesRegex(ValueError, "built-in"):
                    packaging.prepare(self.worker, "aarch64-apple-darwin", self.output, plugin)
            read.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_ha_staging_keeps_architecture_and_existing_output_protection(self):
        """The new package uses the existing guarded binary reader and output rules."""
        with self.assertRaisesRegex(ValueError, "match"):
            packaging.prepare(self.worker, "x86_64-pc-windows-msvc", self.output,
                              "home-assistant")
        self.assertFalse(self.output.exists())
        self.output.mkdir()
        preserved = self.output / "keep"
        preserved.write_text("keep")
        with self.assertRaises(FileExistsError):
            packaging.prepare(self.worker, "aarch64-apple-darwin", self.output,
                              "home-assistant")
        self.assertEqual(preserved.read_text(), "keep")

    def test_staging_is_identical_for_the_same_bytes_and_fixed_metadata(self):
        """Source timestamps and nearby files do not change a package's staged bytes."""
        packaging.prepare(self.worker, "aarch64-apple-darwin", self.output, "home-assistant")
        other_worker = self.root / "copy"
        other_worker.write_bytes(self.worker.read_bytes())
        (self.root / "unrelated.txt").write_text("not part of the payload")
        other_output = self.root / "other"
        packaging.prepare(other_worker, "aarch64-apple-darwin", other_output, "home-assistant")
        for path in self.output.rglob("*"):
            if path.is_file():
                self.assertEqual(path.read_bytes(),
                                 (other_output / path.relative_to(self.output)).read_bytes())

    def test_generic_cli_and_frigate_compatibility_entry_stage_identical_files(self):
        """Existing Frigate invocations retain their metadata and payload layout."""
        generic = self.root / "generic"
        for script, output, extra in (
            ("prepare-frigate-package.py", self.output, ()),
            ("prepare-plugin-package.py", generic, ("--plugin", "frigate")),
        ):
            result = self.command(script, *extra, "--worker", self.worker,
                                  "--target", "aarch64-apple-darwin", "--output", output)
            self.assertEqual(result.returncode, 0, result.stderr)
        for relative in ("manifest.json", "payload/bin/inverter-frigate-worker"):
            self.assertEqual((self.output / relative).read_bytes(),
                             (generic / relative).read_bytes())

    def test_generic_cli_stages_ha_and_help_requires_no_file_access(self):
        """The new entry point works as a real command and offers standalone help."""
        for script in ("prepare-plugin-package.py", "prepare-frigate-package.py"):
            result = self.command(script, "--help")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("--worker", result.stdout)
        result = self.command("prepare-plugin-package.py", "--plugin", "home-assistant",
                              "--worker", self.worker, "--target", "aarch64-apple-darwin",
                              "--output", self.output)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((self.output / "manifest.json").read_text())
        self.assertEqual(manifest["plugin_id"], "inverter-desktop.home-assistant")

    def test_cli_rejects_external_metadata_and_unknown_plugin(self):
        """Only fixed publisher metadata may be selected through the CLI."""
        for extra in (("--plugin", "../home-assistant"),
                      ("--plugin", "home-assistant", "--manifest", str(self.worker))):
            result = self.command("prepare-plugin-package.py", *extra,
                                  "--worker", self.worker, "--target", "aarch64-apple-darwin",
                                  "--output", self.output)
            self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()

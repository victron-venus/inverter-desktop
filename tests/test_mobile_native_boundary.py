"""Regression tests for the gate that inspects shipped mobile native code."""

import importlib.util
import plistlib
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import Mock, patch

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/check-mobile-native-boundary.py"
SPEC = importlib.util.spec_from_file_location("mobile_native_boundary", SCRIPT)
boundary = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(boundary)
CORE = (
    b"native-code\x00perform_action\x00connect_mqtt\x00get_setpoint_override"
    b"\x00set_setpoint_override\x00ha_url\x00camera_enabled"
)


def package_fixture(path, entries):
    """Create a ZIP package fixture with exactly the supplied entries."""
    with zipfile.ZipFile(path, "w") as archive:
        for filename, payload in entries.items():
            archive.writestr(filename, payload)
    return path


class MobileNativeBoundaryTests(unittest.TestCase):
    """Exercise rejection and acceptance using disposable native package fixtures."""

    def setUp(self):
        # TestCase owns and exits this context after each test, including failures.
        # pylint: disable-next=consider-using-with
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def archive(self, name, entries):
        """Create a ZIP package fixture with exactly the supplied entries."""
        return package_fixture(self.root / name, entries)

    def test_passive_legacy_settings_are_allowed(self):
        """Retained configuration keys do not imply an active integration."""
        boundary.verify_native_payload(CORE, "mobile")

    def test_registered_feature_commands_are_rejected(self):
        """Every desktop command is forbidden even when core commands exist."""
        for command in boundary.FORBIDDEN_COMMANDS:
            payload = CORE + command.encode()
            with (
                self.subTest(command=command),
                self.assertRaisesRegex(ValueError, command),
            ):
                boundary.verify_native_payload(payload, "mobile")

    def test_missing_core_handler_is_rejected(self):
        """Core controls must remain registered when desktop handlers are removed."""
        for command in (
            "perform_action",
            "connect_mqtt",
            "get_setpoint_override",
            "set_setpoint_override",
        ):
            payload = CORE.replace(command.encode(), b"missing_handler")
            with (
                self.subTest(command=command),
                self.assertRaisesRegex(ValueError, "Cannot identify"),
            ):
                boundary.verify_native_payload(payload, "mobile")

    def test_cargo_rejects_option_like_and_nonmobile_targets(self):
        """Invalid target arguments must fail before any Cargo process starts."""
        manifest = boundary.CHECKOUT / "src-tauri/Cargo.toml"
        with patch.object(boundary.subprocess, "run") as run:
            for target in (
                "--config=/tmp/injected.toml",
                "aarch64-linux-android --config=/tmp/injected.toml",
                "x86_64-unknown-linux-gnu",
            ):
                with (
                    self.subTest(target=target),
                    self.assertRaisesRegex(ValueError, "Unsupported mobile target"),
                ):
                    boundary.cargo_tree(manifest, target)
            run.assert_not_called()

    def test_cargo_rejects_manifest_outside_this_checkout(self):
        """Neither inspection command may select an arbitrary Cargo project."""
        external = self.root / "Cargo.toml"
        external.write_text('[package]\nname = "external"\nversion = "0.1.0"\n')
        with patch.object(boundary.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "this checkout"):
                boundary.cargo_tree(external, "aarch64-apple-ios")
            with self.assertRaisesRegex(ValueError, "this checkout"):
                boundary.cargo_metadata(external)
            run.assert_not_called()

    def test_cargo_receives_canonical_manifest_and_allowlisted_target(self):
        """Valid arguments retain fixed offline inspection commands and paths."""
        manifest = boundary.CHECKOUT / "src-tauri/../src-tauri/Cargo.toml"
        expected = str((boundary.CHECKOUT / "src-tauri/Cargo.toml").resolve())
        with patch.object(
            boundary.subprocess, "run", return_value=Mock(stdout="{}")
        ) as run:
            boundary.cargo_tree(manifest, "aarch64-apple-ios-sim")
            tree_arguments = run.call_args.args[0]
            self.assertEqual(
                tree_arguments[tree_arguments.index("--target") + 1],
                "aarch64-apple-ios-sim",
            )
            self.assertEqual(tree_arguments[-1], expected)
            self.assertIn("--offline", tree_arguments)
            self.assertEqual(run.call_args.kwargs["cwd"], boundary.CHECKOUT)
            self.assertFalse(run.call_args.kwargs["shell"])
            boundary.cargo_metadata(manifest)
            self.assertEqual(run.call_args.args[0][-1], expected)

    def test_empty_or_unrelated_binary_cannot_pass(self):
        """Require application identity before accepting an otherwise clean file."""
        with self.assertRaisesRegex(ValueError, "Cannot identify"):
            boundary.verify_native_payload(b"unrelated shared library", "mobile")

    def test_shared_network_dependencies_are_allowed(self):
        """Core networking crates may remain shared with desktop integrations."""
        boundary.verify_dependency_tree(
            "inverter-dashboard v1.0.0 (/checkout)\nreqwest v0.13.4\nfutures-util v0.3.34 (*)"
        )

    def test_transitive_desktop_dependency_is_rejected(self):
        """A forbidden runtime crate fails regardless of its dependency depth."""
        with self.assertRaisesRegex(ValueError, "tokio-tungstenite"):
            boundary.verify_dependency_tree(
                "inverter-dashboard v1.0.0\nadapter v1.0.0\ntokio-tungstenite v0.30.0"
            )

    def test_unrelated_cargo_workspace_cannot_pass(self):
        """Reject dependency output for a different application."""
        with self.assertRaisesRegex(ValueError, "inverter-dashboard"):
            boundary.verify_dependency_tree("other-application v1.0.0")

    def test_compiler_source_graph_accepts_core_with_escaped_spaces(self):
        """Support escaped directory spaces and continued dep-info lines."""
        path = self.root / "library.d"
        path.write_text(
            "/workspace\\ name/target/lib.a: /workspace\\ name/src-tauri/src/lib.rs "
            "\\\n /workspace\\ name/src-tauri/src/inverter_control.rs\n"
        )
        boundary.verify_depfile(path)

    def test_compiler_source_graph_rejects_excluded_module(self):
        """Reject feature source even when it exposes no recognizable command."""
        path = self.root / "library.d"
        path.write_text(
            "/target/lib.a: /checkout/src-tauri/src/lib.rs "
            "/checkout/src-tauri/src/mqtt/camera_events.rs\n"
        )
        with self.assertRaisesRegex(ValueError, "camera_events"):
            boundary.verify_depfile(path)

    def test_missing_compiler_source_graph_cannot_pass(self):
        """Reject empty or unrelated compiler inputs rather than assuming absence."""
        path = self.root / "library.d"
        path.write_text("/target/other.a: /checkout/other.rs")
        with self.assertRaisesRegex(ValueError, "Missing application source"):
            boundary.verify_depfile(path)

    def test_android_checks_every_packaged_abi(self):
        """A clean first architecture cannot hide a contaminated second one."""
        path = self.archive(
            "app.aab",
            {
                "base/lib/arm64-v8a/libinverter_dashboard_lib.so": CORE,
                "base/lib/x86_64/libinverter_dashboard_lib.so": CORE
                + b"discover_ha_entities",
            },
        )
        with self.assertRaisesRegex(ValueError, "discover_ha_entities"):
            boundary.verify_archive(path, "android")

    def test_android_aab_and_apk_paths_are_supported(self):
        """Find app libraries in both bundle and installable APK layouts."""
        for prefix in ("", "base/"):
            with self.subTest(prefix=prefix):
                path = self.archive(
                    "app.zip",
                    {f"{prefix}lib/arm64-v8a/libinverter_dashboard_lib.so": CORE},
                )
                self.assertEqual(
                    boundary.verify_archive(path, "android"), {"aarch64-linux-android"}
                )

    def test_android_missing_application_library_cannot_pass(self):
        """Reject archives that contain assets but no application native library."""
        path = self.archive("empty.apk", {"assets/index.html": b"html"})
        with self.assertRaisesRegex(ValueError, "No application native"):
            boundary.verify_archive(path, "android")

    def test_ios_resolves_actual_executable_from_plist(self):
        """Read the application executable named by the packaged iOS metadata."""
        path = self.archive(
            "app.ipa",
            {
                "Payload/Energy.app/Info.plist": plistlib.dumps(
                    {"CFBundleExecutable": "Energy"}
                ),
                "Payload/Energy.app/Energy": CORE,
            },
        )
        self.assertEqual(boundary.verify_archive(path, "ios"), {"aarch64-apple-ios"})

    def test_ios_cannot_hide_feature_in_executable(self):
        """Inspect the executable content after resolving the iOS bundle layout."""
        path = self.archive(
            "app.ipa",
            {
                "Payload/Energy.app/Info.plist": plistlib.dumps(
                    {"CFBundleExecutable": "Energy"}
                ),
                "Payload/Energy.app/Energy": CORE + b"camera-event",
            },
        )
        with self.assertRaisesRegex(ValueError, "camera-event"):
            boundary.verify_archive(path, "ios")


class MobilePluginBoundaryTests(unittest.TestCase):
    """Keep the signed package ecosystem out of native mobile inputs."""

    def test_plugin_manager_commands_are_rejected_in_every_mobile_package_format(self):
        """Shared handler registration cannot leak manager actions into mobile apps."""
        # TestCase exits this context even when a package assertion fails.
        # pylint: disable-next=consider-using-with
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        # Keep the required manager surface explicit rather than deriving this
        # fixture from the guard, so an omitted command fails the regression.
        for command in (
            "get_plugin_manager_snapshot",
            "get_plugin_settings",
            "save_plugin_settings",
            "preview_plugin_package",
            "install_plugin_package",
            "discard_plugin_package",
            "set_plugin_enabled",
            "rollback_plugin_package",
            "uninstall_plugin_package",
        ):
            payload = CORE + b"\x00" + command.encode()
            for suffix, prefix in (("apk", ""), ("aab", "base/"), ("ipa", "")):
                if suffix == "ipa":
                    entries = {
                        "Payload/Energy.app/Info.plist": plistlib.dumps(
                            {"CFBundleExecutable": "Energy"}
                        ),
                        "Payload/Energy.app/Energy": payload,
                    }
                    platform = "ios"
                else:
                    entries = {
                        f"{prefix}lib/arm64-v8a/libinverter_dashboard_lib.so": payload
                    }
                    platform = "android"
                with self.subTest(command=command, suffix=suffix):
                    archive = package_fixture(root / f"app.{suffix}", entries)
                    with self.assertRaisesRegex(ValueError, command):
                        boundary.verify_archive(archive, platform)

    def test_plugin_package_dependencies_are_rejected(self):
        """Package verification and installation must not enter mobile runtime code."""
        for crate in ("ed25519-dalek", "curve25519-dalek", "zip"):
            with self.subTest(crate=crate), self.assertRaisesRegex(ValueError, crate):
                boundary.verify_dependency_tree(
                    f"inverter-dashboard v1.0.0\npackage-adapter v1.0.0\n{crate} v1.0.0"
                )

    def test_compiler_graph_rejects_plugin_services_and_publisher_policy(self):
        """Startup, storage, and embedded policy stay out even without handlers."""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "library.d"
            for source in (
                "application.rs",
                "bridge.rs",
                "settings.rs",
                "settings_store.rs",
                "publishers.json",
            ):
                path.write_text(
                    "/target/lib.a: /checkout/src-tauri/src/lib.rs "
                    f"/checkout/src-tauri/src/plugins/{source}\n"
                )
                with (
                    self.subTest(source=source),
                    self.assertRaisesRegex(ValueError, "plugins"),
                ):
                    boundary.verify_depfile(path)

    def test_shared_config_source_excludes_the_plugin_key_adapter_on_mobile(self):
        """Core encryption remains shared; its desktop adapter needs its own guard."""
        source = (SCRIPT.parents[1] / "src-tauri/src/config_store.rs").read_text()
        # Depfiles correctly include config_store.rs on mobile. This narrow
        # source check complements native target compilation for its one new
        # desktop-only entry point, without excluding core credential storage.
        self.assertRegex(
            source,
            r'#\[cfg\(not\(any\(target_os = "android", target_os = "ios"\)\)\)\]\s*'
            r'pub\(crate\) fn plugin_settings_key\(',
        )


if __name__ == "__main__":
    unittest.main()

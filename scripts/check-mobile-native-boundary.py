#!/usr/bin/env python3
"""Reject desktop feature code in compiled mobile libraries and shipped packages.

This combines the compiler's source dependency graph, Cargo's target-selected
normal dependencies, and the final native payload. Legacy configuration field
names are deliberately allowed: importing old settings does not load a feature.
"""

# CLI filenames use hyphens to match their shell entry points.
# pylint: disable=invalid-name

import argparse
import json
import plistlib
import re
import subprocess
import zipfile
from pathlib import Path

CHECKOUT = Path(__file__).resolve().parents[1]
ANDROID_TARGETS = {
    "arm64-v8a": "aarch64-linux-android",
    "armeabi-v7a": "armv7-linux-androideabi",
    "x86_64": "x86_64-linux-android",
    "x86": "i686-linux-android",
}
FORBIDDEN_COMMANDS = (
    "disconnect_ha_mqtt",
    "connect_ha_mqtt",
    "test_ha_connection",
    "get_ha_appliance_states",
    "get_ha_entity_states",
    "discover_ha_entities",
    "get_ha_filtered_data",
    "get_ha_connection_status",
    "set_cover_position",
    "open_camera_video_window",
    "close_camera_video_window",
)
FORBIDDEN_PROTOCOLS = ("ha-filtered-update", "camera-event", "frigate/events")
FORBIDDEN_CRATES = {"tokio-tungstenite", "tungstenite"}
FORBIDDEN_SOURCES = re.compile(
    r"/src-tauri/src/(?:ha_api(?:\.rs|/)|ha_session\.rs|camera(?:\.rs|/)"
    r"|mqtt/camera_events\.rs|plugins/|desktop/)"
)


def verify_dependency_tree(tree):
    """Inspect the normal dependency tree after target and feature selection.

    `cargo metadata` can include inactive optional edges (for example rumqttc's
    websocket adapter), so absence checks use `cargo tree`'s resolved view.
    """
    names = {line.split()[0] for line in tree.splitlines() if line.strip()}
    if "inverter-dashboard" not in names:
        raise ValueError("Cargo tree must contain the inverter-dashboard package")
    for name in names:
        if name in FORBIDDEN_CRATES or name.startswith("inverter-plugin-"):
            raise ValueError(f"Desktop feature dependency in mobile graph: {name}")


def verify_depfile(path):
    """Use rustc's actual input list, including nested modules and included files."""
    content = path.read_text().replace("\\ ", " ").replace("\\\n", "")
    if "/src-tauri/src/lib.rs" not in content:
        raise ValueError(f"Missing application source in Rust dep-info: {path}")
    leaked = FORBIDDEN_SOURCES.search(content)
    if leaked:
        raise ValueError(f"Desktop feature source compiled into mobile: {leaked[0]}")


def verify_native_payload(payload, label):
    """Reject registered feature commands/protocols and require the core handler."""
    for command in ("perform_action", "connect_mqtt"):
        if command.encode() not in payload:
            raise ValueError(f"Cannot identify inverter core commands in {label}")
    for marker in FORBIDDEN_COMMANDS + FORBIDDEN_PROTOCOLS:
        if marker.encode() in payload:
            raise ValueError(f"Desktop feature marker {marker!r} in {label}")


def verify_archive(path, platform):
    """Inspect packaged app code without extracting or executing any payload."""
    targets = set()
    with zipfile.ZipFile(path) as archive:
        if platform == "ios":
            plists = [
                name
                for name in archive.namelist()
                if re.fullmatch(r"Payload/[^/]+\.app/Info\.plist", name)
            ]
            if len(plists) != 1:
                raise ValueError(f"Expected one application Info.plist in {path}")
            info = plistlib.loads(archive.read(plists[0]))
            executable = info.get("CFBundleExecutable")
            if not isinstance(executable, str) or Path(executable).name != executable:
                raise ValueError(f"Invalid application executable in {path}")
            name = str(Path(plists[0]).parent / executable)
            verify_native_payload(archive.read(name), f"{path}:{name}")
            targets.add("aarch64-apple-ios")
        else:
            for name in archive.namelist():
                match = re.fullmatch(
                    r"(?:base/)?lib/([^/]+)/libinverter_dashboard_lib\.so", name
                )
                if not match:
                    continue
                target = ANDROID_TARGETS.get(match[1])
                if target is None:
                    raise ValueError(f"Unrecognized Android ABI {match[1]} in {path}")
                verify_native_payload(archive.read(name), f"{path}:{name}")
                targets.add(target)
            if not targets:
                raise ValueError(f"No application native libraries found in {path}")
    return targets


def cargo_tree(manifest, target):
    """Read the runtime dependency tree for one native compilation target."""
    result = subprocess.run(
        [
            "cargo",
            "tree",
            "--locked",
            "--offline",
            "--target",
            target,
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
            "--manifest-path",
            str(manifest),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


def cargo_metadata(manifest):
    """Locate Cargo's configured target directory without resolving dependencies."""
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            str(manifest),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


def main():
    """Verify each packaged architecture and its corresponding compiler inputs."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=("android", "ios"), required=True)
    inputs = parser.add_mutually_exclusive_group(required=True)
    inputs.add_argument("--artifact", type=Path, nargs="+")
    inputs.add_argument("--native-library", type=Path)
    parser.add_argument("--target", help="Rust target for a raw native library")
    parser.add_argument(
        "--manifest-path", type=Path, default=CHECKOUT / "src-tauri/Cargo.toml"
    )
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--profile", default="release")
    args = parser.parse_args()

    if args.native_library:
        valid = (
            args.target in ANDROID_TARGETS.values()
            if args.platform == "android"
            else args.target
            in {"aarch64-apple-ios", "aarch64-apple-ios-sim", "x86_64-apple-ios"}
        )
        if not valid:
            parser.error("--native-library requires a matching mobile --target")
        verify_native_payload(
            args.native_library.read_bytes(), str(args.native_library)
        )
        targets = {args.target}
    else:
        targets = set()
        for artifact in args.artifact:
            targets.update(verify_archive(artifact, args.platform))

    target_dir = args.target_dir or Path(
        cargo_metadata(args.manifest_path)["target_directory"]
    )
    for target in sorted(targets):
        verify_dependency_tree(cargo_tree(args.manifest_path, target))
        verify_depfile(
            target_dir / target / args.profile / "libinverter_dashboard_lib.d"
        )
        print(f"Verified mobile native core boundary: {target}")


if __name__ == "__main__":
    try:
        main()
    except (
        ValueError,
        OSError,
        KeyError,
        zipfile.BadZipFile,
        subprocess.CalledProcessError,
    ) as error:
        raise SystemExit(
            f"Mobile native boundary verification failed: {error}"
        ) from error

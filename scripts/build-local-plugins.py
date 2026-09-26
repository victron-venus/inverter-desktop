#!/usr/bin/env python3
"""Compile all desktop workers and create local .idplugin artifacts with checksums.

No release plan, network publication or signing key is needed. Artifacts are
published to a new local directory only when every worker packages successfully.
This builds packages; installed plugin pins and encrypted settings are unchanged.
"""

import argparse
import hashlib
import json
import subprocess
import tempfile
import tomllib
import uuid
from pathlib import Path

from plugins import plugin_package

ROOT = Path(__file__).resolve().parents[1]


def compiler_host(root):
    """Restrict native builds to a supported desktop ABI."""
    output = subprocess.check_output(["rustc", "-vV"], cwd=root, text=True)
    hosts = [line.removeprefix("host: ") for line in output.splitlines()
             if line.startswith("host: ")]
    if len(hosts) != 1 or hosts[0] not in plugin_package.TARGETS:
        raise ValueError("Local plugins require a supported desktop Rust host")
    return hosts[0]


def run(root, arguments):
    """Compile or package without executing a worker."""
    subprocess.run([str(argument) for argument in arguments], cwd=root, check=True)


def build_plugins(root, target):
    """Build one complete native plugin set, retaining previous successful sets."""
    if target not in plugin_package.TARGETS:
        raise ValueError("Local plugins require a supported desktop target")
    target_dir = root / "src-tauri" / "target"
    output = root / "target" / "local-plugins" / target
    output.mkdir(parents=True, exist_ok=True)
    plugin_package.checked_path(output)
    run(root, ["cargo", "build", "--locked", "--release", "--manifest-path",
               root / "src-tauri/Cargo.toml", "--example", "plugin-package",
               "--target", target, "--target-dir", target_dir])
    suffix = ".exe" if "windows" in target else ""
    packager = target_dir / target / "release/examples" / ("plugin-package" + suffix)
    with tempfile.TemporaryDirectory(prefix=".building-", dir=output) as temporary:
        staging = Path(temporary).resolve()
        artifacts = staging / "artifacts"
        artifacts.mkdir()
        packages = []
        for plugin, (_, binary) in plugin_package.PLUGINS.items():
            manifest = root / "desktop-plugins" / plugin / "Cargo.toml"
            cargo = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]
            if cargo["name"] != binary:
                raise ValueError("Worker Cargo identity does not match package metadata")
            run(root, ["cargo", "build", "--locked", "--release", "--manifest-path",
                       manifest, "--bin", binary, "--target", target,
                       "--target-dir", target_dir])
            worker = target_dir / target / "release" / (binary + suffix)
            prepared = plugin_package.prepare(worker, target, staging / plugin, plugin)
            metadata = json.loads((prepared / "manifest.json").read_text(encoding="utf-8"))
            if metadata["version"] != cargo["version"]:
                raise ValueError("Worker Cargo version does not match package metadata")
            name = f"{metadata['plugin_id']}-{metadata['version']}-{target}.idplugin"
            archive = artifacts / name
            run(root, [packager, "--unsigned", "--manifest", prepared / "manifest.json",
                       "--root", prepared / "payload", "--output", archive])
            data = archive.read_bytes()
            if not data:
                raise ValueError("Packaging utility produced an empty archive")
            digest = hashlib.sha256(data).hexdigest()
            (artifacts / (name + ".sha256")).write_text(
                f"{digest}  {name}\n", encoding="utf-8")
            packages.append({"plugin_id": metadata["plugin_id"],
                             "version": metadata["version"], "file": name,
                             "sha256": digest})
        # An inventory, not an importable configuration: local files must not
        # masquerade as published release URLs or change installed package pins.
        inventory = {"target": target, "packages": packages}
        (artifacts / "local-plugin-artifacts.json").write_text(
            json.dumps(inventory, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        destination = output / uuid.uuid4().hex
        artifacts.rename(destination)
    return destination


def main():
    """Build the current checkout's workers for the native desktop target."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()
    try:
        destination = build_plugins(ROOT, compiler_host(ROOT))
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Cannot build local plugins: {error}\n")
    print(f"Local plugin artifacts: {destination}")
    print("Built all four .idplugin archives and SHA-256 checksums.")
    print("Installed plugin pins are unchanged. To install a published build, import")
    print("its matching desktop-plugins fragment in your configuration backup.")


if __name__ == "__main__":
    main()

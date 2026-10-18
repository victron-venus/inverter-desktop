#!/usr/bin/env python3
"""Fail closed when the temporary zbus 5 retirement deferral no longer applies."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import os
from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
EXPIRES = datetime(2026, 11, 17, tzinfo=timezone.utc)
DEFERRALS = {
    "RUSTSEC-2026-0370": "zvariant",
    "RUSTSEC-2026-0371": "zbus_names",
    "RUSTSEC-2026-0372": "zvariant_derive",
}
# Review the deferral again whenever any part of these dependency paths changes.
LOCKED_VERSIONS = {
    "zvariant": "5.12.0",
    "zvariant_derive": "5.12.0",
    "zvariant_utils": "3.4.0",
    "zbus_names": "4.3.2",
    "zbus": "5.16.0",
    "zbus_macros": "5.16.0",
    "notify-rust": "4.18.2",
    "tauri-plugin-notification": "2.5.1",
    "tauri-plugin-opener": "2.7.1",
    "secret-service": "5.1.0",
    "zbus-secret-service-keyring-store": "1.0.0",
    "keyring": "4.2.0",
}
EXISTING_IGNORES = {"RUSTSEC-2026-0192", "RUSTSEC-2024-0370"}
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def advisory_metadata(source: str) -> dict:
    """Read RustSec's leading fenced TOML without interpreting its prose."""
    if not source.startswith("```toml\n"):
        raise ValueError("Advisory does not begin with RustSec TOML metadata")
    header, separator, _ = source[len("```toml\n") :].partition("\n```")
    if not separator:
        raise ValueError("Advisory TOML metadata is not terminated")
    return tomllib.loads(header)


def validate_advisory(metadata: dict, advisory_id: str, package: str) -> None:
    """Only the reviewed, unresolved informational retirement is deferred."""
    advisory = metadata["advisory"]
    if (advisory.get("id"), advisory.get("package"), advisory.get("informational")) != (
        advisory_id, package, "unmaintained"
    ):
        raise ValueError(f"Review changed RustSec classification: {advisory_id}")
    if any(advisory.get(key) for key in ("cvss", "categories", "aliases", "withdrawn")):
        raise ValueError(f"Review changed RustSec security classification: {advisory_id}")
    versions = metadata.get("versions", {})
    if versions.get("patched") != [] or versions.get("unaffected", []) != []:
        raise ValueError(f"Review changed RustSec remediation: {advisory_id}")


def validate_deferrals(root: Path, database: Path, now: datetime) -> None:
    """Validate metadata, exact dependency versions and both cargo-deny configs."""
    if now >= EXPIRES:
        raise ValueError(f"zbus retirement deferral expired at {EXPIRES.isoformat()}")

    packages = tomllib.loads((root / "src-tauri/Cargo.lock").read_text())["package"]
    for name, version in LOCKED_VERSIONS.items():
        matches = [package for package in packages if package["name"] == name]
        if len(matches) != 1 or (
            matches[0]["version"], matches[0].get("source")
        ) != (version, REGISTRY):
            raise ValueError(f"Review deferral after dependency change: {name}@{version}")

    expected_ignores = EXISTING_IGNORES | DEFERRALS.keys()
    for filename in ("cargo-deny.toml", "deny.toml"):
        config = tomllib.loads((root / "src-tauri" / filename).read_text())
        entries = config["advisories"]["ignore"]
        ignores = [entry if isinstance(entry, str) else entry["id"] for entry in entries]
        if len(ignores) != len(expected_ignores) or set(ignores) != expected_ignores:
            raise ValueError(f"Unexpected advisory exceptions in {filename}")

    for advisory_id, package in DEFERRALS.items():
        path = database / "crates" / package / f"{advisory_id}.md"
        validate_advisory(advisory_metadata(path.read_text()), advisory_id, package)


def find_database(database_root: Path) -> Path:
    """Resolve the single RustSec database fetched by cargo-deny."""
    candidates = [
        path
        for path in database_root.glob("*")
        if (path / "crates/zvariant/RUSTSEC-2026-0370.md").is_file()
    ]
    if len(candidates) != 1:
        raise ValueError("Expected exactly one freshly fetched RustSec database")
    return candidates[0]


def main() -> int:
    """Check the fresh database before cargo-deny consumes its offline snapshot."""
    cargo_home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database-root", type=Path, default=cargo_home / "advisory-dbs")
    args = parser.parse_args()
    try:
        database = find_database(args.database_root)
        validate_deferrals(ROOT, database, datetime.now(timezone.utc))
    except (OSError, KeyError, TypeError, ValueError) as error:
        print(f"RustSec deferral rejected: {error}", file=sys.stderr)
        return 1
    print(f"Three informational retirement deferrals remain valid until {EXPIRES.isoformat()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

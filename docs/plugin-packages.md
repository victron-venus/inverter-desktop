# Desktop plugin packages

The native package pipeline builds, verifies, installs, updates, disables, rolls
back, and removes signed desktop workers. It is a native API checkpoint: the
application does not yet expose package-install IPC, a plugin manager, automatic
package discovery, or installed HA/camera workers. The shipped worker registry
remains empty and the legacy desktop features remain bundled.

Android and iOS compile neither this pipeline nor its publisher policy,
cryptographic verifier, ZIP handling, or packaging tool. The existing native
artifact gate also rejects the new desktop-only dependencies.

## Package and signature contract

An `.idplugin` file uses a strict, uncompressed ZIP layout. `manifest.json` comes
first, followed by regular payload files in sorted portable-path order. Fixed ZIP
timestamps and permissions make identical metadata, payload bytes, and signing
keys produce identical archive bytes. No host filesystem timestamp enters the
package identity. The producer pins ZIP creator metadata to Unix on every desktop
OS. The verifier also accepts DOS creator metadata with ordinary file attributes,
including the regular Unix mode written by Windows ZIP tooling, while rejecting
links, special files, and directory attributes. Compressed/encrypted entries,
directories, extra fields,
comments, ZIP64, data descriptors, duplicate paths, and unlisted payloads are not
part of this format. The entire archive is bounded to 64 MiB.

The manifest uses the [worker manifest schema](plugin-worker-protocol.md), with
a mandatory Ed25519 signature. The signing message is the ASCII domain prefix
`inverter-desktop:idplugin:manifest:v1` followed by a NUL byte and compact UTF-8
JSON serialization of the typed manifest with `signature` set to `null`. Object
keys within schema metadata are sorted recursively. Verification requires the
canonical manifest representation, so duplicate JSON keys and alternative
representations cannot create different interpretations of the signed content.

The signature authenticates plugin identity, version, host API requirement,
target, entrypoint, configuration schema, permission declarations, and every
payload path, byte length, and SHA-256 digest. The verifier checks the complete
archive structure and file inventory before returning an immutable
`VerifiedPackage`. Installation consumes those verified bytes, rather than
reopening a caller-controlled archive after verification.

The parser's larger declared-inventory limit does not relax the stricter archive
limit. A signed manifest with incompatible target/API, unsafe paths, unexpected
files, missing files, or incorrect lengths/digests is rejected.

## Producing an archive

Build the frontend once, then run the desktop packaging tool from the repository
root with an existing publisher seed file:

```bash
pnpm build
cargo run --locked --manifest-path src-tauri/Cargo.toml --example plugin-package -- \
  --manifest /absolute/path/plugin-manifest.json \
  --root /absolute/path/payload \
  --key-id publisher-v1 \
  --key-file /secure/path/publisher.seed \
  --output /absolute/path/plugin.idplugin
```

The manifest input supplies all `PluginManifest` metadata fields. Set `inventory`
to `[]` and `signature` to `null`; the tool replaces both using the actual payload
files and the supplied key. The target is the worker's Rust desktop target triple,
and the entrypoint is a portable path relative to the payload root.

The key file contains exactly 32 raw Ed25519 seed bytes, not hexadecimal, Base64,
or PEM. Both it and the manifest file must reside outside the payload directory.
The tool does not generate keys or change receiver trust. It rejects linked files,
symlink/reparse-point parent components, nonportable names, source changes during
collection, and a payload that contains a copy of the private seed. The source
tree admits at most 128 files and 512 directory entries within the archive budget.

The destination parent must already exist. Output publication never overwrites an
existing file, including a symlink. Identical valid inputs produce identical bytes;
the producer fixes timestamps to 1980-01-01 and archive permissions to 0644 for
data and 0755 for the entrypoint. It only creates an archive and never launches
the worker. Native executable signing/notarization remains a release requirement.

Windows MSVC builds embed the same Common Controls v6 manifest in application,
test, and example executables. Tauri's default resource handling only covers
application binaries; the other executables also need this dependency to load
the linked dialog/tray APIs before their own code can run.

## Publisher trust

`src-tauri/src/plugins/publishers.json` is a release-owned policy embedded only in
desktop builds. Each publisher entry has a stable `key_id`, a 32-byte Ed25519
public key encoded as 64 lowercase hexadecimal digits, and an explicit list of
plugin IDs that key may sign. Key rotation can retain the old and new key IDs
for the same plugin during a release transition.

The initial policy has no publishers. A maintainer must configure real publisher
keys through a reviewed application release before shipping installation support.
There is no trust-on-first-use, package-provided public key, webview override,
environment override, or unsigned installation fallback. Fixture signing keys
exist only in tests and are never added to this policy.

Native test or development callers may construct their own `TrustStore`; that is
an explicit native trust boundary, not an application user setting. A key valid
for one plugin cannot sign another plugin merely because the signature is valid.
Changing or removing release trust must take effect when installed packages are
reverified before a subsequent start.

## Lifecycle requirements

The native entry point is `PackageManager::open(root, target, trust, host)`.
Its clones share one transaction mutex. A lifetime OS file lock excludes another
manager/process from the same store until owned workers have been reaped. Call
`close().await` to complete cleanup explicitly; if process cleanup cannot be
confirmed, the lease must remain held until the hosting process exits.

The store uses an atomically created `store-v1/` ownership directory, a bounded
`state.json` inventory, private
staging directories, and content-addressed package versions. Each immutable
version retains its verified archive and extracted payload. At most eight plugins,
two retained versions per plugin after recovery, 512 MiB of total store data, and
8,192 filesystem entries are admitted. These storage limits are independent of
the worker's runtime CPU or memory use.

Package lifecycle operations are serialized. Verification and staging precede
activation; a candidate must complete the real worker handshake before becoming
active. The previous working package remains available for rollback. A failed
update must preserve the previous package state and restore its worker when the
original session is still authorized.

Operations capture the authentication epoch before waiting for the transaction
lock. Registration checks that same epoch under the host's authority lock.
Logout and re-login cannot authorize an old queued activation. Worker removal
waits for termination and reaping before releasing its registry slot or deleting
owned package files. The package manager has no MQTT/IGW handle and does not
reconnect core telemetry.

Opening a package store recovers its metadata and interrupted staging work; it
does not execute previously enabled workers. Application startup/session/UI
integration remains a later checkpoint. Plugin settings and secrets are not
migrated by this package layer, and uninstall must leave unrelated data intact.

Ordinary caller cancellation does not abandon an in-progress transaction: an
owned task finishes or restores its state while holding the store lease. Process
interruption can leave staging or obsolete content that the next open removes.
State files are synced before atomic replacement; parent directories are synced
on Unix. This does not promise identical sudden-power-loss behavior across file
systems, and Windows directory metadata has no equivalent sync step in this API.
Force-killing the application can also leave a noncooperating OS process outside
normal worker cleanup; this checkpoint does not adopt orphan processes.

## Verification boundaries

Tests use disposable signing keys, actual produced archives, and separately
compiled fixture workers. They must establish deterministic packaging, signature
and inventory rejection, installation, failed activation, rollback, cancellation,
recovery, and removal. Mobile source/dependency/payload checks complement these
desktop lifecycle tests.

A verified package authenticates content and publisher scope. Its worker still
runs with the user's OS privileges; this is not an OS sandbox. Permission
metadata does not implement network, secret, configuration, or media services.
The remaining contribution services and HA/camera parity work stay in
[TODO.md](../TODO.md).

This checkpoint does not establish production publisher provisioning, signed
macOS/Windows worker distribution, real HA/camera behavior, physical inverter
commands, or mobile device usability.

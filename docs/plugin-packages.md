# Desktop plugin packages

The desktop application installs packages declared in application configuration
using exact version, platform, and archive SHA-256 pins. It also retains a separate
manual signed-package review flow. **Configuration → Plugins** shows download and
restoration results, installed workers, typed settings, and package lifecycle actions.

Installing a plugin does not add a generic panel or duplicate its entity cards on
the main dashboard. The existing home and appliance sections remain in place.
Connection diagnostics appear in **Configuration -> Plugins**. Background
workers, Frigate notifications and owned video windows continue without a visible
dashboard panel. Integrating plugin data into the existing feature sections is a
separate migration step; installing a package does not complete that migration.

Configured downloads can use unsigned packages and need no publisher or app
signing keys. The embedded publisher policy is currently empty, so manual package
selection remains unavailable. Home Assistant and cameras still have bundled
desktop implementations alongside the separately packaged workers; full migration
and bundled-feature removal remain unfinished.

Android and iOS compile neither the manager UI nor its native commands, package
store startup, publisher policy, cryptographic verifier, ZIP handling, or packaging
implementation. Mobile preserves declarations as dormant configuration data;
it does not interpret, fetch or execute them. Mobile artifact gates reject the
desktop implementations.

## Declaring plugins in application configuration

Add a `desktop_plugins` list to the application settings or merge the corresponding
fragment from a published release into your settings backup before importing it.
The fragment contains package declarations, not a complete application backup.
Keep only the plugins you want. To share a configuration between desktop
platforms, combine their artifact maps under one declaration for each plugin.
Each declaration supplies an exact plugin ID and version, an enabled preference,
and one or more desktop target artifacts. For example:

```json
{
  "desktop_plugins": [
    {
      "plugin_id": "example.monitor",
      "version": "1.2.3",
      "enabled": true,
      "artifacts": {
        "aarch64-apple-darwin": {
          "url": "https://packages.example.invalid/monitor-1.2.3.idplugin",
          "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        }
      }
    }
  ]
}
```

The example is illustrative; use the URL and checksum of the actual released
archive. Desktop accepts at most eight unique plugin declarations. Versions are
exact semantic versions, not ranges or `latest`. Artifact keys must be supported
desktop Rust target triples. Every configured URL must use HTTPS and contain no
credentials, query, or fragment; SHA-256 uses 64 lowercase hexadecimal characters.
Unknown declaration and artifact fields are rejected on desktop. Mobile preserves
future metadata without interpreting it. Omitted `enabled` means true; explicit
false permits installation but never launches that worker.

After authentication, startup, configuration import/save, and **Retry restoration**
reconcile the declarations with the private package store. Missing
packages are downloaded with bounded HTTPS requests, checked against their exact
identity/version/target/hash, and installed through the existing verified package
lifecycle. Healthy matching installations are reused without downloading or
restarting them. A missing artifact for this target or a failed download appears
in the manager. A failed package change preserves the previous installed version.
The full pin describes the desired content: editing or importing a declaration
can replace an archive with different bytes at the same worker version or select
an older version intentionally. Replacement is transactional and retains the
previous active archive for rollback. The app follows the saved pins; upgrading
the application does not silently select a newer release or rebuild. Change the
declaration's URL/hash and, when needed, version to select different content.

Declared plugins take their package version and enabled state from configuration.
The native API and manager prevent manual replacement, enable/disable, rollback,
or uninstall while a declaration owns that ID. Settings remain editable. Remove
the declaration to release management; doing so retains installed state and data.
Uninstall afterward if the package should also be removed.

An app upgrade or reinstall that preserves app data preserves declarations and
encrypted plugin settings. If app data was deleted, import a saved application
backup to restore the declarations and fetch missing packages. The portable backup
does not include plugin settings or secrets. A newly downloaded package needing
credentials remains installed and stopped until valid settings are saved, then
activation is retried if enabled in the declaration. Missing local data cannot be
recovered without a backup. A missing package can be downloaded again, but damaged
installed content is not automatically overwritten or erased. To reinstall a
damaged package that remains manageable, remove its declaration, uninstall the
package while retaining settings, then restore the declaration and retry. An
unreadable or corrupt store inventory is reported without deleting it; this flow
does not bypass an inventory error or silently reset the store. Source declarations
do not migrate bundled HA/camera settings or change the core inverter MQTT connection.

Hosted desktop releases generate `.idplugin` assets, `.sha256` files, and
`desktop-plugins-<target>.json` fragments. Their pinned URLs refer to that frozen
release tag and become usable only after publication. See the
[release output contract](release-workflow.md#desktop-plugin-release-assets).

## Selecting and reviewing a signed package

1. Open **Plugins** in desktop settings and choose a package through the native
   file dialog. Cancelling the dialog changes nothing. Selection is available only
   when the release policy contains approved publishers and the store is ready.
2. The native service verifies publisher scope, signature, target/API compatibility,
   archive structure, and the complete inventory. It retains the verified bytes
   and returns an opaque review token; the webview cannot supply an archive path,
   executable path, public key, or replacement archive contents.
3. Review the plugin ID, proposed and installed versions, approved publisher key,
   target, and declared capabilities. **Enable after installation** defaults to
   selected and can be cleared. Capability declarations describe features; workers
   still run with the user's OS account privileges.
4. Choose **Install** or **Update** to consume the token and commit those exact
   bytes. Replacing the original file after review cannot change the installation.
   Changes apply immediately without the configuration Save button or an app
   restart. Cancelling a review discards its token.

At most one selection/review is pending in the application. Its token belongs to
the originating settings window and authentication epoch and expires after five
minutes. Replacement, cancellation, closing that window, logout, or authentication
policy changes invalidate it. An install attempt consumes the review; an expired
or failed attempt requires selecting and reviewing the package again.

Manual signed-file updates require a newer version and reject different content
under an already installed version. Use the retained **Rollback** action to return
to its previous archive. Explicit configuration pins have the separate content
selection behavior described above, including same-version replacement and
intentional downgrade.

Installed cards show their version and running, starting, failed, installed, or
disabled state. A running worker may also provide one compact connection summary;
entity state cards and device actions are not repeated in this manager.
Rollback names the retained version. Uninstall requires an inline
confirmation identifying the plugin and version, then stops its worker and removes
owned package files. Settings are retained by default; the confirmation offers
explicit deletion of that plugin's settings and secrets. A configuration-capable
package has a **Settings** action with a native-validated declarative editor.
Password fields show only whether a secret exists; replacement and clearing are
explicit. Saving persists immediately and restarts an enabled worker. A failed
restart is reported separately from a failed save. See the [settings
contract](plugin-settings.md). Native failures are rendered as text in English/Russian UI; metadata never becomes HTML or
executable frontend code.

The on-demand stored-data view lists sizes and quota usage without decrypting
records. Data with no installed owner can be removed after explicit confirmation;
data belonging to any installed package remains protected. A removed package's
name cannot be reconstructed from its record hash. See [retained data inventory
and cleanup](plugin-settings.md#retained-data-inventory-and-cleanup).

## Package and signature contract

An `.idplugin` file uses a strict, uncompressed ZIP layout. `manifest.json` comes
first; the producer writes regular payload files in sorted portable-path order.
Fixed ZIP timestamps and permissions make identical metadata, payload bytes, and
signing keys produce identical archive bytes. Sorting and fixed timestamp values
are producer normalization, not additional verifier requirements. No host filesystem timestamp enters the
package identity. The producer pins ZIP creator metadata to Unix on every desktop
OS. The verifier also accepts DOS creator metadata with ordinary file attributes,
including the regular Unix mode written by Windows ZIP tooling, while rejecting
links, special files, and directory attributes. Compressed/encrypted entries,
directories, extra fields,
comments, ZIP64, data descriptors, duplicate paths, and unlisted payloads are not
part of this format. The entire archive is bounded to 64 MiB.

The manifest uses the [worker manifest schema](plugin-worker-protocol.md).
Manual file selection requires an Ed25519 signature from an approved publisher.
Configured downloads instead authorize the exact archive SHA-256 recorded in
application settings; their manifest may have `signature: null`. Both paths enforce
the same target/API, archive structure, and complete payload inventory checks.
The signing message is the ASCII domain prefix
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
limit. A manifest with incompatible target/API, unsafe paths, unexpected
files, missing files, or incorrect lengths/digests is rejected.

## Frigate worker package

The first real feature payload is the separately built
[Frigate worker](../desktop-plugins/frigate/README.md). Version 0.2.0 declares
configuration, dashboard status, MQTT networking, native desktop notifications,
and scoped HTTP video under host API 1.3. A nonempty direct Frigate base URL enables
completed-clip requests; the host owns their download, private files, and windows.
The staging helper verifies a target-specific native executable header and copies
only that worker into the payload. It does not execute the binary, generate keys,
install anything, or modify publisher policy. Compiler provenance and runtime
library compatibility still require target builds and execution checks.

Two dedicated acceptance tests package the actual executable with a disposable
fixture key and exercise it through the application service against a private
Mosquitto broker. The clip fixture adds a private HTTP origin, owned range reads,
and lifecycle cleanup through a simulated native window adapter. HTTP transfer
failures and resource limits have separate service tests; neither test suite
proves native decoding or window behavior. The opt-in
[native media smoke harness](native-plugin-media-smoke.md) covers that separate
boundary; local macOS playback/window acceptance passed, while Linux and Windows
graphical acceptance remains pending. This does not provision production trust
or replace the bundled camera feature; current acceptance results stay in TODO.md. Frigate snapshots,
optional HA proxy access, other camera adapters, and legacy configuration
migration remain unfinished.

## Home Assistant worker package

The separate [Home Assistant worker](../desktop-plugins/home-assistant/README.md)
uses the same package/configuration lifecycle with a bounded explicit entity list,
an HTTP(S) base URL, and a write-only HA token. It declares only dashboard
contributions, plugin configuration, and direct HTTP/WebSocket networking. The
worker supplies connection status and state cards. Version 0.12 requires host API
`^1.7` to group each selected entity's state and explicitly authorized controls
through validated `state_id` references. Existing IDs, parameters and numeric
revisions retain their authority; equal friendly names do not merge entities.
Explicitly watched weather entities project condition and a finite temperature
with its supplied unit into one bounded text card. At most five existing legacy
forecast entries are included when supplied by the state. This adds no requests,
service authority, configuration fields, contribution kinds or state slots;
modern forecast subscriptions remain separate work.
Optional `dishwasher_running_entity` and `dishwasher_duration_entity` settings
assign two distinct literal entities to a read-only profile. The duration role
requires the running role; both are watched within the 64-state union.
The running entity's existing card combines its state and literal runtime since
midnight, with no inferred unit, conversion or countdown. The duration card remains independently visible and may have its own
explicitly configured remaining-time profile. Either role's updates refresh the summary, while unknown or
unavailable running state uses the existing status card. The two settings default
to empty and use `omitEmpty` to preserve prior startup envelopes. This adds no
service authority, contribution kind, host API or automatic settings migration.
Optional `washer_remaining_entity` and `dryer_remaining_entity` each assign one
literal remaining-time source to its existing card. The roles append to the same
read union, retain complete bounded literals including zero, and use neutral
idle/unknown/unavailable statuses. They do not infer activity, units or a local
countdown, and grant no controls. Each primary profile needs a distinct entity;
sharing a dishwasher duration source is allowed and explicitly projects that
independently visible duration card too. Both projections update together after
the same source-ordering guard. Empty defaults use `omitEmpty` and preserve the
prior 32-KiB configuration/storage boundary; selected values count normally.
Existing appliance start/pause buttons require separate `action_entities`
selection. Legacy settings migration and complete bundled UI removal remain open.

The worker keeps service actions disabled unless `action_entities` explicitly
selects literal `button.*` or `scene.*` targets, `media_player_entities` selects
literal media players, `binary_entities` selects literal switches, input booleans
or lights, `cover_entities` selects literal covers, `number_entities` selects
literal numbers, or `cover_position_entities` selects literal covers for position writes.
Fixed button presses, scene
activation, media-player Play/Pause/Stop, explicit Turn on/Turn off and cover
Open/Close/Stop are supported. On/off actions require exact observed
`on` or `off` state, and service responses do not synthesize a state change.
Cover operations require a known cover state and the corresponding reported
capability; capability-only updates withdraw or restore controls. Cancellation
never sends a cover Stop command. Numeric inputs use exact bounded decimal grids
and explicit Apply; changed constraints or eligibility revoke stale submissions.
Position writes require their own selection and reported set-position support.
Tilt and other parameterized services remain pending.
HA 0.12 requires host API `^1.7`. The combined selection permits 64 watched
entities, up to 16 binary targets, and 63 controls, bounded to 128 contributions
and the unchanged 64 KiB complete-frame limit. Earlier HA packages retain their
original limits. There is no generic service proxy, core MQTT
access, camera authority, or inverter flag alias lookup. Explicit targets use
individual initial REST reads. Optional `discovery_prefixes` adds read-only
`sensor.*` and `binary_sensor.*` cards in unused slots after all explicit targets,
preserving their IDs and control authority. Prefixes are literal, with at most
eight unique entries and no wildcards or other domains; the field is empty by
default and uses `omitEmpty` to preserve existing startup envelopes.

Only enabled discovery with free slots requests the all-entity `/api/states`
collection once per connection, bounded to 1 MiB, 4,096 source items and 15 seconds.
The worker subscribes first and retains at most 128 projected live updates or
deletions during bootstrap so a late snapshot cannot resurrect older state.
Initial matches are selected lexically; later matches use free slots. Excess
matches are discarded with a visible limit indication, with no hidden catalog
or periodic collection refresh. A collection failure or buffer overflow disables
discovery for that connection while explicit reads and controls continue;
authentication rejection still stops the whole session. Reconnect resamples.
Both the collection and broader `state_changed` stream are filtered locally,
not by a restricted HA token or server-side prefix subscription. Network
permissions describe trusted worker behavior and do not sandbox its operating-system access.

`prepare-plugin-package.py --plugin home-assistant` stages this worker using
fixed built-in metadata and the same native header/path checks as Frigate. The
old Frigate staging command remains supported. The small shared
`desktop-plugins/worker-protocol` library owns bounded stdio framing, JSON
number-object rejection and flushed output; it has no feature configuration or network clients. Neither worker
is linked into the main executable or mobile build.

The explicit installed-package acceptance uses the actual release worker,
disposable package trust, a temporary HA HTTP/WebSocket fixture and an independent
core MQTT connection. Separate read-only and action fixtures cover settings
restart, disable, logout/uninstall, exact service POSTs and stale action rejection.
Current demonstrated results and pending checks are recorded in TODO.md. A production HA installation, service/UI
parity, entity-picker UI, full appliance presentation, modern forecast retrieval and legacy migration
remain separate work. The release trust policy stays
empty and this slice does not create signing keys.

## Producing an archive

Build the frontend once, then run the desktop packaging tool from the repository
root. For a package installed through an exact configuration pin, no key is needed:

```bash
pnpm build
cargo run --locked --manifest-path src-tauri/Cargo.toml --example plugin-package -- \
  --unsigned \
  --manifest /absolute/path/plugin-manifest.json \
  --root /absolute/path/payload \
  --output /absolute/path/plugin.idplugin
```

The unsigned mode removes any input signature, rebuilds the payload inventory,
and uses the same deterministic ZIP and source protections. Compute SHA-256 of
the resulting complete archive and place that exact digest in its declaration.
For manual publisher review, the existing signed mode takes an externally
supplied seed file:

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
files and, in signed mode, the supplied key. The target is the worker's Rust desktop target triple,
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
the worker. This plugin implementation introduces no new requirement to sign or
notarize desktop executables or the application. Archive authentication is a
separate mechanism from platform application signing; Android signing does not
apply to this desktop-only plugin ecosystem.

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

The policy has no publishers. It applies to manual signed-file selection;
configured exact archive pins are independently authorized by saved application
configuration and need no publisher keys. The manual path has no trust-on-first-use,
package-provided public key, webview key override, or environment override.
Fixture signing keys exist only in tests and are never added to this policy.

Native test or development callers may construct their own `TrustStore`; that is
an explicit native trust boundary, not an application user setting. A key valid
for one plugin cannot sign another plugin merely because the signature is valid.
Changing or removing release trust must take effect when installed packages are
reverified before a subsequent start.

## Application and native lifecycle

Tauri owns one desktop `PluginHost` and `PackageApplication`, shared by all
windows. The application opens its private plugin store once under its local data
directory. Only authenticated `config` windows may call manager snapshot, selection,
review, install/update, enable/disable, rollback, settings, or uninstall commands. Dashboard
windows can read contributions and dispatch advertised actions, but cannot manage
packages. File dialogs and trusted publisher configuration remain native-owned.
The management IPC surface is:

- `get_plugin_manager_snapshot`: authenticated inventory, runtime status,
  initialization errors, configured-plugin results, and manual installation availability.
- `retry_configured_plugins`: retry saved declarations in the current authenticated session.
- `preview_plugin_package`: takes no path argument; opens the native dialog and
  returns `null` on cancellation or verified metadata with an opaque token.
- `install_plugin_package({ token, enable })` and
  `discard_plugin_package({ token })`: consume or discard the native-owned review.
- `set_plugin_enabled({ pluginId, enabled })`,
  `rollback_plugin_package({ pluginId })`, and
  `uninstall_plugin_package({ pluginId, deleteSettings? })`: manage an installed
  identity. Omitted `deleteSettings` retains data.
- `get_plugin_settings({ pluginId })`: verified schema, ordinary values, opaque
  revision, and secret-presence flags.
- `save_plugin_settings({ pluginId, revision, values, secretChanges })`: validate
  and persist typed values/secret changes; return refreshed settings and a separate
  nullable `restart_error`.
- `get_retained_plugin_data`: bounded ciphertext metadata, current installed
  owner associations, and storage quotas; no stored values or key access.
- `delete_retained_plugin_data({ recordId, revision })`: remove one unchanged
  canonical record only when the native installed inventory has no owner for it.

The application uses `PackageManager::open_with_configuration` with its private
settings loader; `PackageManager::open(root, target, trust, host)` remains available
for native callers without configured workers. Its clones share one transaction
mutex. A lifetime OS file lock excludes another
manager/process from the same store until owned workers have been reaped. Call
`close().await` to complete cleanup explicitly; if process cleanup cannot be
confirmed, the lease must remain held until the hosting process exits.

The store uses an atomically created `store-v1/` ownership directory, a bounded
`state.json` inventory, private
staging directories, and content-addressed package records. Each immutable archive
record retains its verified bytes and extracted payload; a configured replacement
may have the same semantic version with a different archive digest. At most eight plugins,
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

Opening a package store recovers metadata and interrupted staging work without
executing workers. After successful authentication, configured declarations are
reconciled and other installed packages recorded as enabled can resume. Every launch
rechecks the archive against its configured pin or approved publisher and verifies
the installed payload. Disabled packages stay stopped;
a queued restore cannot undo an explicit disable or uninstall. A failed restore
is visible in the manager. Only explicit package declarations cause automatic
downloads; preserved bundled HA/camera settings do not.

Logout, policy changes, and session expiry revoke the old epoch, clear previews
and contributions, and stop its workers. A later login may restore enabled packages
as fresh processes in a new epoch; it never authorizes old queued actions or late
results. Background expiry checks cover workers and pending package operations.
The package-store lease survives ordinary authentication transitions. Normal quit
waits for initialization, transactions, and process reaping before releasing the
lease and permitting exit; repeated quit requests wait for the same cleanup.

`plugin-host-update` signals changes without carrying worker data. Settings fetch
an authenticated snapshot with at most one request in flight and one pending
refresh. Native inventory metadata is cached by inventory/session revision;
frequent worker updates do not rehash all files. Mutation/restoration/session
changes invalidate that cache, and launch verification always reads and verifies
the archive and payload again. Package operations do not reload core configuration
or reconnect MQTT/IGW.

Plugin settings and secrets are not migrated by this package layer, and uninstall
must leave unrelated data intact.

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
recovery, and removal. Application-service tests additionally cover file replacement
after review, single-use/expired consent, closed windows, authentication races,
enabled-only restoration, initialization/shutdown ordering, and empty release trust.
Vue tests exercise actual review/management controls, escaped metadata, cancellation,
busy states, listener cleanup, late responses, and burst refresh coalescing. Mobile
source/dependency/payload checks complement these desktop lifecycle tests. The
current checkpoint's local suite results, remaining target/artifact checks, and PR
merge are tracked separately in [TODO.md](../TODO.md). Browser visual smoke uses
mocked native IPC and does not establish native file-dialog GUI behavior.

A verified package establishes exact pinned content or approved publisher scope. Its worker still
runs with the user's OS privileges; this is not an OS sandbox. Permission
metadata alone does not grant host services. This checkpoint implements scoped
native settings and startup configuration for `plugin_configuration`, plus bounded
notifications for `desktop_notifications` and owned direct clips for `http_video`.
General network host services and request-time secret access remain unfinished.
Frigate opens its own MQTT connection and has no access to the core MQTT client
through this protocol.
The remaining contribution services and HA/camera parity work stay in
[TODO.md](../TODO.md).

This checkpoint does not establish production publisher provisioning, platform
code signing, complete HA/camera parity, physical inverter
commands, or mobile device usability.

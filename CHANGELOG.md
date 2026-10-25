# Changelog

All notable changes to this project will be documented in this file.

## [2.5.52] - 2026-10-24

### Fixed

- Seal the complete macOS application with an explicit ad-hoc signature before building its ZIP and DMG. The native executable, Info.plist and resources are now checked together before publication.
- Verify both macOS installer payloads against the sealed app, rejecting missing, changed or substituted files before upload.

### Upgrade

Install the new official macOS package through the existing update path. Existing
settings, credentials and plugin choices remain compatible. Version 2.5.51 macOS
packages are superseded because their linker-only signature did not seal the
application bundle. Do not repair previously published packages by re-signing them.

### Security

Ad-hoc signing verifies local bundle integrity; it does not establish an Apple
Developer ID identity or notarization. This release does not disable Gatekeeper
or change the trust policy for downloads and plugins.

## [2.5.51] - 2026-10-20

### Fixed

- Clear stale Active Loads when a successful gateway snapshot removes a load or reports unavailable power or a disconnected device. Failed polls preserve labelled cached readings; partial MQTT updates and independent Home Assistant state keep their existing behavior.
- Update the JavaScript and Rust notification/opener plugins together. The upstream fixes keep Linux notifications visible and avoid nested-runtime failures when revealing files.
- Include the reviewed Scorecard HTTP/2 client fix.

### Changed

- Update Tauri, its CLI and runtime, dialog integration, native certificate loading and Lucide icons while preserving the existing dependency compatibility checks.
- Align shared automation and RC fuzzing on a reviewed immutable Toolkit revision. Retain the reviewed ARC standby routing and its separate admission requirements.

### Upgrade

Install the package for the intended platform through the existing update path.
Settings, credentials and plugin choices remain compatible. Beta publication
checks the built candidate; device, MQTT, plugin and infrastructure acceptance
remain separate requirements.

### Security

Update Tokio to 1.53.3 in all affected application, plugin and fuzz lockfiles to
address RUSTSEC-2026-0386. Keep the existing non-Tokio dependency graph and plugin
protocol unchanged.

Update `ctutils` to 0.4.4 in the application lockfile to restore the upstream
constant-time mask generation and 32-bit `u128` comparisons covered by
RUSTSEC-2026-0385. Existing encrypted settings, password verifiers and signed
plugin formats remain compatible.

Update `rustls-webpki` to 0.103.16 and `webpki-root-certs` to 1.0.10 in every
affected lockfile to reject distrusted emSign certificate paths. Audit all nine
Rust lockfiles, including target-specific, fuzz and native-regression
dependencies, without modifying the committed dependency graph. Servers that
rely on the removed trust must provide a valid certificate chain; keep TLS
verification enabled.

Re-review the exact notification and opener versions against the fresh RustSec
database and the unchanged zbus 5 dependency paths. The three informational
retirement deferrals still expire on 2026-11-17; no advisory, source, license or
dependency check is relaxed. See [the recorded review](docs/RUSTSEC.md).

## [2.5.50] - 2026-10-18

### Fixed

- Keep the Rust and JavaScript notification plugins on matching versions so Android builds remain compatible.
- Detect split notification updates in the dependency contract checks before native builds.

### Changed

- Route Dependabot metadata approval through the trusted default branch while preserving the existing author and review policy.
- Update notification, file-system, dialog and opener plugins together with compatible frontend and Rust build dependencies.
- Document Linux credential-store requirements and recovery without changing credential storage behavior.
- Clarify desktop plugin release and delivery status and link the maintained RC fuzzing process.
- Remove obsolete handoff wording from the project documentation.

### Upgrade

Install the package for the intended platform using the existing update path.
Settings and plugin choices remain compatible. Linux installations must provide
the credential-store services described in the hardening guide; publication does
not replace device, MQTT or plugin acceptance.

### Security

The existing TLS checks, credential storage requirements and RC qualification
remain required. Three informational zbus-family retirement notices retain their
exact-version, fresh-classification guard and expire on 2026-11-17. This dependency
update does not claim to resolve those upstream maintenance notices.

## [2.5.49] - Development line

### Changes

- Strengthen release-candidate fuzz evidence with required seed corpora, bounded
  replay and validation of the pinned shared workflow.

### Upgrade

- Install the package for your current platform through the normal update path.
  Existing settings and plugin choices are preserved; this patch adds no new
  configuration or credential migration.
- Keep camera plugin packages on the patched release when updating the desktop
  application. The existing certificate and hostname verification requirements
  still apply.

### Security

- Update rustls to 0.23.46 in the application, camera plugins and fuzz lockfiles
  for RUSTSEC-2026-0346, RUSTSEC-2026-0347, RUSTSEC-2026-0348 and
  RUSTSEC-2026-0349. TLS trust and hostname verification remain enabled.

## [2.5.48] - Development line

### Added
- Ring-MQTT camera motion/ding handling via HA MQTT `camera_topic` wildcards and optional
  `ring_snapshot_url_template` (HTTP snapshot / HA camera_proxy). See `docs/ring-mqtt.md`.
- Per-plugin always-on-top video windows, disabled by default, and exclusions for
  individual legacy camera sources.
- Live Home Assistant camera previews from configured MQTT URL topics.
- Daily grid import and export in kWh beside the current tariff, using the
  controller's physical Victron meter readings. Partial days show their start
  time, and unavailable or stale readings remain unknown.

### Fixed
- Reject undersized RSA and ECDSA keys throughout the Apple-validated TLS
  certificate chain, including trust anchors omitted by the server.
- Desktop tariff exports use the system save dialog, avoiding a macOS WebKit
  download hang. Cancelling the export leaves the tariff draft unchanged.
- Camera previews start with less repeated work and discard stale queued live-preview frames.
- Configuration restore no longer leaks subscriptions or lets an older load
  overwrite newly restored settings. Plugin status refreshes coalesce event bursts.
- Plugin restoration avoids repeatedly verifying every installed package for
  each declaration while retaining integrity checks for the selected package.
- Local macOS updates stage the replacement application before stopping the old
  one and restore the previous bundle if replacement fails.
- The tariff spreadsheet uses matching Univer 1.0.2 packages, fixing builds that
  mixed incompatible Facade types. Dependabot now updates the Univer family together.
- The tariff in the daily statistics strip inherits the surrounding typography
  and stays inline with the other values.

### Changed

- Update both Tauri API layers to 2.12.2, ZIP packaging to 9.0.0, and the compatible frontend dependencies together.
- Refresh quinn-proto to 0.11.18 in the application and Home Assistant worker locks, closing the known peer-triggered panic and memory-exhaustion advisories.
- Update Lucide icons and Prettier. Remove the unused direct Undici development
  dependency; jsdom keeps its compatible, patched Undici 7.29.1 dependency.

### Upgrade
- Windows HTTPS integrations require certificate chains with RSA keys of at
  least 2048 bits or ECDSA keys of at least 256 bits and SHA-2 signatures.
  Apple platform-verified connections require the same minimum key sizes,
  including the trust anchor. Replace undersized private CA keys or legacy
  signatures before upgrading; certificate trust and hostname verification
  remain enabled.
- Local authentication passwords migrate once to Argon2id verifiers while keeping
  the same login. This storage change is one-way: older app versions cannot
  unlock migrated profiles. Portable settings exports do not include credentials;
  see `docs/desktop-hardening.md` before intentionally downgrading.
- Application updates preserve the existing configuration, cached encryption key,
  plugin version pins and enabled/disabled choices. Camera packages remain managed
  separately through the Plugins settings.
- Keep the previous application bundle for rollback. Updating the app does not
  require exporting and importing configuration or enabling camera plugins.

### Maintenance

- Gate release candidates on bounded camera-topic, live-URL, and worker-frame fuzz campaigns, retaining source-bound validation evidence.
- Publish reviewed release notes from the exact source commit used to build each candidate, preserving build provenance.
- Document contribution checks, confidential security reporting and the project-specific trust boundaries.

### Security
- Backport the upstream GLib `VariantStrIter` pointer-mutability fix for
  RUSTSEC-2024-0429 (GHSA-wrw7-89jp-8q8g) while retaining Tauri's GLib 0.18 API.
  An optimized regression reproduces the original crash and passes the fix.
  This is part of the unreleased development line, not a retroactive claim about
  previously published packages.
- Remove six obsolete Cargo advisory ignores after verifying that the locked
  rustls-webpki and quick-xml versions already contain their upstream fixes.
- Apply explicit strong-sign policy to Windows' native certificate-chain
  builder for plugin downloads, media transfers and Home Assistant HTTP. Earlier
  native verification could accept weak intermediate and trusted-root keys.
- Replace reversible local login passwords with salted Argon2id verifiers,
  keep them out of settings IPC and exports, and atomically migrate the native
  configuration before granting access. Outbound service credentials are unchanged.
- Verify the official Gradle 8.14.3 wrapper and pinned distribution checksum before and after Android project generation.

Private vulnerability reporting and response policy are documented in SECURITY.md. This maintenance update strengthens release evidence and review instructions; it does not replace deployment authentication, network isolation or independent equipment safeguards. No new project CVE is announced by these changes.

## [2.5.41] - 2026-09-12

### Fixed
- Android ARM64 and x86_64 builds align both load segments and RELRO boundaries
  for 16 KB memory pages, preventing failures caused by the earlier ARM64 boundary.

### Added
- A manual Google Play upload-bundle workflow with a separate upload key, complete
  signature verification, and native alignment checks. Existing GitHub APK signing
  identity is preserved; the workflow does not submit the app to Google Play.

## [2.5.40] - 2026-09-12

### Fixed
- Installed iOS bundle and build versions now follow the app release version
  instead of the hardcoded `1.0.0` value.

## [2.5.39] - 2026-09-12

### Fixed
- Clearing the MQTT host with IGW disabled now stops both inverter transports and
  clears their displayed telemetry. Home Assistant camera MQTT remains independent.
- Pending startup, reconnect and configuration operations cannot revive a removed
  inverter connection; callbacks from the previous session cannot overwrite new data.

## [2.5.31] - 2026-09-08

### Fixed
- Water pump/valve buttons now publish GX MQTT-API `W/<portal>/pump/<n>/Mode`
  (Cerbo `{"value": mode}`) instead of the unused `inverter/cmd/water_mode_set`
  topic; Mode read-back updates AUTO chips (#369).

### Added
- Manual pump/valve buttons in the Water card, routed through dbus-pump's
  writable `/Mode` (0 auto, 1 always-on, 2 always-off) via the GX MQTT-API
  (`W/<portal>/pump/<n>/Mode`) - still no direct Home Assistant control.
  Opening the city valve asks for confirmation; an AUTO chip appears while a
  device is under manual override to hand it back to dbus-pump automation.
- dbus-pump fix required on the GX: its `/Mode` onchange handler must return
  true, otherwise vedbus rejects the write and the mode silently stays `auto`
  (deployed as dbus-pump main).

### Changed
- EV section now sources data from Cerbo MQTT (dbus-ev / dbus-evcharger) instead of
  Home Assistant. Subscribes to `N/<portal>/ev/<instance>/Soc` (%),
  `N/<portal>/ev/<instance>/Ac/Power` (W), and
  `N/<portal>/evcharger/<instance>/Ac/Power` (W).
  New config fields: `evcharger_instance` (default 40) and `ev_instance` (default 22).
  The `ha_ev_soc_entity`, `ha_ev_charging_entity`, and `ha_ev_clamp_entity` config
  fields are removed. EV section visibility no longer requires HA direct API — it shows
  when Cerbo MQTT is connected and at least one EV metric is live.

## [2.5.0] - 2026-08-24

### Added
- Portal ID auto-discovery: subscribes to the retained `inverter/portal`
  topic published by inverter-control and arms water/alarm subscriptions and
  the GX keepalive automatically - `portal_id` is now optional in app config.

### Changed
- Water section now uses **only** Cerbo GX MQTT (dbus-pump): the Home Assistant
  entity fallback (`ha_water_level_entity` / `ha_valve_switch_entity` /
  `ha_pump_switch_entity`) is removed, along with its config fields
- Water card shows level in % plus pump/valve status badges; toggle buttons
  removed — pump/valve automation lives in dbus-pump

## [2.4.4] - 2026-08-23

### Added
- Solar forecast display (#252) and forecast brackets in DailyStats (#251)
- Yesterday solar production (#241)
- Washer/dryer START/PAUSE buttons from HA button entities (#247)
- Persistent notification banner + Victron alarm watcher (#244)

### Fixed
- Tray icon keeps updating after poisoned lock, panic, or sleep/wake (#255)
- Solar breakdown now adds up to headline total (#254)
- Existing config/about windows brought to front on reopen (#253)

### Security
- Bump glob override to 11.1.0 (CVE-2025-64756) (#250)

## [2.4.3] - unreleased

### Fixed
- Various fixes and dependency updates

## [2.2.2] - 2026-07-20

### Fixed

- CI build failure: remove empty Apple code signing env vars causing `SecKeychainItemImport` error

# Desktop plugin implementation roadmap

This checklist tracks optional, separately installed features in inverter-desktop.
Check an item only after demonstrating its acceptance criteria. Source separation,
passing unit tests, an installable package, and verified device behavior are
different milestones. Update this file as implementation progresses.

The completed resilience checklist is preserved in
[docs/archive/data-resilience-todo.md](docs/archive/data-resilience-todo.md).

## Product boundaries

- **Core on every platform:** Victron/Cerbo telemetry, MQTT and IGW source
  selection, inverter-control MQTT flags and dashboard controls, batteries, solar,
  grid, daily statistics, Cerbo EV/charger and water/pump controls, authentication,
  core notifications, configuration, and app updates.
- **Desktop plugins:** Home Assistant and cameras. Inverter controls must never
  require HA. Direct Frigate/Kerberos/Ring MQTT camera use must not require HA;
  HA proxy support is an optional adapter.
- **Android and iOS:** core only. Do not compile, bundle, initialize, or offer a
  plugin manager, workers, HA integration, or camera integration. No mobile plugin
  store, deferred download, or preinstalled mobile version of these features.
  This is a build boundary, not a runtime visibility switch.
- An absent desktop plugin contributes no feature code, assets, connections,
  subscriptions, settings, or actions to a clean core installation.
- Disabling stops plugin work. Uninstalling also removes its package. Neither
  operation may stop or reconnect the core MQTT/IGW transport.
- Preserve daemon flag keys, topics, absolute command semantics, and saved core
  controls. HA remains another consumer of the same MQTT contract.
- Preserve legacy feature configuration without automatically installing plugins.
  Installation remains an explicit desktop choice.

## Delivery strategy and current checkpoint

The first checkpoint establishes core/mobile build boundaries and separates core
from desktop feature contributions. Desktop retains bundled HA/camera behavior
during this compatibility stage. This is **not** completion of installable plugins.

The intended package is a signed first-party `.idplugin` archive: a versioned
manifest, a target-specific executable worker, and declarative UI contributions.
Use a bounded, versioned JSON protocol instead of Rust dynamic-library ABI or
runtime loading of Tauri crates. Prove the worker contract before shipping the
installer. Native workers have the user's OS privileges; broker permissions are
not an OS sandbox. Arbitrary executable UI is outside the initial package contract.

### 0. Baseline and ownership

- [x] Establish inverter-control ownership of seven MQTT flags and metadata;
      preserve legacy target aliases (inverter-control PR #210).
- [x] Separate dashboard controls and action dispatch from `useHA` (desktop PR #408).
- [x] Record desktop-only plugins and the core-only mobile product.
- [x] Preserve the previous completed TODO in the documentation archive.
- [x] Record exact checks, commit, and PR for the first implementation checkpoint.

### 1. Frontend core and desktop feature boundaries — implemented

- [x] Define a build-selected contribution port with matching TypeScript contracts;
      the mobile implementation must import no desktop implementation.
- [x] Keep one shared core dashboard, authentication flow, telemetry model, charts,
      EV/water controls, and configuration persistence path.
- [x] Move HA appliance/entity cards out of the shared side panel. Keep core
      inverter controls visible when HA is absent.
- [x] Move HA and camera settings out of core settings; retain Cerbo tank/pump/EV
      instance selectors in core settings.
- [x] Extract camera broker connection, listeners, retries, and shutdown from
      core MQTT/IGW lifecycle; prevent duplicates after configuration reloads.
- [x] Extract HA/camera setup, status, notifications, camera actions, media routes,
      and feature translations behind the desktop boundary.
- [x] Keep core flag presets independent from HA discovery. Preserved HA controls
      must not become misleading mobile buttons when their implementation is absent.
- [x] Preserve desktop behavior and saved field names during this checkpoint;
      do not advertise package installation as available yet.

Acceptance: the same core dashboard builds for desktop/mobile. The mobile module
graph has no HA/camera implementation, feature UI, or plugin manager. Mobile does
not invoke desktop commands or subscribe to feature events.

### 2. Native mobile exclusion — implemented

- [x] Compile HA REST/WS/session implementations only for desktop.
- [x] Compile camera parsing, subscriptions, downloads, windows, and cache handling
      only for desktop; extract camera parsing from core MQTT.
- [x] Remove HA/camera commands, state, startup tasks, and authorization entries
      from mobile instead of registering no-op handlers.
- [x] Move feature-only dependencies into desktop target declarations. Retain
      HTTP/MQTT/authentication dependencies still needed by core.
- [x] Preserve legacy configuration as passive data on mobile without activating
      connections, accessing feature credentials for requests, or deleting settings.
- [x] Restrict feature window capabilities and asset scopes to desktop.
- [x] Prevent configuration imports, feature flags, or frontend overrides from
      opting mobile back into the plugin ecosystem.

Acceptance: real iOS/Android targets compile with core commands only; flags retain
their MQTT route. Desktop regression checks remain green.

### 3. Platform builds and regression gates — in progress

- [x] Select frontend platform from explicit intent and Tauri target environment,
      never host OS; reject conflicting desktop/mobile overrides.
- [x] Provide a reproducible core-only mobile frontend build command.
- [x] Apply selection to Tauri builds, Android CI, the iOS direct-cargo helper,
      release builds, and Play builds.
- [x] Verify the actual frontend module graph; fail mobile builds that include
      desktop implementation instead of relying on minified-string searches.
- [ ] Check actual native compilation inputs, dependencies, commands, and packaged
      libraries alongside target builds and capability checks.
- [x] Cover absent features, preserved core controls, and cross-platform saved
      configuration round trips.
- [ ] Run frontend tests/typecheck/build/format, Rust tests/clippy/format,
      release contracts, and Android/iOS CI on the integration commit.

Acceptance: deliberately including a desktop module or native command makes a
mobile gate fail. A successful desktop build is insufficient evidence.

### 4. Versioned desktop host and worker protocol

- [ ] Define host API version independently from app/package versions.
- [ ] Specify manifest schema, stable ID, package version, host API range, target
      triple, entrypoint, config schema, permissions, file sizes/digests, and signature.
- [ ] Define request/response/events, correlation IDs, deadlines, maximum frame
      sizes, cancellation, startup handshake, and error codes.
- [ ] Implement desktop-only start/stop/restart supervision, exit monitoring,
      bounded retries/backoff, resource limits, and deterministic app shutdown.
- [ ] Render generic dashboard/settings/status/notification/media contributions;
      keep HA entity types out of host contracts.
- [ ] Authorize host operations by plugin identity and app session, with scoped
      access to its own settings/secrets, permitted origins/topics, and owned files.
- [ ] Keep inverter writes in core; grant neither arbitrary Tauri invocation nor
      arbitrary inverter commands to workers by default.
- [ ] Exercise a separate fixture worker through the actual protocol: start,
      contribute, act, stop, crash, and recover without affecting core telemetry.
- [ ] Verify multiple app windows neither duplicate workers nor bypass authority.

Acceptance: a separately built worker completes the lifecycle through the real
host. A fake registry or statically linked crate does not satisfy this milestone.

### 5. Installation, update, rollback, and removal

- [ ] Produce deterministic `.idplugin` archives for supported macOS, Linux, and
      Windows architectures.
- [ ] Verify publisher signature, API range, target, schema, complete file
      inventory, sizes, and digests before executing package content.
- [ ] Reject traversal, absolute paths, links, duplicate/case-colliding entries,
      oversized archives, unexpected files, and unsupported schemas.
- [ ] Stage privately and atomically activate immutable versions after validation
      and a successful startup handshake; retain a working rollback version.
- [ ] Serialize install/update/remove operations and recover after interruption.
- [ ] Add desktop install/enable/disable/update/uninstall UI with compatibility
      errors, declared permissions, and restart requirements.
- [ ] Separate installed package inventory from feature settings and secrets.
- [ ] Stop before removing: cancel work, remove contributions, close owned windows,
      and clean owned temporary files.
- [ ] Offer retention/deletion of plugin settings on uninstall; preserve core data.
- [ ] Test clean install, update, failed activation, rollback, and uninstall with
      actual produced archives.

Acceptance: a clean core installation contains no HA/camera payloads. Installing
or removing a package changes available features without reinstalling the app.

### 6. Cameras package

- [ ] Move camera MQTT client and Frigate/Kerberos/Ring adapters to the worker,
      separately from core Cerbo MQTT and source selection.
- [ ] Preserve deduplication/cooldowns, snapshots, configured topics, reconnect,
      clip behavior, and window stacking.
- [ ] Move URL resolution, download validation, clip ownership/cancellation,
      temporary-file cleanup, settings, translations, and media contributions.
- [ ] Add optional origin-scoped HA proxy/name enrichment; verify direct camera
      transports with no HA plugin installed.
- [ ] Test camera-only install, broker recovery, changed settings, disable during
      download, crashes, and uninstall while media is open.
- [ ] Remove bundled camera implementation after installable parity is verified.

### 7. Home Assistant package

- [ ] Move REST/WS supervision, discovery/filtering, connection status, and
      household-device actions into the HA worker.
- [ ] Preserve initial/live state, reconnect, visibility refresh, grace/unavailable
      behavior, and cancellation when configuration changes.
- [ ] Move appliance/entity UI, settings, translations, and assets into package
      contributions.
- [ ] Retain all seven inverter flags, metadata, saved controls, and MQTT routing
      in core, including `input_boolean.<flag>` aliases.
- [ ] Verify HA-only install, real HA IDs resembling flag names, service scopes,
      worker restart, and removal during requests.
- [ ] Remove bundled HA implementation after installable parity is verified.

### 8. Configuration migration and compatibility

- [ ] Introduce versioned `modules.ha` and `modules.cameras` with idempotent legacy
      migrations and explicit migration versions.
- [ ] Preserve unknown namespaces during core/mobile load/save/export/import and
      resets that do not explicitly delete plugin data.
- [ ] Separate core flags from genuine HA targets in `ha_entities` and
      `header_toggles_config`; preserve labels, order, IDs, and state keys.
- [ ] Migrate secrets through encrypted storage; never put them in manifests,
      logs, diagnostics, process arguments, or package contents.
- [ ] Offer explicit desktop installation for preserved unavailable features;
      do not infer installation consent from existing configuration.
- [ ] Test desktop → core mobile → desktop round trips, missing plugins,
      downgrade/rollback, partial migration, and failed activation.

### 9. Release acceptance and operational verification

- [ ] Verify desktop core / core+HA / core+cameras / both using actual installed
      files and behavior, including clean profiles.
- [ ] Inspect final APK/AAB/IPA files and native libraries for feature absence;
      module-graph checks complement packaged-artifact inspection.
- [ ] Test app updates with installed plugins, incompatible API versions,
      interrupted updates, rollback, and offline startup.
- [ ] Integrate package signing/provenance, compatibility metadata, and release
      receipts into existing release gates.
- [ ] Measure installed size, startup, CPU/RAM, and connections per combination;
      do not promise savings based on source size.
- [ ] Verify real desktop HA/camera installations and representative mobile
      devices separately from mocks. Record exact tested versions and limits.
- [ ] Update user/developer docs and remove compatibility-stage statements only
      after the corresponding implementation and delivery path are complete.
- [ ] Deliver reviewed PRs and merge after required checks pass.

## Evidence log

- Baseline: desktop `c51fad8` (PR #408), daemon `84999d8` (PR #210).
- Working branch: `feat/desktop-plugin-modules`, isolated from other local work.
- First implementation commit: `c805d833`; review and integration:
  [PR #411](https://github.com/victron-venus/inverter-desktop/pull/411).
- Initial validation: `pnpm test:build-profiles` passed four tests, including an
  actual Vite build that deliberately imports forbidden desktop code. The real
  `pnpm build:mobile` passed Vue typecheck/build and emitted a core-only source
  graph in `dist/build-profile.json`.
- Native artifact verifier: 17 fixture tests passed, including rejection of
  arbitrary Cargo manifests and option-like targets; real mobile archives remain
  subject to their build/upload gates.
- Local integration: 192 desktop/core frontend tests and 7 mobile tests passed;
  both profile typechecks/builds and formatting passed. Rust: 171 full-suite tests,
  5 final auth-focused tests, clippy, and iOS simulator/Android aarch64
  `cargo check --locked --all-features` passed. Lint completed with warnings.
- Release helper: all four iOS build-order/failure fixtures passed after updating
  their expected command to `build:mobile`; 12 version/native-package tests passed.
  The complete local release contract suite also passed all 178 tests.
  Hosted integration and mobile artifact gates are still pending.
- Hosted iOS at `c805d833`: the native release, IPA packaging, and packaged
  exclusion verifier passed in
  [Quality gate run 34802901966](https://github.com/victron-venus/inverter-desktop/actions/runs/34802901966).
  Android packaging and the final reviewed integration head remain pending.
- Review fixes: desktop fields emit model updates into the existing reactive
  draft; tests cover real settings save/reset and both defaults import orders.
  Data-only defaults remove a configuration/UI initialization cycle introduced
  by the extraction. Both frontend builds and all frontend tests passed again.
- Remaining validation boundary: no physical inverter commands, installed HA/camera
  deployment, or real mobile device exercise was performed.
- Append PR links and final hosted gate results here.

## Reference constraints

- [Tauri sidecars](https://v2.tauri.app/develop/sidecar/): target-specific worker
  delivery does not itself provide our installer.
- [Tauri capabilities](https://v2.tauri.app/security/capabilities/): preserve
  explicit window and command authority across plugin surfaces.
- [Cargo target dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#platform-specific-dependencies):
  follow compilation targets, including cross-builds.

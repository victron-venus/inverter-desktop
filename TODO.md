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
  grid/submeter telemetry, daemon setpoint override, daily statistics,
  Cerbo EV/charger and water/pump controls, authentication,
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

The first checkpoint established core/mobile build boundaries and separated core
from desktop feature contributions. The second added a versioned worker protocol,
real process supervision, and declarative dashboard contributions. The third
implemented deterministic signed archives, publisher/content verification, and
transactional native package APIs exercised with real workers.

[PR #422](https://github.com/victron-venus/inverter-desktop/pull/422) completed
application integration and the package manager on main, including hosted mobile
artifact checks. The next checkpoint adds isolated encrypted plugin settings,
a declarative desktop editor, and verified worker startup configuration.

**The embedded production publisher policy is still empty, so installation is
disabled in the shipped configuration.** This checkpoint does not introduce new
publisher keys or app signing. Existing HA/camera implementations remain bundled
desktop features until real package parity is verified. Network/media host services,
legacy configuration migration, and feature extraction remain unfinished.

### Current checkpoint: isolated settings and worker configuration

- [x] Add a versioned, encrypted record per plugin ID outside package contents;
      reuse the existing application encryption key with a separate authenticated
      encryption domain. Missing records must not touch the OS credential store.
- [x] Bound records, retained data, field counts, input sizes, filesystem entries,
      and temporary transactions; reject unsafe links and corrupt ciphertext.
- [x] Compile a flat declarative schema subset with typed ordinary values and
      write-only string secrets. Validate defaults, required fields, enums, and
      bounds natively; reject unsupported schema features.
- [x] Return only ordinary values and secret-presence flags to the settings UI.
      Support explicit secret replacement/clear and preserve omitted secrets.
      Prevent an updated schema from exposing a previously secret field.
- [x] Bind saves to both archive identity and data revision; preserve unknown
      stored fields through updates/rollback and reject stale editors.
- [x] Stage encrypted writes before the auth commit, serialize them with package
      lifecycle operations, survive IPC cancellation, and reject stale epochs.
- [x] Send configuration only to a verified worker declaring configuration
      permission; require an exact startup acknowledgment before accepting data.
      Keep secrets out of argv, environment, runtime snapshots, and host logging.
- [x] Save before restarting an enabled worker; keep disabled workers stopped
      and distinguish successful persistence from failed runtime activation.
- [x] Add a desktop-only typed settings editor with English/Russian messages,
      secret draft cleanup, auth/lifecycle invalidation, and cross-window races.
- [x] Offer explicit settings deletion on uninstall, defaulting to retention;
      deletion must remain within the same authorized package operation.
- [x] Verify schema/storage failure cases, real worker startup/save/restart,
      auth races, UI flows, and mobile absence. Keep heavy local checks serialized.
- [x] Update protocol/package documentation and complete independent reviews of
      storage, schema/application integration, protocol/lifecycle, UI, and mobile
      boundaries. Address the review findings with focused regressions.

- [x] Update rustls to 0.23.45 and its required crypto dependencies for
      RUSTSEC-2026-0285; preserve advisory enforcement and verify Cargo Deny.

Local validation: 323 macOS native tests and strict Clippy, 252 frontend tests, 8 mobile frontend
checks, 4 build-profile checks, and 22 native mobile-boundary tests passed. Both
frontend production builds passed; final desktop output is restored. Hosted
Linux/Windows/iOS/Android checks and final APK/AAB/IPA inspection must pass on the
PR's current head before merging; the PR records live delivery status. This
checkpoint does not establish real HA/camera package or physical-device parity.

### Completed checkpoint: application integration and management UI

- [x] Open one private package store per application, retaining its exclusive
      lease across authenticated sessions and releasing it after worker cleanup.
- [x] Restore previously enabled packages only in the current authenticated
      session; reverify each package, surface failures, and keep disabled packages
      stopped. A queued restore must not undo an explicit disable/uninstall.
- [x] Add settings-window-only lifecycle IPC. Select package files through a
      native dialog; accept no webview-supplied archive or executable path or key.
- [x] Keep one bounded, expiring native preview bound to its originating window
      and authentication epoch. Verify target/API compatibility, show verified
      identity, versions, target, publisher, and declared capabilities, and commit
      the exact reviewed archive bytes. Invalidate stale or replaced consent.
- [x] Add a desktop-only Plugins settings panel with inventory, runtime status,
      install/update review, enable/disable, rollback, uninstall confirmation,
      readable errors, and English/Russian messages.
- [x] Report an empty publisher policy accurately; provide no unsigned, fixture
      key, environment, or user-supplied trust fallback in application builds.
- [x] Share native inventory and worker state across windows without reloading
      core configuration or reconnecting MQTT/IGW during package operations.
      Cache inventory metadata by revision and coalesce UI snapshot requests.
- [x] Connect logout, expiry, policy-change, and normal-quit cleanup to the package
      service: revoke previews and workers and retain the store lease until owned
      operations and process cleanup finish.
- [x] Exclude native management commands, frontend manager code/messages, package
      store startup, and management routes from Android/iOS. Verify the frontend
      boundary with mobile tests and the actual production module graph and JS.
- [x] Exercise the initial native application service with real signed archives
      and workers, including review/install file replacement, restoration, auth
      races, cancellation, initialization/quit ordering, and empty trust.
- [x] Verify review/install/update/enable/disable/rollback/uninstall UI workflows,
      escaped metadata and errors, busy states, stale asynchronous responses,
      listener cleanup, and bounded refreshes. Build both frontend profiles.
- [x] Document the implemented checkpoint and remaining delivery limits.
- [x] Complete the operation-activity/expiry regression and pass the full macOS
      native/frontend suites, typecheck, formatting, lint, and strict Clippy after
      the integration fixes. Keep target builds and hosted checks separate.
- [x] Pass local native `cargo check --locked --all-features` for the iOS
      simulator and Android aarch64 targets.
- [x] Bind advertised actions to their original session and reject stale calls
      before enqueueing to a replacement worker; pass the real-worker regression.

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

### 3. Platform builds and regression gates — implemented and verified

- [x] Select frontend platform from explicit intent and Tauri target environment,
      never host OS; reject conflicting desktop/mobile overrides.
- [x] Provide a reproducible core-only mobile frontend build command.
- [x] Apply selection to Tauri builds, Android CI, the iOS direct-cargo helper,
      release builds, and Play builds.
- [x] Verify the actual frontend module graph; fail mobile builds that include
      desktop implementation instead of relying on minified-string searches.
- [x] Check actual native compilation inputs, dependencies, commands, and packaged
      libraries alongside target builds and capability checks.
- [x] Cover absent features, preserved core controls, and cross-platform saved
      configuration round trips.
- [x] Run frontend tests/typecheck/build/format, Rust tests/clippy/format,
      release contracts, and Android/iOS CI on the integration commit.

Acceptance: deliberately including a desktop module or native command makes a
mobile gate fail. A successful desktop build is insufficient evidence.

### 4. Versioned desktop host and worker protocol — runtime checkpoint implemented

- [x] Define host API version independently from app/package versions.
- [x] Specify manifest schema, stable ID, package version, host API range, target
      triple, entrypoint, config schema, permissions, file sizes/digests, and
      signature encoding. Package trust and cryptographic verification remain
      phase 5 requirements.
- [x] Define request/response/events, correlation IDs, deadlines, maximum frame
      sizes, cancellation, startup handshake, and error codes.
- [x] Implement desktop-only start/stop/restart supervision, exit monitoring,
      bounded retries/backoff, queue/process/message limits, and normal app shutdown.
- [x] Render generic text, metric, status, and preset-action dashboard contributions
      without HA entity types, executable UI, or remote navigation.
- [ ] Extend contributions to settings, notifications, and owned media surfaces.
- [x] Limit worker IPC to authenticated dashboard/settings windows and advertised
      action parameters. Revoke old session epochs on logout/policy change/expiry;
      reject stale queued writes, contributions, and action results after re-login.
- [ ] Authorize scoped host services for plugin settings/secrets, permitted
      origins/topics, and owned files. Manifest permission metadata alone grants
      none of these services.
- [x] Keep inverter writes in core; expose no arbitrary Tauri invocation or
      core MQTT publishing operation to workers.
- [x] Exercise a separately compiled fixture executable through actual pipes:
      start, contribute, act, cancel, stop, crash, and recover. The standalone
      host has no MQTT/IGW handles or core transport dependency.
- [x] Share one native registry across windows; verify duplicate registration,
      window authority, revocation, and repeated quit waiting for cleanup.
- [ ] Verify installed-plugin recovery alongside active MQTT/IGW telemetry and
      real multiwindow interaction during final operational acceptance.

Acceptance for the runtime checkpoint: a separately built worker completes the
lifecycle through the actual host. The original executable proof used trusted native test/development code. The
application now constructs workers from verified installed packages and restores
only previously enabled packages after authentication; there is still no
executable-path IPC or automatic installation. Complete phase 4 acceptance also requires the remaining contribution
surfaces and scoped host services above. A fake registry or statically linked
feature implementation does not satisfy worker lifecycle verification.

### 5. Installation, update, rollback, and removal — application/UI checkpoint

- [x] Implement a deterministic, bounded `.idplugin` format and a native packaging
      CLI that signs actual file inventories with an externally supplied key.
- [ ] Produce and publish real worker archives for supported macOS, Linux, and
      Windows architectures; a host-platform fixture does not establish delivery
      or native signing on every desktop target.
- [x] Verify publisher signature, API range, target, schema, complete file
      inventory, sizes, and digests before executing package content.
- [x] Reject traversal, absolute paths, links, duplicate/case-colliding entries,
      oversized archives, unexpected files, and unsupported schemas.
- [x] Stage privately and atomically activate immutable versions after validation
      and a successful startup handshake; retain a working rollback version.
- [x] Serialize native install/update/remove operations; recover interrupted
      initialization, staging, and inventory changes without executing workers.
- [x] Bind asynchronous activation to its original authentication epoch and
      release worker registry capacity only after process reaping.
- [x] Embed a release-owned publisher policy scoped to exact plugin IDs; reject
      unknown keys and never trust a key supplied by the package or webview.
- [ ] Configure real production publisher keys and signing provenance in a
      reviewed release. Disposable fixture keys must never become shipped trust.
- [x] Add desktop install/enable/disable/update/uninstall UI with verified review,
      compatibility errors, declared capabilities, rollback target, and immediate
      application of changes without an app restart.
- [x] Connect the native package manager to authenticated application startup,
      settings, and shutdown without automatically installing legacy features.
- [x] Separate installed package inventory from feature settings and secrets.
- [x] Stop before removing package files: cancel actions, remove contributions,
      confirm process reaping, and release the worker registry entry.
- [ ] Close owned windows and clean owned media/temporary files when those host
      services are introduced.
- [x] Offer retention/deletion of plugin settings on uninstall; preserve core data.
- [ ] Add a retained-data inventory and explicit cleanup for uninstalled or
      unavailable packages before end-user release, including quota recovery.
- [x] Test native clean install, update, failed activation, rollback, and uninstall
      with actual signed archives and executable workers.

Acceptance: a clean core installation contains no HA/camera payloads. Installing
or removing a package changes available features without reinstalling the app.
Native lifecycle and desktop application/UI integration are implemented. Full
phase acceptance still requires final integration checks, production trust and
worker distribution, remaining host services, and migrated HA/camera packages.

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
- [x] Inspect final APK/AAB/IPA files and native libraries for feature absence;
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
- Native artifact verifier: 18 fixture tests passed, including rejection of
  arbitrary Cargo manifests, option-like targets, and missing core commands. Real mobile archives also
  passed the build/upload gates recorded below.
- Local integration: 199 desktop/core frontend tests and 8 mobile tests passed;
  both profile typechecks/builds and formatting passed. Rust: 175 full-suite tests,
  clippy, and iOS simulator/Android aarch64
  `cargo check --locked --all-features` passed. Lint completed with warnings.
- Release helper: all four iOS build-order/failure fixtures passed after updating
  their expected command to `build:mobile`; 12 version/native-package tests passed.
  After integrating main `8ecb573` (release tooling PR #410) without application
  changes, the complete local release contract suite passed all 180 tests.
- Hosted integration at `2b1bd9e6`: all checks passed, including `CI gate`,
  SonarCloud, CodeQL, frontend/Rust tests, and dependency checks.
  [Quality gate run 34803663596](https://github.com/victron-venus/inverter-desktop/actions/runs/34803663596)
  built the iOS IPA and universal Android APK/AAB, verified Android native
  alignment, and inspected each package for desktop feature absence.
  Verified targets: `aarch64-apple-ios`, `aarch64-linux-android`,
  `armv7-linux-androideabi`, `i686-linux-android`, and `x86_64-linux-android`.
  The PR requires the same gates on its final integration head before merging
  to main, including the additional core-command checks described below.
- Main integration at `0b0ae9d` (PR #409): grid submeter telemetry and daemon
  setpoint override remain shared core. Both override commands are registered
  in mobile handlers and covered by authentication checks. The mobile widget
  test covers start/stop and readback; packaged native verification now rejects
  missing override commands as well as forbidden HA/camera implementation.
- Review fixes: desktop fields emit model updates into the existing reactive
  draft; tests cover real settings save/reset and both defaults import orders.
  Data-only defaults remove a configuration/UI initialization cycle introduced
  by the extraction. Both frontend builds and all frontend tests passed again.
- Remaining validation boundary: no physical inverter commands, installed HA/camera
  deployment, or real mobile device exercise was performed.

### Worker host checkpoint

- Baseline: merged boundary PR #411, main `563a6dd`.
- Branch: `feat/desktop-plugin-worker`, isolated from the canonical checkout.
- Protocol: eight isolated contract tests passed, including incompatible versions,
  identities, unsafe metadata, unknown operations, and oversized/deep frames.
- Runtime: 13 tests passed against the production host, including a separately
  compiled Rust fixture and deterministic pipe-backpressure tests. Covered
  cancellation/deadlines/revocation, late results, restart exhaustion, startup
  failure, stderr draining, environment isolation, forced stop, and process reaping.
- Frontend: all 212 tests and eight mobile tests passed. Both profile builds,
  typechecks, formatting, and lint passed (repository lint retains warnings).
- Full native integration: all 198 macOS Rust tests passed, including the
  protocol/runtime/bridge checks, and strict clippy passed. iOS simulator and
  Android aarch64 `cargo check --locked --all-features` passed. The existing
  local HTTP fixture required localhost permission; no working service was used.
- Mobile guards reject both worker IPC commands and the fixed host event;
  all 18 packaged-verifier fixtures passed. Final Android/iOS archive checks
  passed in [PR #420](https://github.com/victron-venus/inverter-desktop/pull/420):
  [quality gate 34809280336](https://github.com/victron-venus/inverter-desktop/actions/runs/34809280336)
  verified the IPA and APK/AAB for integration head `82bd964`; merged as `5b80aaa`.
  Hosted Rust passed 195 Linux tests and strict clippy.
- Scope: no installed plugin package, signature verification, HA/camera migration,
  physical inverter write, or real-device deployment is claimed by this checkpoint.

### Native package checkpoint

- Baseline: merged worker-host PR #420, main `5b80aaa`.
- Branch: `feat/desktop-plugin-packages`, isolated from the canonical checkout.
- Acceptance requires actual signed archives and executable worker lifecycle tests,
  including tampering, unknown publishers, wrong target/API, failed activation,
  rollback, interrupted work, disabled startup, and removal after process reaping.
- The compiled publisher policy starts empty. No production key is invented or
  inferred from archive contents; configuring release trust remains unchecked.
- At the native API checkpoint, desktop still kept an empty worker registry;
  application/UI integration is recorded separately below. HA/camera migration and
  final operational acceptance remain open.
- Package verification: 19 tests passed with real Ed25519 signatures, canonical
  manifests, ZIP structure/CRC rejection, exact publisher scope, and file integrity.
- Packaging: 11 local tests passed. The actual CLI produced byte-identical archives
  containing a compiled worker; Python `cryptography` independently verified the
  Ed25519 signature, and Python `zipfile` checked CRC and inventory digests.
  Existing output and accidental private-seed inclusion were rejected.
- Native lifecycle: 21 focused tests passed, including real cross-process locking,
  failed activation and rollback, authentication epochs, caller cancellation,
  corruption, initialization recovery, and file-count/disk quota preflight.
- Integration: all 256 macOS Rust tests and strict all-targets clippy passed after
  integrating main `8c995d7` (Frigate progressive clip streaming). Hosted
  Windows/Linux/Android/iOS gates were still required at that local validation stage.
- Frontend/core: all 212 desktop tests and eight mobile tests passed, along with
  typecheck/build, formatting, and lint (existing lint warnings remain).
- Mobile boundary verifier: all 20 fixtures passed, including the new package
  dependency and embedded publisher-policy rejection checks.
- Mobile compilation: iOS simulator and Android aarch64
  `cargo check --locked --all-features` passed; both frontend profile builds passed.
- Hosted CI follow-up: update signature decoding for the newer Clippy rule and
  embed Tauri's existing Common Controls v6 dependency in Windows test/example
  executables as well as the application. The initial Windows run compiled but
  its loader exited before tests; successful compilation alone is not acceptance.
- Windows portability follow-up: admit regular-file modes emitted by Windows ZIP
  tooling without admitting links or special files, pin producer creator metadata
  across operating systems, and preserve raw `..` components in traversal fixtures
  instead of letting Windows verbatim-path joining normalize them first.
- After the portability fixes, all 77 local plugin tests, strict all-targets
  clippy, formatting, and the independent real-CLI signature/archive checks passed.

### Application and manager checkpoint

- Baseline: merged native package PR #421, main `0c27c54`.
- Branch: `feat/desktop-plugin-manager`, isolated from the canonical checkout.
- Initial native validation: all 94 plugin tests passed, including application
  service tests with actual signed archives and executable workers. This precedes
  the final operation-activity/expiry regression and full native integration run.
- Frontend: 35 focused tests passed (17 manager, 13 dashboard, five desktop
  configuration). Coverage includes native selection cancellation, verified review,
  explicit install/update, declared capabilities, enable/disable, named rollback,
  concrete uninstall consent, auth/teardown races, and 100-event refresh bursts.
- Eight mobile tests passed, including the empty manager contribution and absence
  of its settings tab. Both frontend production builds passed; inspection of the
  mobile receipt and generated JavaScript found no manager module, lifecycle IPC
  names, or manager labels. Desktop output includes the manager.
- Full local integration: all 276 macOS native tests and all 229 desktop frontend
  tests passed. Strict Clippy, frontend typecheck, formatting, and Biome passed;
  existing repository lint warnings/information remain. Native metadata caching
  and one-in-flight UI refresh avoid repeated verification per worker event;
  launch verification remains mandatory.
- Browser visual smoke exercised the actual manager component with disposable
  mocked native IPC: English/light verified update to 2.0.0 and disable, plus
  Russian/dark empty-publisher policy with installation disabled. The fixture and
  server were removed afterward. This does not test the real native file-dialog GUI.
- Local iOS simulator and Android aarch64 native
  `cargo check --locked --all-features` passed.
- Final action/session fix: all 96 plugin tests passed, including a delayed
  old-session crash action rejected before reaching the replacement worker;
  a new-session echo action succeeded. The other 181 native tests are unchanged.
- Implementation commit: `b0e6dbd`. Hosted APK/AAB/IPA validation, exact-commit
  review/check results, and merge status are recorded in
  [PR #422](https://github.com/victron-venus/inverter-desktop/pull/422).
  Local results do not prove production publisher provisioning, migrated HA/camera
  packages, physical inverter commands, or real-device behavior.

## Reference constraints

- [Tauri sidecars](https://v2.tauri.app/develop/sidecar/): target-specific worker
  delivery does not itself provide our installer.
- [Tauri capabilities](https://v2.tauri.app/security/capabilities/): preserve
  explicit window and command authority across plugin surfaces.
- [Cargo target dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#platform-specific-dependencies):
  follow compilation targets, including cross-builds.

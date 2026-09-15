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
application integration and the package manager. [PR #423](https://github.com/victron-venus/inverter-desktop/pull/423)
added isolated encrypted settings, a declarative editor, and verified worker
startup configuration on main. Both checkpoints passed hosted mobile artifact
checks. [PR #424](https://github.com/victron-venus/inverter-desktop/pull/424) adds
inventory and cleanup for retained plugin data on main. [PR #425](https://github.com/victron-venus/inverter-desktop/pull/425)
completed the standalone Frigate motion worker on main at `5254610`, with all
hosted worker/host/security/mobile artifact checks passing for
[the final PR head](https://github.com/victron-venus/inverter-desktop/commit/ed3aadd5f27260c6e4334c018d016d36f2154837).
[PR #426](https://github.com/victron-venus/inverter-desktop/pull/426) completed direct
Frigate clips and owned video windows on main, with macOS native playback and all
hosted checks passing. [PR #427](https://github.com/victron-venus/inverter-desktop/pull/427)
completed the independent read-only HA worker on main at `3d731a3`.
[PR #428](https://github.com/victron-venus/inverter-desktop/pull/428) adds explicitly
selected HA button/scene actions and instance-bound dashboard dispatch on main
at `f91598c`. All 41 executed checks passed for final head `c13489d`, including
Android/iOS artifact boundaries; review was approved with no open threads.
[PR #429](https://github.com/victron-venus/inverter-desktop/pull/429) added selected
media Play/Pause/Stop on main at `7e5ad18`. Its final head `3d1a188` passed all 41
executed checks, with review approved and no open threads.
[PR #431](https://github.com/victron-venus/inverter-desktop/pull/431) added explicit
HA on/off controls on main at `ae73fa0`. Its final head `7d75afd` passed all 41
executed checks, with review approved and no open threads. The current separate
iteration adds bounded numeric inputs for selected HA numbers and cover positions.
[PR #434](https://github.com/victron-venus/inverter-desktop/pull/434) completed cover
Open/Close/Stop on main at `c614f07`; all 41 executed checks passed for `f55ace8`,
with exact-head approval and no unresolved review threads.

**The embedded production publisher policy is still empty, so installation is
disabled in the shipped configuration.** This checkpoint does not introduce new
publisher keys or app signing. Existing HA/camera implementations remain bundled
desktop features until real package parity is verified. Network/media host services,
legacy configuration migration, and feature extraction remain unfinished.

### Completed checkpoint: Frigate motion worker and native notifications

This iteration delivers a separately built first-party worker for Frigate MQTT
motion-start notifications and connection status. It does not claim camera feature
parity: snapshots, completed clips, media windows, Kerberos/Ring, optional HA proxy
support, and legacy camera migration remain later work. The bundled desktop
camera implementation remains available during that transition.

- [x] Extend host API to 1.2 with an explicit verified `desktop_notifications`
      permission and bounded plain-text notifications; preserve wire/manifest v1.
- [x] Bound notification queues, rates, identity history, and age; reject malformed,
      unauthorized, or premature messages, and drop excess valid traffic safely.
- [x] Submit notifications only for the current running worker generation and
      authenticated session. Reject queued work after disable, removal, logout,
      expiry, restart, or shutdown; keep event text out of UI snapshots and logs.
- [x] Add synchronous native OS submission with literal text handling. Keep the
      native plugin path out of Android/iOS and preserve core notifications.
- [x] Build an independent desktop-only Rust Frigate worker, with its own locked
      dependency graph, bounded stdin/stdout protocol, and no core/Tauri imports.
- [x] Require configuration acknowledgment before connecting to MQTT. Accept one
      explicit broker and exact topic, isolated write-only credentials, optional
      certificate-verified TLS, and no core control subscriptions or publishing.
- [x] Parse Frigate motion starts, preserve the 45-second per-camera cooldown and
      ten-minute event deduplication, bound incoming data/state, and ignore retained
      replay. Report generic connection status and recover after broker restarts.
- [x] Make shutdown/EOF cancel reconnect and network activity promptly, including
      when the peer stops reading stdout. Keep credentials out of diagnostics.
- [x] Add a target-specific manifest template and deterministic staging helper;
      use the existing archive encoder and disposable test signing keys only.
- [x] Retain inspected file/directory identities while staging, reject replacement
      before creating output, and compare Windows path/handle timestamps using
      matching semantics. Cover the reproduced races and metadata regression.
- [x] Exercise the actual packaged worker against a private loopback Mosquitto
      broker: subscription isolation, motion, duplicates, cooldown, reconnect,
      changed settings, disable, logout, and uninstall.
- [x] Add worker Linux/macOS/Windows build/test/lint checks, dependency audit, and
      an explicitly required real-broker acceptance check to hosted CI.
- [x] Extend mobile dependency/source/archive checks to reject worker code,
      executable payloads, package assets, and notification host services.
- [x] Complete independent code reviews, focused regressions, native/frontend
      validation, and both production frontend builds with serialized local load.
- [x] Update English documentation and this checklist with demonstrated local
      results. Final hosted checks and merge are recorded separately below.

No production publisher keys are introduced. Package authentication is separate
from desktop application signing; this work adds no requirement to sign or
notarize the desktop application or worker executable. Operating-system display
permission and notification-center behavior require platform runtime verification.

Local validation passed: 353 macOS native tests across all targets and strict
Clippy, plus the explicitly selected real signed-package/Mosquitto acceptance
(1 passed, 0 ignored). The ordinary native suite leaves that one broker test
ignored; CI selects it separately and rejects zero-test success. Worker validation
passed 9 unit tests, 6 subprocess/TCP tests, strict Clippy, formatting, debug/release builds,
and a refreshed dependency audit without advisory exceptions. The actual macOS
release binary passed staging/header/hash validation and the real signed-package/
Mosquitto acceptance. Frontend checks passed 277 tests
(32 manager tests), 8 mobile tests, 4 profile checks, typecheck, formatting, lint,
and both builds; final output is desktop. Packaging/mobile Python suites passed
19 and 25 tests respectively, with Pylint 10/10 and no scoped frontend diagnostics.

Independent reviews found and fixed strict Shutdown decoding, notification-service
liveness, UI signal coalescing, and repeated macOS backend initialization. All
regressions passed. Final-head hosted worker/host/security/mobile artifact checks
and review passed, and PR #425 merged into main. Local native collection is not proof
of OS display; TLS process checks cover ClientHello/no plaintext fallback, not a
complete certificate-trust/hostname fixture. No physical camera or inverter command
was exercised. The PR records final hosted status separately from this completed
source/local acceptance checklist.

CI review reproduced a staging input replacement race and a Windows Python 3.12
path-stat/fstat timestamp mismatch. The helper now preserves the original file
and directory identities, validates before creating output, and compares creation
timestamps across Windows APIs while retaining full open-handle change checks.
Older Windows Python uses its matching creation-time fallback. Packaging tests
now run in all three desktop worker jobs, in addition to actual release staging.

### Current checkpoint: bounded HA numeric inputs

Start from the verified PR #434 merge at `c614f07`. Add one declarative numeric
input to the desktop host and use it for explicitly selected `number.*` values
and cover positions. Existing fixed-action presets retain their exact parameter
authorization. This is an independent PR and does not introduce plugin code,
assets, imports or runtime capabilities into Android/iOS.

- [x] Define host API 1.5 and a `number_input` contribution with bounded identity,
      title, label, optional unit, `input_revision`, observed `value_scaled`,
      `min_scaled`, `max_scaled`, `step_scaled` and `decimal_places`. Keep wire and
      manifest schema version 1 and compatibility with older ^1.3/^1.4 workers.
- [x] Use exact scaled integers with absolute coefficients at most 10^15 and
      decimal precision 0..6. Require minimum <= maximum, positive step, and
      in-range, grid-aligned observed and requested values. Reject malformed,
      unsafe, nonfinite or unrepresentable numeric data; preserve state cards
      when an entity cannot offer a valid numeric input.
- [x] Authorize exactly `{input_revision, value_scaled}` for numeric inputs.
      Preserve fixed Action preset equality and reject numeric action-ID
      collisions with every actionable contribution. Validate current numeric
      authority at frontend preflight, host enqueue, host control dispatch and
      immediately before the first pipe byte; retain authentication epochs,
      worker-instance binding, cancellation and original deadlines.
- [x] Invalidate queued numeric grants on withdrawal/restoration or changed
      constraints, even if a worker reuses its revision. Prove this with an
      actually delayed writer and retain all existing static-action behavior.
- [x] Render a numeric field and explicit Apply using desktop-only code. Parse
      decimal drafts exactly; invalid or empty drafts send nothing. Keep drafts,
      observed state and request results separate. Preserve dirty drafts during
      ordinary HA updates and require review of changed constraints.
- [x] Key numeric pending/error state by plugin, instance and action ID rather
      than draft value or revision. Capability changes must not unlock another
      pending write or erase an unknown result. Test real component/composable
      behavior, duplicate clicks, stale instances, changed bounds and restoration.
- [x] Add separate optional `number_entities` and `cover_position_entities`, each
      empty by default and limited to four unique literal IDs. Append watched
      targets after existing selections, preserving IDs and ordered deduplication.
      Existing `cover_entities` must not gain position-write authority implicitly.
- [x] Reserve one control slot per numeric input within the combined 31-control,
      32-watched-entity and 64-contribution limits. Preserve previously accepted
      configurations and their exact serialized 32 KiB boundary when new fields
      are omitted or empty. Exercise actual maximum encoded frames.
- [x] Require current-session number state and valid `min`, `max`, `step` and
      optional unit metadata. Derive only `number/set_value` with a literal
      `entity_id` and a validated JSON numeric `value`.
- [x] Preserve the native startup envelope at the exact previous 32 KiB boundary,
      not just worker reserialization. Add explicit `omitEmpty` schema semantics
      only for optional public default-empty strings; preserve effective editor
      values, use defaults instead of redundant stored keys, and retain all prior
      schemas' delivery behavior. Opt in only the two new HA numeric fields and
      test exact storage/envelope boundaries, upgrade/save, nonempty delivery and
      invalid schemas.
- [x] Require a known cover state, integer `current_position` in 0..100 and
      unsigned `supported_features` bit 4. Use fixed bounds 0..100, step 1 and
      `cover/set_cover_position` with only literal `entity_id` and `position`.
      Keep Open/Close/Stop and their existing capability rules unchanged.
- [x] Rotate worker input revisions when bounds, precision, unit or eligibility
      change, including revocation/restoration and new sessions. Ordinary
      observed-value updates must not invalidate an edited draft. Recheck the
      published revision and current constraints before admitting a request.
- [x] Keep the shared two-request limit, verified TLS, no redirects/retries,
      original deadlines, cancellation, authentication rejection and private
      unknown-outcome feedback. Service results never synthesize device state;
      cancellation never sends a cover Stop command.
- [x] Add worker unit and actual-process acceptance for decimal/grid boundaries,
      exact bodies/routes, read-only defaults, malformed metadata, stale revisions,
      capability changes, prior actions and shared executor behavior.
- [x] Add installed-release-package acceptance using private HA/MQTT services:
      exact numeric writes, server-confirmed state, stale parameters/instances,
      changed constraints, settings replacement and stalled-command teardown.
      Keep both core charger-flag states live and observe zero core commands.
- [x] Update host/worker/package metadata and English documentation, including
      unsupported precision and remaining discovery/household UI limitations.
      Preserve the empty publisher policy and existing app-signing boundaries.
- [x] Run relevant frontend, native, worker, packaging and mobile tests, strict
      lint, fresh frontend/release builds and staging with serialized local load.
      Complete independent source, authorization and acceptance reviews.
- [ ] Push the separate PR, resolve comments in English, pass final-head CI and
      synchronize clean main after the authorized merge.

Local acceptance passes: 51 HA worker unit tests, 56 action subprocess tests,
16 read/protocol subprocess tests and three macOS TLS scenarios. The shared
protocol's 10 tests and Frigate's 25 tests pass. Strict all-target Clippy passes
for the host and all three worker crates. HA 0.6 and Frigate release workers
build separately; HA 0.6 stages with the final manifest.

All 410 ordinary native tests pass. All eight explicitly selected installed
release-package scenarios pass with zero failures or ignored selections: two
Frigate and six HA. The numeric scenario checks ten exact service POSTs,
HA-confirmed observations, revised constraints, revoked/restored capabilities,
stale instances and teardown during stalled requests. Independent core MQTT
telemetry stays live for both charger-flag states with zero core commands.

All 366 frontend tests, 17 mobile tests and five profile tests pass. Fresh desktop
and mobile builds include typechecking. A focused 17-test rerun passes after a
test-only lint correction. Project Biome succeeds with the unchanged baseline
of 79 warnings and 84 informational diagnostics; changed plugin files have no
lint diagnostics. Prettier checks pass. Packaging and native mobile-boundary
suites pass 27 and 29 tests, and packaging Pylint rates 10/10.
All applicable pre-commit hooks pass without skip overrides.
The branch also incorporates PR #435 on main at `0962bd7`; all 366 frontend
tests, 17 mobile tests, both typechecked builds, lint and formatting pass again
on the combined tree.

Four new native settings regressions bring that suite to 13 passing tests.
They exercise the exact 32 KiB startup envelope and real encrypted settings
storage through upgrade, editor save and reread. Empty numeric selections no
longer add 50 bytes to an existing envelope or redundant keys to storage;
one-byte overflow still fails. Other schemas retain default delivery semantics.
Independent host, frontend, worker, JSON parsing, settings, installed-fixture and
documentation reviews have no open findings. Hosted checks and the merge receipt
remain pending for this separate PR.

Discovery, appliance/weather presentation, other camera adapters, legacy
configuration migration, removal of bundled desktop integrations and production
package delivery remain separate work. Fixtures do not establish behavior of
physical HA devices or existing household automations.

### Completed checkpoint: explicit HA cover controls

Extend the independently installed HA package with opt-in Open, Close and Stop
for selected `cover.*` entities. Start from the verified PR #431 merge on main
at `ae73fa0`. Use the existing immutable action-button contract and host API 1.4;
arbitrary position, tilt, speed and numeric input remain a separate UI increment.

- [x] Add optional `cover_entities`, empty by default, with at most four unique
      literal `cover.*` IDs. Append watched targets after existing binary
      selections, preserving previous indices, deduplication order and accepted
      configurations, including the serialized 32 KiB boundary.
- [x] Reserve three action slots per selected cover within the combined
      31-action and 32-watched-entity limits, independently of current device
      capabilities. Prove actual maximum contribution count and encoded size.
- [x] Publish immutable `ha-cover-<index>-open/close/stop` presets with `{}`.
      Resolve only fixed `cover/open_cover`, `cover/close_cover` and
      `cover/stop_cover` routes, with literal `entity_id` bodies. Reject arbitrary
      service, target, position, tilt and speed parameters and core aliases.
- [x] Require the current connected session, an exact observed `open`, `closed`,
      `opening` or `closing` state, and the operation's unsigned integer
      `supported_features` bit (Open 1, Close 2, Stop 8). Missing or malformed
      attributes grant no commands. Other bits grant no additional commands.
- [x] Publish capability-only changes even when displayed state/title is
      unchanged; immediately recheck current capabilities at action admission.
      Clear eligibility on missing/deleted/unavailable state or disconnection.
- [x] Preserve HA-confirmed state, original deadlines, cancellation, verified
      TLS, no redirects/retries, authentication rejection and shared two-request
      concurrency. Cancellation must not issue Stop. Stop is an explicit action
      with the same admission and concurrency rules as other commands.
- [x] Add unit and actual-process acceptance for exact routes, immutable params,
      stable IDs, feature-only updates, malformed observations, moving states,
      read-only defaults, all configuration bounds and shared executor behavior.
- [x] Exercise the actual installed release package with private HA/MQTT
      services: exact writes, server-confirmed state, capability revocation,
      stale-instance rejection, settings replacement and stalled-command
      teardown. Keep core charger-flag telemetry live and observe zero commands.
- [x] Update package version/manifest, packaging contracts and English docs.
      Preserve mobile compile/bundle exclusions and the empty publisher policy.
- [x] Run relevant tests, strict lint, fresh frontend/release builds and staging
      with serialized local load; complete independent subagent reviews.
- [x] Push a separate PR, resolve comments in English, pass final-head CI and
      synchronize clean main after the authorized merge.

Local acceptance passes: 39 worker unit tests, 44 action subprocess tests, 16
read/protocol subprocess tests and three macOS TLS scenarios. Strict worker and
native all-target Clippy pass. The actual HA 0.5 release worker builds and stages;
all seven separately selected installed-package scenarios pass with actual
release workers (two Frigate, five HA), with zero failures or ignored selections.
The cover fixture verifies seven exact admitted POSTs, capability-only revocation,
HA-confirmed state, stale-instance/preset rejection and pending-request teardown.
Core charger-flag telemetry remains live in both states, with zero MQTT commands.

Packaging and mobile boundary suites pass 27 and 29 tests; packaging Pylint is
10/10. A fresh desktop frontend build includes typechecking. Frontend sources
are unchanged from the tested main base; project Biome succeeds with the same
79 warnings and 84 informational diagnostics. All applicable pre-commit hooks
pass without skip overrides. Independent production, process,
installed-fixture, CI-selection and documentation reviews found no open issues.
Configuration tests cover all 3,825 action-count combinations and preserve old
IDs and the exact serialized 32 KiB boundary. Real encoder checks cover maximum
64-item frames, including the single-cover case with more long state values.

PR #434 merged as `c614f07` after all 41 executed checks passed for `f55ace8`,
with exact-head approval and no unresolved review threads. Canonical main was
verified clean with the identical tested tree; six stashes and the private project
guide were preserved. All 18 merged-main checks subsequently passed.

Physical HA devices and actual scheduled automations remain separate acceptance
work. This iteration does not complete arbitrary cover positioning, numeric
inputs, discovery, appliance/weather presentation, remaining camera adapters,
legacy configuration migration or replacement of bundled desktop features.

### Completed checkpoint: explicit HA on/off controls

Extend the independently installed HA package with explicit Turn on and Turn off
for selected `switch.*`, `input_boolean.*` and `light.*` entities. Keep observed
state separate from command submission and preserve core inverter-control MQTT
ownership. No generic toggle, arbitrary service parameters or lighting options
are introduced. Start from the verified PR #429 merge on main at `7e5ad18`.
The branch also incorporates PR #430 on main at `723c41e`; the combined native,
frontend, mobile and installed-package paths have been revalidated.

- [x] Add optional `binary_entities`, empty by default, with at most eight unique
      literal IDs from the three supported domains. Append new watch targets
      after existing watch/button/scene/media selections; preserve existing IDs
      and all previously accepted configurations.
- [x] Preserve the 32-entity watch limit and bound combined action buttons to 31:
      one per button/scene, three per media player and two per on/off target.
      Exercise the maximum 64 contributions through the actual bounded encoder.
- [x] Publish immutable `ha-binary-<index>-on/off` presets with empty parameters.
      Derive fixed domain-specific `turn_on`/`turn_off` routes and literal entity
      bodies from validated configuration. Reject arbitrary domains, targets,
      parameters, core aliases and MQTT fallback.
- [x] Offer and admit commands only for exact observed `on` or `off` state in the
      current connected session. Withdraw missing/deleted/unknown/unavailable or
      malformed targets. Keep both absolute commands available for either state.
      A service response must never synthesize a new entity state.
- [x] Retain original deadlines, cancellation, two-request concurrency, verified
      TLS, no redirects/retries, authentication rejection and unknown-outcome
      feedback. Preserve existing button/scene/media behavior.
- [x] Add actual-worker process acceptance for all six routes, read-only
      defaults, configuration bounds, state confirmation, literal flag-like IDs,
      invalid parameters and withdrawn actions.
- [x] Exercise the actual installed release package with private HA/MQTT
      services: exact writes, state confirmation, stale-instance rejection,
      settings replacement and stalled-command teardown. Keep the independent
      core charger flag live in both states and observe zero MQTT commands.
- [x] Update version/manifest, packaging contracts and English documentation.
      Preserve mobile exclusions and the empty production publisher policy.
- [x] Run relevant tests, lint, fresh frontend/release builds and staging with
      serialized local load; complete independent subagent reviews.
- [x] Push a separate PR, resolve comments in English, pass final-head CI and
      synchronize clean main after the authorized merge.

Local acceptance passed: 32 unit tests, 37 action subprocess tests, 16
read/protocol subprocess tests and three macOS TLS scenarios. Worker and native
all-target Clippy pass. The default native suite passes 398 tests; all six
separately selected installed-package scenarios pass with actual release workers
(two Frigate, four HA). The binary fixture verifies ten exact admitted POSTs,
HA-confirmed state, stale-instance/preset rejection, stalled-command teardown,
independent charger-flag telemetry in both states and zero core MQTT commands.

The actual HA 0.4 release worker builds and stages successfully. Packaging and
mobile boundary suites pass 27 and 29 tests, with packaging Pylint 10/10.
The combined frontend passes 311 tests, 17 mobile tests and five build-profile
checks. Both frontend builds include typechecking; the final output is desktop.
Ten Android local-build script tests also pass. Project Biome exits successfully
with 79 warnings and 84 informational diagnostics in frontend sources identical
to the merged main base. Independent source, fixture, CI-selection and
compatibility reviews found no open issues. Old configurations retain the exact
32 KiB validation boundary when the new field is omitted or empty. Real encoded
63/64-contribution cases preserve the 64 KiB frame limit with escaped bounded
fields and valid binary states. No application signing or publisher keys are added.

One Windows CI run failed the retained empty-body video retry scenario while the
parallel check at the same head passed. Its assertion hid the transfer-error
category, so the original cause is not established. The test server now reads
bounded header chunks instead of one socket operation per byte, success assertions
report only safe error enums, and the retry scenario checks exact served bytes
and complete file cleanup. Transfer deadlines, retry counts and production code
are unchanged. The exact scenario, all 16 media tests and strict native Clippy
pass locally. Both final Windows runs pass. PR #431 merged as `ae73fa0` after all
41 executed checks passed for `7d75afd`, with exact-head approval and no unresolved
review threads. The canonical checkout is clean on main with the identical tested
tree; all six stashes and the private project guide are preserved. The original
Windows failure cause remains unproven.

Physical HA devices, actual scheduled automations, number/cover inputs, discovery,
appliance/weather presentation, remaining camera adapters and legacy migration
remain separate acceptance work.

### Completed checkpoint: explicit HA media-player transport

Extend the independent HA package with opt-in Play/Pause/Stop for literal
`media_player.*` targets. Reuse the existing declarative actions and host API 1.4;
keep core inverter-control flags on their existing MQTT path. PR #429 merged as
`7e5ad18` with an identical tested tree and a clean synchronized canonical main.

- [x] Add optional `media_player_entities`, empty by default, limited to four
      unique literal media-player IDs. Preserve the combined 32-entity watch limit
      and existing button/scene action indices and defaults.
- [x] Advertise three immutable empty-preset actions for each connected, observed
      player. Withdraw actions for missing, deleted, unknown or unavailable state.
      Resolve Play/Pause/Stop to fixed `media_play`, `media_pause` and `media_stop`
      services entirely inside the worker, with the selected entity as the only
      request-body field.
- [x] Reuse bounded concurrency, original deadlines, cancellation, verified TLS,
      prefix handling, authentication rejection and unknown-outcome feedback.
      Preserve the no-retry rule and prohibit arbitrary service names or MQTT
      fallback. Check maximum combined contribution count and encoded frame size.
- [x] Exercise actual worker subprocesses for defaults, literal targets, exact
      methods/paths/body, lifecycle availability, rejected caller parameters,
      duplicate limits, no retries and independence of existing button/scene IDs.
- [x] Exercise the actual installed signed package with private HA/MQTT services
      and disposable trust. Prove exact media POSTs, stale-instance rejection,
      teardown during a stalled command and absence of core command-topic traffic.
- [x] Update package version/manifest, packaging contracts and English docs.
      Preserve mobile exclusion and the empty production publisher policy.
- [x] Run serialized relevant tests, lint, release staging and all installed-package
      scenarios. Review with subagents and mark only verified work complete.
- [x] Push a separate PR, resolve comments in English, pass final-head CI and
      synchronize clean main after the authorized merge.

Local worker validation passes 25 unit tests and strict all-target Clippy. The
maximum mixed snapshot exercises the real output encoder with 61 contributions
and worst-case escaped bounded fields. Packaging and mobile boundaries pass 27
and 29 tests; packaging Pylint scores 10/10. All 29 action subprocess scenarios
pass, including nine media cases; 16 read/protocol and three macOS TLS scenarios
also pass. The actual HA 0.3 release worker builds and passes native staging.
All five installed-package scenarios pass with actual release workers: two
Frigate and three HA scenarios. The media fixture proves eight exact admitted
POSTs, stale-instance/parameter rejection, teardown of stalled commands and no
core MQTT commands while independent charger-flag telemetry remains live.

The first combined local run had startup failures in the existing Frigate clip
and HA read-only scenarios. Both exact reruns and the subsequent combined
five-scenario run passed with unchanged deadlines and binaries; no root cause
was established. Test-only installer diagnostics now retain the host-owned state
and error code if this recurs, without worker output or credentials. The initial
failures are not described as a fixed production defect.

No production HA or physical media player is invoked by automated fixtures.
Stateful home toggles, number/cover inputs, discovery, weather/appliance layout
and legacy configuration migration remain later parity work.

### Completed checkpoint: explicit HA button and scene actions

Keep the default HA package read-only. Add explicitly selected `button.press` and
`scene.turn_on` actions through existing declarative plugin buttons, preserving
the legacy POST mappings without importing core flag aliases or MQTT fallbacks.
The read-only worker was delivered in PR #427; this iteration builds on it.

- [x] Add optional `action_entities` (empty by default), at most 16 unique literal
      `button.*`/`scene.*` IDs. Watch the ordered union with `watch_entities`, at
      most 32 entities total; read selection alone never enables an action.
- [x] Add strict bounded Action/Cancel decoding to the shared stdio protocol.
      Preserve independent EOF/Shutdown delivery and Frigate rejection of actions.
- [x] Advertise one fixed, self-contained action per configured target only while
      HA is connected and the target exists and is not unavailable. Unknown
      button/scene state before first use remains actionable. Use immutable empty
      params; resolve the entity and fixed service exclusively inside the worker.
- [x] Send exactly one authenticated, TLS-verified, non-redirecting service POST
      beneath the configured URL prefix. Bound concurrency, response bytes and
      original request deadlines; never retry a submitted service operation.
- [x] Keep live reads and heartbeat responsive during pending actions. Correlate
      success/error, reject changed params/unadvertised actions, handle Cancel,
      and prevent queued work after disconnect/auth rejection/settings restart.
      Cancellation cannot undo an operation already accepted by HA.
- [x] Bind native dashboard clicks to an opaque actual worker-instance identity,
      including reinstall with reused generation counters. Preserve exact preset
      equality and reject stale clicks before the replacement worker sees them.
- [x] Forward only the remaining original deadline after host queue pressure,
      preserving request identity and params. Exercise the real bounded pipe
      writer, submillisecond expiry and cancellation during partial frames.
- [x] Reject stale clicked descriptors rather than substituting new parameters.
      Scope pending/error feedback to the actual instance and action identity;
      coalesce snapshot refreshes and explain unknown service outcomes without
      encouraging automatic retries. Verify accessible action names.
- [x] Exercise real subprocess service behavior, no-retry after lost responses,
      deadline/cancel/EOF/output pressure, read-only defaults and literal targets
      against disposable HA fixtures. Preserve all read/TLS/Frigate regressions.
- [x] Exercise an actual installed signed package, native action admission,
      configuration replacement during a stalled service request and removal,
      alongside independent MQTT flags. Use disposable trust and private services.
- [x] Update package version/manifest, English documentation and this checklist;
      run serialized lint/tests/builds and desktop/mobile boundaries.
- [x] Resolve PR comments in English, pass final-head CI, merge and synchronize main.

Local admission checks pass 36 native runtime tests, including reinstall with
reused generation counters, reduced budgets after queue pressure and expiry
before pipe delivery. Frontend changes pass 58 focused tests (26 dashboard,
32 manager), typecheck, scoped lint and formatting. The shared protocol passes
nine unit tests and strict Clippy. The action worker passes 21 unit tests and
20 real subprocess scenarios, including the reviewed late-poll deadline regression
and repeated cancellation/deadline acceptance. Packaging and mobile-native
boundary suites pass 27 and 29 tests. The complete frontend suite passes 308 tests,
plus eight mobile tests and five build-profile checks. The complete native suite
passes 384 tests. All four separately selected installed-package scenarios pass:
Frigate events/clips and HA reads/actions, using actual release worker binaries,
disposable package trust and private services. The new action fixture confirms
exactly six explicit service POSTs, closes stalled work during settings replacement,
disable and uninstall, rejects stale instances/presets, and observes zero core MQTT
commands while independently consuming both values of the real charger flag.

Strict all-target Clippy passes for the host and all worker crates. Both frontend
profiles build, and actual native release workers pass staging checks. The first
full native compilation exhausted local disk space; after removing only task
intermediate build files, the complete native and installed-package runs passed.
Sonar review follow-up uses an explicit total code-unit comparator and covers
distinct composed/decomposed Unicode keys. Final head `c13489d` passed all 41
executed GitHub checks (three intentional skips), including native/package,
desktop workers on three operating systems, security/dependency checks and both
mobile artifacts. PR #428 was approved with no unresolved threads and merged as
`f91598c`. Canonical main is clean with an identical tested tree; private files
and all six existing stashes were preserved.

No production HA service or physical appliance is exercised by these fixtures.
Stateful toggles, number/cover inputs, media controls, discovery, full appliance
layout and legacy migration remain subsequent parity work.

### Completed checkpoint: standalone Home Assistant read-only worker

Implement the first independently installable HA slice: connection status and
live state for an explicit, bounded list of entities. Preserve the bundled HA
integration until its remaining UI/service/configuration behavior has package
parity. Core inverter-control flags and transports never depend on this worker.

- [x] Extract only the common bounded stdio framing/output primitives into a
      desktop worker library. Keep identity, configuration validation, and network
      behavior in each worker; preserve Frigate handshake/EOF/backpressure behavior.
- [x] Add a separate `inverter-desktop.home-assistant` worker, package manifest,
      independent locked build, and explicit HTTP(S) base URL configuration.
      Deliver its HA token only through the encrypted plugin-secret mechanism.
- [x] Support a bounded explicit entity watch list and render plain-text/metric
      dashboard contributions with connection and unavailable states. Treat HA
      text as data and limit all frames, state fields, maps, queues, and rates.
- [x] Acknowledge validated configuration before any network request. Implement
      authenticated initial state plus WebSocket state changes, heartbeat,
      bounded reconnect, cancellation, and token rejection without logging secrets.
- [x] Reconcile initial reads and live updates without overwriting a newer live
      state with an older REST response. Keep subscription/output backpressure
      from growing memory or blocking heartbeat/shutdown indefinitely.
- [x] Limit this slice to reads. Add no HA service invocation, arbitrary HTTP
      proxy, core MQTT subscription/publish, inverter flag alias, or core config
      lookup. Preserve actual HA entity IDs even when their names resemble flags.
- [x] Verify URL prefix/port/TLS behavior and no redirects/credential forwarding;
      use an explicit read-only fake HA server for process and installed-package
      acceptance. Never connect to a user's production HA during these fixtures.
- [x] Exercise the actual executable through pipes: configuration before network,
      auth success/rejection, initial/live/unavailable states, reconnect, malformed
      and oversized traffic, stalled peers, output pressure, EOF, and shutdown.
- [x] Exercise a real signed HA worker package and temporary HA fixture through
      install, settings restart, disable, logout, and uninstall, while an
      independent core telemetry connection remains usable. Disposable keys only.
- [x] Extend package staging, independent mobile source/module/dependency/archive
      guards, and Linux/macOS/Windows build/test/lint/audit CI for both workers and
      the shared worker library. Keep Android/iOS entirely outside this ecosystem.
- [x] Rebuild the real Frigate worker after extraction and rerun its unit/process
      and signed-package MQTT/clip acceptance; common transport reuse must preserve
      existing behavior rather than rely only on compilation.
- [x] Complete independent reviews, serialized local verification, English docs
      and checklist updates. Add a simultaneous HA-on/core-off regression.
- [x] Pass final-head hosted checks and resolve review comments, then merge the
      separate HA worker PR and synchronize canonical main after PR #426.

Local checks now pass strict all-target Clippy and 14 HA unit tests, 16 actual
subprocess/network scenarios, and three macOS TLS subprocess tests. TLS establishes
untrusted-certificate rejection, authenticated WSS with a child-only temporary CA,
and continued HTTPS OS-verifier rejection of that CA. Successful selected HTTPS
reads with the fixture CA are Linux-specific CI coverage, not a local macOS claim.
Shared transport passes seven tests; Frigate passes 15 unit and ten process tests
after extraction. All three independent advisory audits pass without exceptions.
The frontend passes five build-profile checks (including real imports outside
`src`), eight mobile tests, typecheck, formatting, lint and both production builds.
The retained packaging suite passes 27 tests and mobile payload checks pass 29.
Native host Clippy and its 381-test default suite pass; the three external-package
tests also pass when selected explicitly with actual release workers (3 passed,
0 ignored). The HA fixture uses
the real `do_not_supply_charger` MQTT flag, alternates both boolean values, and
reads an HA `input_boolean.do_not_supply_charger` literally without core aliasing.
The simultaneous collision probe confirms HA remains on while core is off.
Both real release workers and native-header staging pass, including the retained
Frigate staging CLI. The installed-package fixtures verify HA settings restart,
disable, logout and uninstall alongside an independent core MQTT connection,
and preserve Frigate motion/clip behavior after transport extraction. Hosted
checks and reviewed PR delivery completed at head `dabb0f1`, merged as `3d731a3`
in PR #427. All product checks passed, including desktop worker/host checks and
Android APK/AAB/iOS IPA inspection; review had no unresolved threads. Canonical
main was synchronized cleanly with the identical tested tree, preserved private
files and all six stashes. These results do not claim a production HA installation. Hosted Android setup exposed an unavailable `tools`
package in the pinned setup action default. All three Android CI/release setup
steps now request `platform-tools` explicitly and retain their subsequent required
SDK/NDK installation, build and artifact-inspection gates.

The production publisher policy stays empty and this iteration creates no
production keys or application-signing prerequisite. HA services, full legacy feature extraction,
configuration migration, and Linux/Windows native camera playback remain tracked
work. Kerberos producer URL/auth/origin requirements still need source or runtime
evidence; the inspected tracked repositories currently establish consumers only.

### Completed checkpoint: Frigate clips in owned desktop windows

Deliver one actual feature slice: Frigate `end` with boolean `has_clip: true`
opens its downloaded MP4 in a plugin-owned window. Reuse the existing complete-
download-before-playback behavior; incremental HTTP receipt is not streaming
playback. Keep this implementation separate from the current motion checkpoint.

- [x] Add optional direct `frigate_base_url` configuration; no base URL means
      motion notifications continue and clip handling remains inactive. Preserve
      explicit ports and reverse-proxy path prefixes; encode event IDs as one
      URL path segment and reject unsupported URL forms.
- [x] Parse completed events with separate ten-minute ID history and 45-second
      camera cooldown. A start notification must not consume a later clip; clip
      notification IDs need a distinct namespace. Do not reject long recordings
      using the motion parser's start-time freshness cutoff.
- [x] Implement the existing clip-available notification and automatic opening
      after complete download. Record dedupe when the event is admitted, matching
      current behavior, and keep MQTT polling responsive during downloads.
- [x] Add a versioned, permission-checked HTTP-video operation and native media
      ownership keyed by plugin, authentication epoch, worker generation, and
      opaque media ID. Use a fresh revocable instance identity for every spawn:
      generation numbers alone can repeat after removal and registration. Workers
      never choose host paths or native window routes.
- [x] Extract reusable HTTP transfer policy without importing core HA credential
      lookup. Bind allowed HTTP(S) origins and path prefixes to the installed
      plugin's non-secret configuration; accept no HTTP credentials or injected
      headers in this slice and keep URLs out of logs.
- [x] Preserve eight attempts, retry delays 1/2/3/4/5/5/5 seconds, 15-second connect,
      60-second idle-read, ten-minute overall deadline, 256 MiB limit, and no
      redirects. Retry empty/progressive-body failures and currently retryable
      HTTP statuses; reject oversized bodies immediately.
- [x] Bound concurrent transfers, queued requests, windows, and aggregate media
      storage. Write private host-owned files, clean partial writes and startup
      leftovers, and avoid holding runtime/auth locks during HTTP or disk I/O.
- [x] Cancel downloads, revoke media access, and close owned windows on disable,
      settings restart, crash/restart, update, rollback, logout/expiry, uninstall,
      and app shutdown. Reject late completion before window creation and clean
      files if window construction fails.
- [x] Serve completed files through an opaque, requesting-window-bound media
      route with bounded byte-range responses. Do not broaden the existing global
      temporary-directory asset scope or reuse URL-based webview media IPC.
- [x] Implement the built-in player and 330x186 unfocused borderless windows, top-right
      stacking/reflow, muted autoplay, and close-on-completion. Each clip gets its
      own window; closing one must preserve sibling media and core telemetry.
- [x] Exercise the actual release worker in a signed package with private Mosquitto
      and HTTP fixtures: start then end with an old recording timestamp, duplicate
      clips, exact URL prefix and ranges, settings restart, disable with ready media,
      revocation during a stalled body, and uninstall. Keep an independent core
      MQTT telemetry connection alive across those lifecycle operations. Native
      window creation/destruction is simulated in this backend acceptance test.
- [x] Cover transfer failures and service bounds separately with HTTP/service tests:
      delayed availability, empty/truncated bodies, progressive receipt, idle stalls,
      oversized bodies, redirects, total deadline, cancellation, byte ranges,
      storage/queue/window limits, orphan recovery, and failed cleanup retries.
- [x] Add component tests for opaque IDs, exact window ownership, muted inline
      autoplay, owned close/drag commands, safe errors, and legacy-path rejection.
- [x] Add an explicit, feature-gated [native media smoke harness](docs/native-plugin-media-smoke.md)
      that uses isolated fixture grants and never starts normal authentication,
      keychain/configuration, core MQTT, or bundled cameras. Its temporary package
      manager has empty trust and no installed workers.
- [x] Verify actual macOS playback, focus, window stacking, user close, lease
      revocation, automatic close at video end, and complete cleanup in the
      isolated native harness, separately from backend fixtures.
- [ ] Run graphical playback/window acceptance on Linux and Windows. Their CI
      compilation and non-graphical `--help` checks do not establish playback.
- [x] Extend Android/iOS source/dependency/asset checks; add no mobile media plugin
      route, window, package, worker, or optional integration implementation.
      Add independent new media-marker regressions for APK, AAB, and IPA payloads.
- [x] Complete local native/frontend checks, both frontend builds, actual package
      and native playback acceptance, formatting, and strict Clippy for both the
      normal and opt-in smoke-feature builds.
- [x] Pass current mobile target/artifact checks, review, and exact-commit hosted
      CI before merging [PR #426](https://github.com/victron-venus/inverter-desktop/pull/426).

Direct Frigate is the scope of this checkpoint. HA proxy enrichment, snapshots,
Kerberos/Ring, configuration migration, and removing bundled compatibility code
remain later work. The legacy media command is not a safe lifecycle shortcut: it
reads shared configuration and lacks plugin/generation ownership across downloads.

Final local checks passed 381 native tests, 295 frontend tests, and worker strict
Clippy plus 17 unit and 10 actual subprocess/TCP tests. The worker release build,
fresh advisory audit, real-binary package staging, and 19 packaging tests passed.
Both explicitly selected release-worker/signed-package/Mosquitto tests passed
(motion and clips, 2/2); they use simulated native windows and do not prove decoding
or native focus/stacking. The mobile boundary verifier now passes 26 Python tests,
including every new media marker in APK/AAB/IPA fixtures, with Pylint 10/10.
The macOS native smoke passed with exit code 0 and complete cleanup: a 24-second
640x360 H.264 fixture decoded through the actual player, two 330x186 windows stayed
unfocused and nonoverlapping, the focused anchor was preserved, and owned close,
revocation, video-end close, bounded range reads and wrong-window rejection passed.
The test exposed two production bugs that are now fixed: placement uses the
monitor work area, and showing a plugin window on macOS no longer makes it the
key window. The [native smoke record](docs/native-plugin-media-smoke.md#recorded-macos-acceptance)
details the evidence. Linux/Windows graphical playback remains pending; CI
explicitly compiles/lints the opt-in example on Linux and runs its non-graphical
`--help` on Linux and Windows. The explicit smoke input-policy test and all-targets
Clippy with and without the smoke feature also passed. All hosted checks passed
at [the tested head](https://github.com/victron-venus/inverter-desktop/commit/fc340fef7803bab1eb34dfc24f1f747f9aba6e13),
including Android/iOS packaged boundaries and Linux/macOS/Windows worker checks.
PR #426 was squash-merged as [the main checkpoint](https://github.com/victron-venus/inverter-desktop/commit/3173bc078394a3eb1c6d771b79b6cf58be027331).

### Completed checkpoint: retained data inventory and cleanup

- [x] Expose bounded, deterministic metadata for stored records without reading
      plaintext or requesting the credential key. Preserve lazy empty-store reads
      and report record/byte quotas, including transaction overhead.
- [x] Map records only to IDs currently present in the native installed inventory.
      Present records with unknown owners honestly; a filename hash cannot recover
      an uninstalled plugin's name. Expose no filesystem paths or stored values.
- [x] Allow explicit deletion of an unchanged, canonical record using its opaque
      ID and ciphertext revision. Support corrupt or empty bounded ciphertext and
      unavailable credentials; reject unsafe links, paths, and oversized files.
- [x] Serialize inventory and cleanup with package lifecycle operations. Protect
      all installed owners, including disabled packages and invalid payloads;
      reject stale authentication epochs and preserve owned work after IPC cancellation.
- [x] Add an on-demand desktop inventory with usage, refresh, empty/error states,
      installed-owner guidance, and explicit deletion confirmation. Invalidate
      stale responses and consent across authentication and package changes.
- [x] Keep cleanup independent from worker restarts and core transports. Do not
      scan retained files in response to high-frequency worker contribution events.
- [x] Test quota recovery, corrupt/unknown records, stale revisions, reinstall
      races, cancellation/logout, UI confirmation, and desktop/mobile boundaries.
- [x] Complete independent reviews, lint/format/typecheck, focused and full relevant
      suites, and both frontend builds. Fix the cross-window invalidation and
      enable/disable refresh issues found during review with regression tests.

Local validation: 340 macOS native tests and strict Clippy, 275 frontend tests,
8 mobile frontend tests, 4 build-profile checks, and 22 native mobile-boundary
tests passed. Both frontend production builds passed; final desktop output is
restored. Formatting/typecheck passed, changed frontend files have no lint
diagnostics, and Python lint scored 10/10. Final head `c8d7dcf` passed all hosted
checks, including Linux/Windows and actual Android APK/AAB/iOS IPA inspection in
[quality gate 34880350283](https://github.com/victron-venus/inverter-desktop/actions/runs/34880350283).
PR #424 merged as `5791d21`; canonical main was synchronized with an identical
validated source tree, a clean checkout, and preserved private files and stashes.

This iteration does not add a plaintext identity catalog, migrate legacy feature
configuration, install a package, or introduce production publisher keys. Unsafe
filesystem entries remain explicit errors; cleanup never follows links or accepts
arbitrary paths. Real HA/camera extraction and device parity remain later phases.

### Completed checkpoint: isolated settings and worker configuration

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

Validation for PR #423: 323 macOS native tests and strict Clippy, 252 frontend tests,
8 mobile frontend checks, 4 build-profile checks, and 22 native mobile-boundary
tests passed. Both frontend production builds passed. Hosted Linux passed 321
tests, Windows passed 136, and final Android APK/AAB and iOS IPA inspection passed
for head `c47ecc5` in [quality gate 34872923208](https://github.com/victron-venus/inverter-desktop/actions/runs/34872923208).
The PR merged as `61b1e18`; the canonical main checkout was synchronized cleanly.
This checkpoint does not establish real HA/camera package or physical-device parity.

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
- [x] Add declarative installed-package settings with validated secret handling.
- [x] Extend contributions to native desktop notifications with verified permission,
      bounded queues/rates, and generation/session-aware OS submission.
- [x] Add owned media surfaces with lifecycle cleanup and scoped access.
      Frigate HTTP video and owned windows passed the PR #426 checkpoint.
- [x] Limit worker IPC to authenticated dashboard/settings windows and advertised
      action parameters. Revoke old session epochs on logout/policy change/expiry;
      reject stale queued writes, contributions, and action results after re-login.
- [x] Authorize scoped settings/secrets delivery through the verified startup
      configuration handshake for packages declaring configuration permission.
- [ ] Authorize scoped host services for permitted origins/topics and owned files.
      Manifest permission metadata alone grants none of these services.
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
      on every desktop target. No new application/executable signing prerequisite
      is introduced by the desktop plugin architecture.
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
- [x] Close owned video windows and clean owned media/temporary files when the
      owning worker/session is revoked; verified in the Frigate clips checkpoint.
- [x] Offer retention/deletion of plugin settings on uninstall; preserve core data.
- [x] Add a retained-data inventory and explicit cleanup for uninstalled packages
      and unknown owners before end-user release, including quota recovery.
- [x] Test native clean install, update, failed activation, rollback, and uninstall
      with actual signed archives and executable workers.

Acceptance: a clean core installation contains no HA/camera payloads. Installing
or removing a package changes available features without reinstalling the app.
Native lifecycle and desktop application/UI integration are implemented. Full
phase acceptance still requires final integration checks, production trust and
worker distribution, remaining host services, and migrated HA/camera packages.

### 6. Cameras package

- [x] Add a separately built Frigate MQTT motion worker with isolated configuration,
      status, exact-topic subscription, deduplication/cooldown, and broker recovery.
      Prove package lifecycle against a real local broker without HA/core MQTT.
- [x] Move direct Frigate clips into the standalone worker with owned HTTP video
      and native window cleanup; preserve independent core telemetry.
- [ ] Move Kerberos/Ring adapters to workers separately from core Cerbo MQTT
      and source selection, after verifying producer URL/auth requirements.
- [ ] Preserve snapshots, complete configured-topic coverage, clip behavior, and
      window stacking; verify actual native notification display on supported OSes.
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
- [x] Retain all seven inverter flags, metadata, saved controls, and MQTT routing
      in core, including `input_boolean.<flag>` aliases. The HA worker treats
      independently configured HA entity IDs literally without this alias lookup.
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

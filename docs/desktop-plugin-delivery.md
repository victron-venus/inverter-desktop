# Desktop plugin extraction delivery

This record separates reviewed source, published release bytes, and observed
installed behavior. It contains no household credentials, camera destinations,
or private entity identifiers.

## Reviewed source

[PR #456](https://github.com/victron-venus/inverter-desktop/pull/456) was merged
on 2026-09-16 at `32a079cad994e3326f3704096fd1594b91dd07ac`.
Its final reviewed head was `2ebba50736b8a1543f9f004ee9d51365994f5370`:
60 checks succeeded, six were skipped, approval covered that exact head, and
there were no unresolved review threads at merge.

The extraction removes bundled HA and camera clients. The desktop host retains
generic package management, encrypted settings, compact presentation, and owned
media windows. HA, Frigate, Kerberos, and Ring are separate worker packages.
Core inverter-control MQTT flags remain available without them. Android and iOS
exclude the package runtime and all optional provider features at build time.

Hosted validation covered frontend tests, strict Rust checks, actual installed
worker fixtures, all shipping worker platforms, and fresh APK/AAB/IPA artifact
boundaries. The final local macOS host suite passed 528 tests plus two packaging
CLI tests. All twelve installed-worker fixtures passed. The shared viewer passed
isolated native MJPEG and H.264 graphical acceptance; see the
[exact executable evidence](native-plugin-media-smoke.md#shared-local-viewer-checkpoint).

## Published release

[v2.5.42-beta.43](https://github.com/victron-venus/inverter-desktop/releases/tag/v2.5.42-beta.43)
was published on 2026-09-16 at `08:41:21Z`. Its tag resolves directly to the merged
source above. [Release run 35069268321](https://github.com/victron-venus/inverter-desktop/actions/runs/35069268321),
attempt 1, succeeded with 37 successful jobs and two intended skips. The frozen
build number is `2005085`.

The published inventory contains 55 assets, including 16 worker archives,
16 SHA-256 sidecars, four platform configuration fragments, and six platform
build receipts. The release pipeline passed all four desktop builds and both
mobile builds. These are CI artifact results; installed behavior remains a
separate acceptance step.

Public-byte verification passed on 2026-09-16 at `08:42:03Z` for the
`aarch64-apple-darwin` app and all four workers. It checked the release manifest,
source tag, frozen plan, build receipt, archive and payload hashes, sidecars,
canonical package manifests, and exact source schemas. The app reports
`2.5.42-beta.43`, native bundle version `201.50.84`, and the merged source above.
Its executable SHA-256 is
`77ac475c89625b27ca0a5e5a66d1d0649309636c377a1e6ecee346a4b4e1aa49`.
Every verified worker requires host API `^1.8`.

The installed macOS selection uses three exact archive pins from the
[published ARM configuration fragment](https://github.com/victron-venus/inverter-desktop/releases/download/v2.5.42-beta.43/desktop-plugins-aarch64-apple-darwin.json):

- `inverter-desktop.home-assistant` version `0.13.0`, SHA-256
  `37eeabe9b3ca20b03047e0cadeb2f8f20731d775afe38a16d2915f2ffdd5fe99`.
- `inverter-desktop.frigate` version `0.3.0`, SHA-256
  `105aedae83ea8cb8a11b8dbdf0116e52bd09a2528c31f4adc17598bc68a47e97`.
- `inverter-desktop.kerberos` version `0.1.0`, SHA-256
  `ee87d74b5615dab79caede132a8d7788454a205f0773bcf401733bf630d943b4`.

Ring `0.1.0` was also verified and remains available but unselected. All four
platform fragments and the complete published inventory were checked; independent
public-byte verification of worker payloads covered macOS ARM only. Configured
downloads require no publisher key or application signing.

## Beta.43 installation acceptance

The verified beta.43 app was installed on 2026-09-16 at `08:59:45Z`, with the
exact executable SHA-256 recorded above. A matched backup retains the previous
bundle, encrypted configuration and plugin settings, package storage, preferences,
WebKit data, and caches.

Protected-data backup verification covered content, file modes, UID/GID,
`st_flags`, ACLs, and extended attributes. Copy-only provenance and quarantine differences were
recorded for exact non-executable data paths, with raw original values retained
privately. Exact restoration of those OS tracking attributes remains unverified.
Strict validation of live data, the staged app, and application bundles remained
unchanged.

The pin-only update applied the three declarations above and preserved every
non-plugin configuration field. The first native launch at `09:00:07Z` stalled
while obtaining access to the existing Keychain encryption key. That access was
resolved without bypassing the protected prompt, and a normal restart restored
beta.43's plugins. This initial stall is resolved; it is not a current Keychain
failure.

Read-only installed acceptance passed all four states: HA and cameras enabled,
HA only, core only, and cameras only. The compact dashboard and core telemetry
remained available as appropriate in each state. Both groups were restored at
the end: HA, Frigate, and Kerberos reported Running/Connected, all three selected
declarations were enabled, and the final inspection confirmed unchanged
non-plugin configuration. No household commands were sent as test traffic.

Native encrypted HA and Kerberos settings reached migration version 1. Existing
authoritative Frigate settings retained marker 0 without requiring migration.
The HA selection retained all 18 previous reads within 45 selected reads,
including all 20 configured clamps, and preserved credentials. All 15 Home
controls retained their targets, labels, and order. There were no saved HA header
controls: DRY/External and the seven inverter flags belong to core.

A natural Kerberos event opened an automatic preview, and the owned window
expired. This verifies the installed event/window lifecycle; it does not claim
decoded physical camera frames. Short native host/worker CPU, RSS, and connection
samples were also collected across the four states. They exclude WebKit and are
observations, not benchmarks, cold-start measurements, or proof of total resource
savings.

## Startup recovery follow-up: published and installed

The initial credential-access delay exposed a recovery gap: after configuration
became readable, beta.43 needed a normal restart to restore a revoked plugin host.
The follow-up implementation retries authorization on a later successful unlocked
`auth_status` read. It samples fresh configuration while holding the configuration
and session-transition locks, retains the session guard through authorization,
and releases these guards before scheduling package restoration. Healthy polls
do not restart workers; logout, policy changes, exit, and stale epochs retain
their existing checks.

Local validation passed 12 focused recovery regressions, the full 535-test native
suite plus two packaging CLI tests, 432 frontend tests, 18 mobile tests, and 29
native mobile-boundary checks. Strict all-target Clippy, type checking,
desktop/mobile builds, and formatting passed. Frontend lint passed its error gate
with existing warnings and informational diagnostics. The twelve external-worker
fixtures are a separate gate, not graphical tests included in the native count.

[PR #458](https://github.com/victron-venus/inverter-desktop/pull/458) merged on
2026-09-16 at `13:06Z`, at revision
`74d7311d132f9262feac65e94086138a173aca25`. Its exact reviewed head
`4151436a1a5bcca73ed987317f36c6faff485834` passed 59 hosted checks with three skips,
was approved, and had no unresolved review threads.

[v2.5.42-beta.45](https://github.com/victron-venus/inverter-desktop/releases/tag/v2.5.42-beta.45)
was published on 2026-09-16 at `14:02:44Z` from the merged revision above.
[Release run 35099769637](https://github.com/victron-venus/inverter-desktop/actions/runs/35099769637),
attempt 1, passed 37 jobs with two intended skips and published 55 assets. The
frozen build number is `2005088`.

Independent public-byte verification passed at `14:09:30Z` for the macOS ARM app
and all four worker packages. The tag, source revision, frozen plan, build
receipts, and downloaded payloads agree. The app reports native bundle version
`201.50.87`; its executable SHA-256 is
`5f173dd6737cfff902872b924e2a2b6cad7df6fa40192ed1cc3dd175aea2ef24`.
The verification receipt SHA-256 is
`8eb061172ccc90f136b9b1794fc8514114c8a02f4269b963fe0a8562fa18062b`.
This independent payload verification covers macOS ARM; other platform evidence
remains the release pipeline's artifact checks.

The verified candidate was staged without execution or installation. Its stage
receipt SHA-256 is
`409e1054e5d06709ac9f394cf6ee08540365b0511331a1b3326699f145942a8c`.
The selected HA, Frigate, and Kerberos package hashes are identical to beta.43's
recorded pins, so the existing declarations and pins are retained.

### Beta.45 installation and normal-start acceptance

macOS became accessible on 2026-09-16 at `14:36Z`, resolving the temporary
installation gate. The unchanged reviewed installer then installed the verified
beta.45 host using a fresh stopped-app backup transaction,
`desktop-plugins-beta45-20260916T143800Z`, with zero app/worker processes present.
The installation receipt SHA-256 is
`bb4df8668be1e2d53c37a4da39e384afc5115044024ada6db4dcfa98797629b3`.
Independent validation passed all five backup fingerprints, four protected-data
security groups, and stage/source identity. The same previously reviewed,
path-specific OS tracking differences were recorded privately; strict bundle and
live-source checks were retained. No exact OS tracking restoration is claimed.

The installed executable matches beta.45's `5f173dd6…` SHA-256 recorded above and
source `74d7311d132f9262feac65e94086138a173aca25`. Its first launch automatically
restored the Frigate, HA, and Kerberos worker processes. At `14:38:48Z`, the GUI
showed Desktop `2.5.42-beta.45`, Control `1.23.4`, IGW Live data, the seven core
flags and DRY/ESS controls, EV/water/battery/solar, Cameras ON, and all 15 Home
controls. No household commands were sent as test traffic.

Independent inspection confirmed all three declarations enabled with the same
selected pins, unchanged non-plugin configuration, and three encrypted plugin
settings byte-identical to the fresh backup. Configuration ciphertext was also
unchanged, with SHA-256
`675953f720d78ff230af19ef76ca8cd1ffebb78a3ff49b411cbcb57415ed0bf8`.

A natural Kerberos preview window opened after startup; its pixels were not
inspected. Subsequent GUI inspection timed out for both the app and SystemUIServer.
Native inspection still showed stable worker processes and the main thread in
its normal event loop, without Keychain blocking. No fresh Plugin Manager
Connected labels or visible confirmation of the new introduction text are claimed.
Source tests and verified artifact identity cover that copy change.

Implementation, release verification, host installation, and normal first-start
acceptance are complete. The earlier beta.43 four-state matrix remains separate
evidence. No delayed or denied Keychain fault was deliberately induced in beta.45;
the 12 focused recovery regressions cover those authorization/race boundaries.

## Configured-package lifecycle correction: published and installed

Beta.45 incorrectly blocked individual enable/disable and uninstall controls for
configuration-managed packages. The correction permits those controls while
preserving exact version pins. Enable/disable changes are saved natively;
confirmed uninstall durably disables the package, removes its restore declaration,
and removes its installed files. Settings and secrets are retained by default,
and rollback remains unavailable while an exact pin owns the package.

The operation is serialized with restoration and downloads. If cleanup fails
after pin removal, a remaining installed record stays disabled and the UI refreshes
its state while retaining the error. Ordinary Config, theme, and setup saves
preserve current declarations, so an old form cannot resurrect an uninstalled
plugin. Explicit declaration edits require a matching saved baseline. Unrelated
workers, core configuration, and transport remain outside the operation.

[PR #461](https://github.com/victron-venus/inverter-desktop/pull/461) merged on
2026-09-16 at `17:15:37Z`, at revision
`cda1225235a04bcc782fd4e6e71ae070fd4c23f0`. Its exact reviewed head
`aab952c87c492271b52fdcfdf65ecd43ceacb7a3` passed 59 hosted checks with three skips,
was approved, and had no unresolved review threads.

Local validation passed 545 native tests (including six focused lifecycle
regressions), plus two packaging CLI tests, 433 frontend tests, 18 mobile tests,
and 29 mobile-boundary tests. The twelve external-worker fixtures remain separate from the ordinary
native count. Desktop/mobile builds, type checking, strict Clippy, formatting,
and error-level lint passed; lint retained 94 warnings and 70 informational
diagnostics. Independent reviews covered native concurrency, UI behavior, and
documentation. The local validation receipt SHA-256 is
`8af74f06d763b4d299f04b8f466db5eee1e7954931aaf5081fffc305dfd15790`.

[v2.5.42-beta.47](https://github.com/victron-venus/inverter-desktop/releases/tag/v2.5.42-beta.47)
was published on 2026-09-16 at `18:12:26Z` from the merged revision above.
[Release run 35127066014](https://github.com/victron-venus/inverter-desktop/actions/runs/35127066014)
succeeded with 37 successful jobs and two skips. Independent public verification
passed; its receipt SHA-256 is
`4f9b58df4197c4a44f6c395fba796e2c6ad087a323535ff966bcf12e429182f1`.
The verified staging receipt SHA-256 is
`8eae00751183aad8402c068d7181887da5e09125e4c402d2bff8ccd777630d8e`.

### Beta.47 installation and GUI acceptance

The verified host was installed using backup transaction
`desktop-plugins-beta47-20260916T181517Z`. Independent backup verification passed
all five fingerprints and four protected security groups; the previous beta.45
rollback bundle remains intact. The installation receipt SHA-256 is
`56a537d0d083393656727e1b38d253b237181dde97804131d581dc8d74831c64`.
The installed executable SHA-256 is
`107953c5b4dab0ecf4137d54ae7d0b5918a48ff0bba68a1518d83ea201dd1a27`.
Configuration ciphertext, all three encrypted plugin settings records, and the
selected declarations and archive pins are unchanged.

Installed GUI acceptance completed at `18:17:59Z`. HA, Frigate, and Kerberos each
showed Ready/Running/Connected with active Disable and Uninstall buttons.
Frigate's uninstall confirmation opened with an active confirmation button and
the settings-deletion checkbox unchecked. Its text explained that uninstall
removes the saved declaration and stops automatic restoration. The dialog was
cancelled; no package was uninstalled. The main dashboard then showed Desktop
`2.5.42-beta.47`, Control `1.23.4`, live IGW data, seven core flags, Cameras ON,
and all 15 Home controls.

The durable acceptance receipt SHA-256 is
`dd0f31f9ea3c391fd154c434a0b8b59382004e8940d625685827a02e26a5bcf8`.
Source, release, installation, and the installed control/confirmation inspection
are complete. No household commands were sent, and no live uninstall was
performed. Removal, partial failures, and concurrent restoration remain covered
by isolated regressions; this acceptance did not exercise forced authorization
failures or other physical platforms.

## Evidence boundaries

Automated process, protocol, restore, and media fixtures do not prove a physical
device acted on a command. macOS graphical acceptance does not prove Linux or
Windows graphical playback or native notification display. Mobile artifact
inspection proves feature exclusion for those artifacts; it is separate from
on-device functional acceptance.

The seven flag identities, topics, and absolute command payloads remain stable.
HA server schedules do not depend on a Desktop plugin being enabled. Correcting
MQTT switch state confirmation can affect state-dependent HA logic; see
[MQTT ownership and schedule compatibility](mqtt-control-ownership.md#ha-mqtt-switches).

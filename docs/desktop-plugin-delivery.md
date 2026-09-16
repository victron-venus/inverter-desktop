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

The prepared macOS selection takes three exact archive pins from the
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

## Installation acceptance

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
non-plugin configuration field. The first native launch at `09:00:07Z` is waiting
for access to the existing Keychain encryption key. The protected macOS prompt
requires user interaction. Migration has not started; plugin health and live
acceptance remain pending.

The installed acceptance must verify package health and restoration, retained
credentials and mappings, the compact dashboard, and core telemetry while
optional packages are disabled and re-enabled. Household actions are outside
this read-only acceptance and must not be used as test traffic.

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

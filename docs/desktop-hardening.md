# Desktop session, transport and storage contracts

The September 2026 audit fixes preserve Vue + Tauri, coalesced IPC delivery and
hidden-window throttling. They do not require a server migration.

## Authentication and plugin media

General application IPC passes through the Rust session guard, except explicit
authentication commands and closing auxiliary windows. Owned media windows have
a separate narrow IPC allowlist tied to their active generation. Reading or
writing configuration, exporting settings, controlling equipment and opening
Settings from the tray require an unlocked session when authentication is enabled.
Main and settings roots use `AuthGate`; protected components mount only after the
backend reports an unlocked session. An unlock is shared by this application's
trusted local windows and expires after 15 minutes. Changing the username,
password, authentication flag or biometric flag revokes it. Biometrics require
both authentication and biometric settings to be enabled. This is an application
lock, not a replacement for the operating system's account boundary.

Camera providers run in optional desktop workers. The generic host derives media
grants from verified package declarations and encrypted plugin settings. Downloads
must match the configured origin and path prefix; optional bearer credentials are
scoped to that grant. Userinfo, traversal, lookalike origins, different effective
ports, and scheme changes are rejected. The native transfer client disables
redirects. Provider proxy credentials do not require an installed HA worker.

Mapped live previews use an exact private URL resolved by native code, an explicit
manifest duration, and generation-owned windows. Their grants cannot authorize
downloads. The private URL is available only to its active owned viewer, whose
image CSP is restricted to its origin; it is absent from dashboard snapshots and
window routes. See [plugin media grants](../src-tauri/src/plugins/protocol.rs),
[bounded transfers](../src-tauri/src/plugins/http_video.rs), and
[owned windows](../src-tauri/src/plugins/media_windows.rs).

## Transport ownership and displayed data

Core MQTT owns its coalesced emitter and cancellation signal. Camera workers own
separate provider connections; stopping them cannot stop or reconnect inverter
telemetry. Cerbo parsing and state overlay remain separate from core lifecycle.

The HA worker owns REST/WS supervision, selected entity state, and household
actions. Settings changes restart the worker under a new generation; the host
rejects old contributions and actions after revocation. Generic Vue presentation
uses exact current contribution references, and disconnected worker state
withdraws action availability. Provider networking and its reconnect policy live
in [the HA worker](../desktop-plugins/home-assistant/src/network.rs), not a bundled
frontend or native HA session. The native
[plugin runtime](../src-tauri/src/plugins/runtime.rs) owns generation authority.

Telemetry metadata records `observed_at`, transport source, per-field reception
times and `live`/`stale`/`unknown` quality. The UI marks data stale after 30 seconds
without observed updates or after a disconnect. Opening a window and reading
cached state does not renew that age. Changing the configured endpoint clears the
previous source's state. These timestamps describe reception by this desktop;
they are not device measurement timestamps. A producer that republishes held
values in a full snapshot must supply its own timestamps to establish sensor age.

## Configuration and credential migration

Configuration remains AES-256-GCM encrypted. Desktop keys move from `config.key`
to the platform credential store. The legacy file is removed only after the exact
key is written and read back successfully. Android stores a random configuration
key wrapped by AndroidKeyStore; iOS stores it in the installation's Keychain
entry. Mobile files encrypted with the historical shared key are read solely for
migration and immediately re-encrypted with the new random key.

Unavailable credential storage and invalid/decryption-failed configuration are
errors, not an invitation to replace saved settings with defaults. A failed save
does not report success, apply dependent startup settings or emit `config-saved`.
Section visibility settings survive serialization as well.

Settings exports omit credentials, local authentication policy and URLs containing
userinfo, query strings or fragments. Imports preserve this installation's auth
policy and ignore imported credentials. Existing credentials are retained only
when the corresponding endpoint is unchanged; importing another endpoint clears
them. Exports are portable settings, not credential recovery backups.

Core ownership remains in `auth`, `config_store`, `config_backup`, `mqtt/lifecycle`,
and `mqtt/cerbo`. Generic desktop services live in `plugins/application`,
`plugins/runtime`, `plugins/settings_store`, `plugins/legacy_migration`, and
`plugins/media`. Provider implementations live in the separate
`desktop-plugins/home-assistant`, `frigate`, `kerberos`, and `ring` packages.
Android/iOS exclude the plugin runtime and providers at build time.

## Build and update policy

Use `pnpm install --frozen-lockfile --ignore-scripts` for development and CI.
`pnpm-lock.yaml` is the single dependency graph; the npm lockfile is removed.
`pnpm build` always runs `vue-tsc --noEmit` before Vite. PR mobile validation is
owned by Quality gate; the release workflow retains its existing signing and
artifact-provenance checks. Manual workflow dispatch remains available.

The **Download updates** menu opens the project's latest GitHub release and
reports a failure to open it. Installation is manual. The unused automatic updater
plugin and placeholder public key are removed.

## Validation and operational follow-up

Run the repository gates before merging:

```sh
pnpm lint
pnpm format:check
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml --all-targets
```

Regression tests cover camera-origin selection, session expiry and policy,
AuthGate races, failed settings saves, HA snapshot/disconnect/configuration
ordering, MQTT cancellation/coalescing, telemetry reception freshness, encrypted
storage migration and redacted exports. Native helper compilation checks API and
syntax compatibility; it does not establish Keychain/Keystore runtime behaviour.

For acceptance on real hardware, exercise tray and secondary-window locks,
sleep/wake, repeated reconnects, MQTT-to-gateway transitions, HA server/mapping
changes and camera playback. For a repeatable performance baseline, use the same
device and entity inventory for five-minute visible and hidden-window runs. Record
IPC events and serialized bytes per second in the WebView profiler, Vue updates,
active connections/timers, process CPU and memory. Repeat after ten reconnects;
connection/timer counts should return to baseline. Choose CPU and IPC thresholds
from these measurements rather than inventing limits from unit-test results.

Tests do not connect to the user's MQTT/HA services, access their credential
stores, install signed applications or substitute for physical Android/iOS and
camera testing.

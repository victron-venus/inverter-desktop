# Desktop session, transport and storage contracts

The September 2026 audit fixes preserve Vue + Tauri, coalesced IPC delivery and
hidden-window throttling. They do not require a server migration.

## Authentication and camera downloads

All application IPC commands pass through the Rust session guard, except the
explicit authentication commands and closing auxiliary windows. Reading or
writing configuration, exporting settings, controlling equipment and opening
Settings from the tray require an unlocked session when authentication is enabled.
All Vue window roots use `AuthGate`; protected components mount only after the
backend reports an unlocked session. An unlock is shared by this application's
trusted local windows and expires after 15 minutes. Changing the username,
password, authentication flag or biometric flag revokes it. Biometrics require
both authentication and biometric settings to be enabled. This is an application
lock, not a replacement for the operating system's account boundary.

Camera downloads send an HA bearer token only to the configured HA origin
(scheme, host and effective port). Userinfo, lookalike domains, a hostname inside
the path/query, different ports and HTTPS downgrades do not qualify. Redirects
are disabled on this download client. Explicit ports, including 80/443, are
honoured; otherwise the same configured/default 8123 port as the HA client is used.

## Transport ownership and displayed data

Each MQTT client owns its coalesced emitter and cancellation signal. Stopping
camera MQTT cannot stop inverter telemetry. Connection-specific keepalive tasks
end when that connection or session ends; queued emissions cannot revive a stopped
session. Cerbo parsing and state overlay live in a separate module from lifecycle.

HA configuration revisions interrupt connect, live subscription and retry waits.
An old reader is aborted when its client is dropped. Revisions are checked while
updating entity state, so the previous server cannot repopulate a cleared map.
Interactive HA controls receive coalesced updates at 500 ms; sensor snapshots use
a trailing two-second window. Initial/refresh snapshots bypass that window.
Vue subscribes before requesting a snapshot, clears all derived display arrays
after the disconnect grace period, and disables offline controls.

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

The main ownership boundaries are `auth`, `config_store`, `config_backup`,
`camera`, `ha_session`, `ha_api/lifecycle`, `mqtt/lifecycle` and `mqtt/cerbo`.

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

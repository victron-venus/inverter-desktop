# Frigate worker

`inverter-frigate-worker` is a standalone Rust executable for the desktop plugin
`inverter-desktop.frigate`, version 0.2.0. It opens its own MQTT connection, emits a
Frigate connection-status card, and sends native motion-start notifications through
host API 1.2. With a configured direct Frigate HTTP address and host API 1.3, it also
requests playback of completed clips. It contains no Tauri, core MQTT client, HA client, inverter controls,
frontend code, or dependency on the running core telemetry connection.

The package requires host API `^1.3`. The executable still negotiates compatible
1.2 sessions as motion-only and never sends a newer media frame to an older host.
Pre-release, incompatible major, and malformed host versions are rejected.

This is part of camera extraction. It does not provide snapshots,
Kerberos/Ring, HA proxy access, or legacy settings
migration. Bundled desktop cameras remain available while those parts are built.
The shipped publisher policy is still empty: production installation is disabled.
Tests use disposable package keys, and no production publisher keys are generated.
Package archive authentication is separate from application signing; desktop app
or executable signing/notarization is not a prerequisite added by this worker.

## Configuration

The package uses the existing native settings editor and encrypted per-plugin
record. It receives configuration through its startup pipe, acknowledges the exact
revision, and only then opens a network connection. Saving settings restarts an
enabled worker; disabling or uninstalling closes its connection. Credentials are
absent from command-line arguments, inherited environment, dashboard status, and
worker diagnostics.

- `mqtt_host`: required hostname or IP address, at most 253 bytes. Do not include
  a scheme, path, username, or port. IPv6 addresses use their unbracketed form.
- `mqtt_port`: integer 1–65535; default 1883. Enabling TLS does not change this
  value automatically; select the actual TLS listener port explicitly.
- `mqtt_tls`: default false. When true, certificate and hostname verification use
  the platform trust store through rustls. There is no insecure certificate mode
  or plaintext fallback. Client-certificate authentication is not implemented.
- `mqtt_topic`: one exact topic, default `frigate/events`, at most 256 bytes. MQTT
  wildcards and the legacy semicolon-separated topic-list syntax are rejected.
- `mqtt_username` and `mqtt_password`: optional write-only secrets, at most 1,024
  bytes each, without control characters. A nonempty password requires a username.
- `frigate_base_url`: optional direct HTTP(S) address, at most 2,048 UTF-8 bytes.
  Include the actual port and any reverse-proxy path prefix, for example
  `http://frigate.local:5000` or `https://camera.example/frigate`. Missing, null,
  or blank values keep motion-only operation. User information, query strings,
  fragments, control characters, and ambiguous path normalization are rejected.
  There are no HTTP credentials or injected headers in this slice; the host does
  not reuse HA credentials to access the direct Frigate API.

The worker connects only to the configured broker and subscribes only to the exact
configured topic at QoS 0. It publishes no messages. The `network_mqtt` permission
is a declaration for this native executable, not an OS firewall or an arbitrary
core MQTT publishing API. HA is not required for this connection.

## Motion and recovery

The parser accepts Frigate `type: new` messages with nonempty `after.id` and
`after.camera`, each bounded to 128 UTF-8 bytes. It emits `Frigate <Camera> camera motion detected` with body
`Motion started`. Names are normalized to plain text and truncated at a UTF-8
boundary to fit the host's 128-byte title limit. Event identifiers are hashed into
the protocol's bounded ID grammar.

Event IDs are deduplicated for ten minutes, each camera has a 45-second cooldown,
and each state map has at most 512 entries. Incoming JSON is limited to 16 KiB.
The motion parser ignores retained deliveries, updates, completed events, and malformed
messages. If `after.start_time` is present, starts older than ten minutes or more
than one minute in the future are ignored. Legacy publishers without timestamps
remain supported. Reconnect preserves the in-memory motion history; a fresh worker
process starts with a new history.

For clips, only `type: end` with the boolean `after.has_clip: true` qualifies.
Clip history is independent: it has its own ten-minute event-ID deduplication,
45-second camera cooldown, and bounded maps. A motion-start notification cannot
suppress the same event's completed clip. The start timestamp is not an age limit
for completed recordings, which can legitimately be long. Retained deliveries,
updates, missing/false/nonboolean clip flags, and malformed events are ignored.

The worker appends `/api/events/<event-id>/clip.mp4` beneath the configured path
prefix, encoding the ID as one URL segment. Spaces, Unicode, and query/fragment
characters are encoded without changing the origin or URL structure. IDs containing
percent signs, separators, controls, or an exact `.`/`..` segment are not accepted
for clips. Motion IDs retain their existing behavior. The completed event emits
one `http_video` frame, with a separate `frigate-clip-<SHA256>` ID; it does not emit
an additional worker notification. The native host owns permission checks, the
clip-available notification, HTTP download, cancellation, and local video windows.
The worker itself never performs HTTP requests or supplies executable frontend code.

The worker limits combined motion/media event output to 30 per minute before the host applies
its independent permission, rate, queue, and generation checks. The existing
two-frame output queue drops excess events while the host is slow, keeping MQTT
polling responsive. Handshake, configuration acknowledgement, and status frames
still wait for their writes to complete. Event delivery is best effort.
See the [notification contract](../../docs/plugin-worker-protocol.md#native-desktop-notifications-host-api-12)
for host queue limits and delivery semantics. Native submission does not prove
that the OS displayed a notification or that the user granted display permission.

Connection status is `Connecting`, `Connected`, or `Disconnected`. Connected
requires a matching successful SUBACK. Failed connections and rejected subscriptions
retry with bounded exponential delays from one to 30 seconds; every retry creates
a clean MQTT session and subscribes again. Shutdown and stdin EOF cancel retry,
network work, and blocked output waits promptly. No endpoint, raw broker payload,
or credential is included in status or error messages.

## Build and stage

This crate is a separate Cargo workspace with a checked-in lockfile. It is built
only for macOS, Linux, or Windows and is not a dependency of the application.
Its build script rejects Android, iOS, and the mobile build profile.

```bash
CARGO_BUILD_JOBS=2 cargo build --release --locked --manifest-path desktop-plugins/frigate/Cargo.toml
```

For example, after a native Apple Silicon build, prepare a new staging directory:

```bash
python3 scripts/plugins/prepare-frigate-package.py \
  --worker desktop-plugins/frigate/target/release/inverter-frigate-worker \
  --target aarch64-apple-darwin \
  --output /private/tmp/frigate-package-stage
```

Use the actual compilation target and binary path for other builds; Windows uses
`inverter-frigate-worker.exe`. The output parent must already exist and the staging
directory must be new. The helper rejects symlinks/reparse points, oversized files,
target/CPU mismatches, non-executable formats, and mobile or universal Mach-O files.
Cargo hardlinked inputs are copied to an independent staged file. Each
archive contains one thin target executable. Header validation does not establish
compiler provenance or compatibility with every system library on that platform.
The helper retains the identities of the selected file and parent directories,
then compares the opened file before and after its bounded read. These checks
detect replacement and observable modification; they are not a filesystem
transaction. Source validation finishes before the output directory is created.

Staging produces `manifest.json` and `payload/`. Pass them to the existing
[native package encoder](../../docs/plugin-packages.md#producing-an-archive) with an
externally supplied publisher key. The helper itself neither signs nor installs.
No worker binary or `.idplugin` archive belongs in an Android/iOS application.

## Validation

Run worker checks separately from the host build to keep local machine load bounded:

```bash
cargo fmt --manifest-path desktop-plugins/frigate/Cargo.toml -- --check
CARGO_BUILD_JOBS=2 cargo clippy --locked --manifest-path desktop-plugins/frigate/Cargo.toml --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --locked --manifest-path desktop-plugins/frigate/Cargo.toml --all-targets -- --test-threads=2
cargo audit --file desktop-plugins/frigate/Cargo.lock
```

The independent worker lockfile receives its own advisory audit without borrowing
host-only exceptions. Hosted CI builds and tests the worker on Linux/macOS/Windows.
Parser tests cover malformed data, bounded state, timestamp/retained suppression,
independent start/clip deduplication, cooldown expiry, URL authority, version
negotiation, and output budgets. Subprocess tests drive real
stdio and TCP connections, including explicit topic/credentials, reconnect,
shutdown, completed-event media frames, motion-only compatibility, absence of
worker HTTP requests, and TLS ClientHello/no-plaintext-fallback behavior. The latter does not
establish a complete certificate-trust/hostname handshake fixture.

A separate pair of host tests uses the actual compiled executable and a private loopback
Mosquitto instance. Build the worker and the desktop frontend first, then provide
explicit absolute fixture paths (substitute the paths on your machine):

```bash
INVERTER_FRIGATE_WORKER=/absolute/path/inverter-frigate-worker \
MOSQUITTO_BIN=/absolute/path/mosquitto \
CARGO_BUILD_JOBS=2 cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
  plugins::frigate_integration_tests::signed_frigate_package_real_mqtt_lifecycle \
  -- --exact --ignored --test-threads=1
```

Run the same command with the exact test name
`plugins::frigate_integration_tests::signed_frigate_package_real_mqtt_clip_lifecycle`
for completed clips. That test also supplies a private HTTP fixture and simulated
native windows. It covers start/end separation, duplicate clips, URL prefixes,
range reads, settings restart, disable during a stalled transfer, uninstall, and
an independent core MQTT probe. HTTP failure/limit scenarios have separate service
tests; they are not all broker acceptance scenarios.

Ordinary host unit tests do not build another crate or start an external broker.
Both ignored acceptance tests are explicitly required by CI; missing fixture paths,
startup failures, or a zero-test selection cannot count as a pass. Its temporary
signed archive, publisher policy, settings, and broker are isolated from production.
The original motion test submits notifications to a test collector, so it does not verify actual OS
notification presentation. Current results and remaining feature/device boundaries
are tracked in [TODO.md](../../TODO.md).

The completed-clip worker tests exercise real process/TCP request production.
Native playback and desktop window behavior use the separate, opt-in
[native media smoke harness](../../docs/native-plugin-media-smoke.md). A successful
worker or broker test alone does not establish decoding, native focus/stacking,
or physical-device proof. Record each result separately in TODO.

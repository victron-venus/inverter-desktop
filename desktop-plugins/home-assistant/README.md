# Home Assistant worker

`inverter-home-assistant-worker` is the separate desktop package
`inverter-desktop.home-assistant`, version 0.1.0, using host API `^1.3`. This first
slice provides connection status and state contributions for an explicit entity
watch list. It has no Home Assistant service actions, camera media, core MQTT/IGW
connection, inverter-control aliases, or dependency on the bundled HA client.

The existing desktop HA integration remains bundled until its remaining features
have package parity. Android and iOS contain neither this worker nor the shared
worker protocol library, its package metadata, or the desktop plugin manager.
The production publisher policy remains empty, so installation is disabled in
shipped builds. This work creates no production publisher keys and adds no application or
worker code-signing/notarization prerequisite. Package signatures use the existing
native archive pipeline and an externally supplied publisher key.

## Configuration and read scope

Configure this package in its native settings editor. Values and write-only
secrets use the existing encrypted per-plugin record; the host sends the exact
configuration revision through the startup pipe. The worker validates and
acknowledges it before opening a network connection. Saving settings restarts an
enabled worker. Disabling, logout and uninstall stop its work without changing
the core telemetry connection.

- `ha_base_url`: required complete HTTP(S) address, at most 2,048 UTF-8 bytes.
  Include the actual port and optional reverse-proxy prefix; no HA-specific port
  is added automatically. Credentials, query strings, fragments, whitespace,
  backslashes and ambiguous path segments are rejected. Safe encoded prefixes
  such as `space%20prefix` are preserved; encoded separators, dot segments,
  double escapes and control characters are rejected. HTTPS uses certificate verification and the matching secure
  WebSocket scheme. Redirects are not followed.
- `watch_entities`: optional string, default empty, at most 4,096 UTF-8 bytes.
  Separate up to 32 unique entity IDs with commas or newlines. Blank entries are
  ignored and duplicate IDs count once. Each ID is at most 128 bytes with two
  nonempty `domain.object_id` parts using lowercase ASCII letters, digits and
  underscores. IDs are preserved literally, including names resembling inverter
  control flags; there is no core alias resolution.
- `ha_token`: required write-only token, 1–4,096 bytes of visible ASCII without
  whitespace or control characters. It is not read from core configuration or
  placed in arguments, inherited environment, dashboard contributions or logs.
  The worker only reads, but the token retains its account's Home Assistant
  permissions; this package does not create a restricted server-side token.

With a nonempty watch list, initial REST reads request only
`/api/states/<entity_id>` beneath the configured prefix. The worker never requests
the all-entity REST collection or WebSocket `get_states`. Live updates use
`subscribe_events` with `event_type: state_changed`. **That server stream covers
all entities**: the worker immediately discards unwatched entities and retains
only its configured list. This is local filtering, not a claim of server-side
subscription filtering or token-level access restriction.

An empty watch list still establishes the authenticated WebSocket connection for
connection status, but makes no entity REST reads or event subscription. A change
to the watch list is applied through the normal settings restart. Entity state
and labels are bounded plain data; the host renders contributions, and the worker
supplies no frontend code or arbitrary navigation URLs.

The worker subscribes before starting initial reads. A live update or deletion
received during a slow initial read wins over that older REST result. Numeric
states become metric cards; other values remain plain text, with explicit unknown
and unavailable states. Disconnect clears stale values. Dashboard updates are
coalesced to four per second, independently of socket reads and heartbeat checks.
Authentication rejection stops reconnect attempts until settings restart the
worker; network failures reconnect with a bounded delay.

The wire behavior follows the official
[WebSocket API](https://developers.home-assistant.io/docs/api/websocket/) and
[REST API](https://developers.home-assistant.io/docs/api/rest/).

The `network_http` permission describes this trusted native executable's direct
HTTP/WebSocket behavior. It is not an OS network sandbox or a host API for arbitrary
requests. The package declares only `plugin_configuration`,
`dashboard_contributions` and `network_http`.

## Build and stage

The worker has its own Cargo workspace and checked-in lockfile. It depends on
the local `desktop-plugins/worker-protocol` crate only for bounded stdio framing
and output. Identity, configuration, HA authentication and networking remain in
this worker. Build it separately from the host to keep machine load bounded:

```bash
CARGO_BUILD_JOBS=2 cargo build --release --locked \
  --manifest-path desktop-plugins/home-assistant/Cargo.toml
```

For a native Apple Silicon build, stage a new directory:

```bash
python3 scripts/plugins/prepare-plugin-package.py --plugin home-assistant \
  --worker desktop-plugins/home-assistant/target/release/inverter-home-assistant-worker \
  --target aarch64-apple-darwin \
  --output /private/tmp/home-assistant-package-stage
```

Use the actual compilation target on other systems and the `.exe` suffix on
Windows. The staging parent must already exist and the output directory must be
new. The helper accepts fixed built-in metadata and checks target executable
headers, bounded file contents, symlinks/reparse points, original path identities
and observable changes through its open handle. These checks do not establish
compiler provenance or a filesystem transaction. The helper does not execute the
worker, read keys, sign or install anything. It copies only the selected binary.

The result is `manifest.json` plus `payload/`. Pass them to the existing
[native package encoder](../../docs/plugin-packages.md#producing-an-archive) when
producing an archive. The old `prepare-frigate-package.py` command remains a
compatibility entry point for the Frigate package; it shares the same guarded
staging implementation.

## Validation boundaries

Run worker and shared-library checks independently:

```bash
cargo fmt --manifest-path desktop-plugins/home-assistant/Cargo.toml -- --check
CARGO_BUILD_JOBS=2 cargo clippy --locked --manifest-path desktop-plugins/home-assistant/Cargo.toml --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --locked --manifest-path desktop-plugins/home-assistant/Cargo.toml --all-targets -- --test-threads=2
CARGO_BUILD_JOBS=2 cargo test --locked --manifest-path desktop-plugins/worker-protocol/Cargo.toml --all-targets -- --test-threads=2
cargo audit --file desktop-plugins/home-assistant/Cargo.lock
cargo audit --file desktop-plugins/worker-protocol/Cargo.lock
python3 -m unittest discover -s tests -p 'test_*package.py'
```

CI runs formatting, strict Clippy and tests for both workers and the shared
library on Linux, macOS and Windows. Executable workers also receive release
builds and actual binary staging/header checks. Every independent lockfile has
its own advisory audit without borrowing host-only exceptions.

TLS subprocess fixtures use a disposable loopback certificate and child-only CA
file; they never alter the system trust store. All three desktop platforms check
untrusted-certificate rejection and verified WSS authentication. Linux also checks
selected initial HTTPS reads with the temporary CA. On macOS/Windows the HTTPS
platform verifier continues using OS trust, and the fixture expects rejection of
that disposable CA after WSS authentication. This does not establish successful
HTTPS against a production HA installation on those platforms.

The separately selected native acceptance test requires an actual compiled
worker, a fresh desktop frontend build, and a private Mosquitto executable for
an independent core telemetry probe. It supplies its own temporary HTTP and
WebSocket HA fixture; no production Home Assistant is contacted:

```bash
INVERTER_HOME_ASSISTANT_WORKER=/absolute/path/inverter-home-assistant-worker \
MOSQUITTO_BIN=/absolute/path/mosquitto \
CARGO_BUILD_JOBS=2 cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
  plugins::home_assistant_integration_tests::signed_home_assistant_package_lifecycle \
  -- --exact --ignored --test-threads=1
```

Ordinary native tests do not build another crate or silently launch this external
fixture. CI explicitly selects it and rejects zero-test success. Its temporary
signed package uses disposable fixture trust; it does not provision production
publishers. Process tests, signed-package lifecycle, hosted CI and a real HA
installation are separate evidence. Demonstrated local checks and remaining
hosted/runtime boundaries are recorded in [TODO.md](../../TODO.md).

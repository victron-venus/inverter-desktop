# Desktop plugin worker protocol

This document describes the first worker contract for optional desktop features.
The current host API is **1.4.0**, independently of the application version. The
wire protocol and package manifest each start at schema version **1**. Android
and iOS do not compile the worker host or include plugin UI contributions.

The contract supplies bounded messages and declarative dashboard contributions.
It does not make a native worker a sandbox, establish publisher trust, verify a
package signature, or install a package. Native workers have the operating
system access of the account that starts them. Permission declarations are not
an operating system sandbox.

## Transport and limits

A worker is an executable launched directly by the host, without a shell. Its
standard input receives host messages and its standard output carries worker
messages. Each message is a UTF-8 JSON object on one line, terminated by LF;
CRLF input is accepted. JSON strings may contain escaped line breaks. Pretty
printed JSON and several messages in one frame are rejected. Standard error is
reserved for diagnostic output and is never parsed as protocol data.

Each complete frame, including its terminating newline, is at most **65,536
bytes**. The host must bound reads before accumulating a complete line, not only
validate a line after reading it. Empty, malformed, oversized, unknown message
types and unknown fields are invalid. Labels are plain text; the contract has
no HTML, JavaScript, component source, navigation URL, Tauri command, or core
MQTT publish contribution.

The additional limits are:

- At most 64 dashboard contributions in one replacement snapshot, with unique
  IDs within that worker.
- Identifiers use ASCII letters, digits, `_`, `.`, `:`, and `-`, start with an
  ASCII letter or digit, and occupy at most 128 bytes.
- Plugin IDs use dot-separated lowercase components, each starting with a
  letter; subsequent characters can include digits and hyphens. A component
  cannot end in a hyphen. A plugin ID occupies at most 128 bytes.
- Titles and action labels occupy at most 128 bytes. Units occupy at most 32
  bytes. These strings cannot be blank or contain control characters.
- Text and status values occupy at most 4,096 bytes. Text can contain line breaks
  and tabs; status values cannot contain control characters.
- Action parameters must be JSON objects occupying at most 4,096 serialized
  bytes. Action results and worker event data occupy at most 16,384 serialized
  bytes. JSON data has at most eight levels below its root and 512 values; object
  keys occupy at most 128 bytes and cannot contain control characters.
- Action deadlines are relative milliseconds in the range 1–60,000. The host
  forwards the remaining budget immediately before writing, subtracting time
  spent in its queues and dropping requests with less than one millisecond left.
  Its original absolute deadline and cancellation remain authoritative, including
  during pipe transmission and when a worker does not cooperate.
- Error codes follow the identifier rules. Error messages occupy at most 1,024
  bytes.

Limits measured in bytes refer to UTF-8 or compact serialized JSON, as
applicable. A set of individually valid contributions must also fit the frame
limit.

## Handshake

The host starts with the expected identity and the API version it selected:

```json
{
  "type": "hello",
  "protocol_version": 1,
  "host_api_version": "1.4.0",
  "plugin_id": "org.example.weather"
}
```

The worker must acknowledge that exact identity and both selected versions
before sending data:

```json
{
  "type": "ready",
  "protocol_version": 1,
  "host_api_version": "1.4.0",
  "plugin_id": "org.example.weather"
}
```

A missing or mismatched acknowledgment fails startup. The worker's complete
supported host API range belongs in its manifest; `ready.host_api_version`
acknowledges the negotiated version and is not a version range. Identity comes
from the host's selected worker, not from an arbitrary frame claiming to be a
different plugin.

## Startup configuration (host API 1.1)

After a valid `ready`, a verified installed package declaring
`plugin_configuration` receives exactly one configuration frame on stdin:

```json
{
  "type": "configuration",
  "configuration": {
    "revision": "abceb85b-59bd-44d8-8135-e7f2ef873122",
    "values": { "endpoint": "https://example.invalid", "enabled": true },
    "secrets": { "token": "example-placeholder" }
  }
}
```

The worker must acknowledge the exact revision before contributing dashboard data
or accepting actions or sending notifications:

```json
{
  "type": "configuration_ready",
  "revision": "abceb85b-59bd-44d8-8135-e7f2ef873122"
}
```

The worker remains `starting` until both acknowledgments succeed. The existing
startup deadline covers both steps. Early contributions, missing/mismatched or
duplicate acknowledgments fail that generation. Packages without configuration
permission receive no configuration frame and complete startup after `ready`.
Workers must acknowledge the host API selected in `hello`; hard-coded 1.0 replies
are incompatible with a 1.4 host. The wire protocol and manifest remain version 1.
A configured worker should declare an API requirement such as `^1.1`.

The complete configuration object is bounded to 32 KiB, in addition to the 64 KiB
frame limit. Values and secrets are scoped to the verified plugin ID; fields not
present in its current schema are retained on disk but withheld from the worker.
The native service validates required values before launching. Missing required
setup therefore requires installing disabled, saving settings, then enabling.

Configuration is sent through the owned worker's startup pipe only. It is absent
from command-line arguments, inherited environment, runtime snapshots, UI events,
and host debug formatting. The pipe writer checks the originating authentication
epoch, stop signal, and startup deadline before delivery; an interrupted partial
frame fails that worker generation. A trusted native worker necessarily receives
its own secrets and must not echo them into diagnostics, contributions, or action
results. This is not process sandboxing or a guarantee against a malicious worker.

Saving settings restarts an enabled worker with the new configuration; disabled
workers remain stopped. There is no hot-reload or request-time secret API in this
checkpoint. See [plugin settings](plugin-settings.md) for the schema, encryption,
retention, revision, and failure contracts.

## Displayed action authority (host API 1.4)

Native dashboard snapshots include `instance_id`, an opaque identifier for the
actual worker process, or null when no live instance is available. A new process
always gets a fresh identity, including reinstall when generation counters repeat.
The authenticated `plugin_action` application command requires that displayed
identity as `instanceId`, plus the action ID and exact advertised preset params.
The host rejects stale instances before enqueueing, and continues checking the
session, generation, advertisement and deadline before writing the worker pipe.

The frontend also rejects changed clicked descriptors instead of substituting new
parameters. Pending and uncertain-result feedback belongs to the actual instance
and operation; changing a label or temporarily withdrawing an action does not
unlock duplicate dispatch. Snapshot requests are coalesced so live updates do not
starve visible state. Authentication changes and instance removal revoke old UI
work. None of this identity data is a publisher key or permission to bypass auth.

This extends the native dashboard boundary, not the worker wire schema. Existing
worker Action/Cancel/ActionResult/ActionError messages stay at protocol version 1.
HA packages with service actions require `^1.4` so they cannot be installed on an
older host without the displayed-instance check. Frigate remains compatible with
its existing API range.

## Dashboard contributions

A `contributions` message replaces that worker's entire current snapshot. An
empty `items` array removes all its contributions. The host associates the
snapshot with the selected worker process; items cannot choose another
plugin's namespace.

```json
{
  "type": "contributions",
  "items": [
    { "kind": "text", "id": "description", "title": "Conditions", "text": "Clear skies" },
    { "kind": "metric", "id": "temperature", "title": "Outside", "value": 21.5, "unit": "°C" },
    {
      "kind": "status",
      "id": "connection",
      "title": "Connection",
      "value": "Connected",
      "tone": "success"
    },
    {
      "kind": "action",
      "id": "refresh-card",
      "title": "Weather",
      "action_id": "refresh",
      "label": "Refresh",
      "params": { "force": true }
    }
  ]
}
```

Four contribution kinds are supported:

- `text`: `id`, `title`, `text`.
- `metric`: `id`, `title`, finite numeric `value`, and optional `unit` (omitted or
  `null` when unused).
- `status`: `id`, `title`, `value`, and `tone`. Tone is one of `neutral`,
  `success`, `warning`, or `error`.
- `action`: `id`, `title`, `action_id`, `label`, and object-valued `params`.

The UI renders text using text bindings. A string such as `<b>example</b>` is
literal text, not markup. Metric formatting and visual appearance belong to
the host. A worker supplies data, not application code.

## Native desktop notifications (host API 1.2)

A verified package declaring `desktop_notifications` may send a notification
only after its startup handshake and required configuration acknowledgment:

```json
{
  "type": "notification",
  "id": "motion-13d7bbf9",
  "title": "Frigate Garage camera motion detected",
  "body": "Motion started"
}
```

The ID follows the existing 128-byte identifier grammar. Title and body must be
nonblank plain text without control characters, bounded to 128 and 1,024 UTF-8
bytes respectively. Unknown fields, malformed content, missing permission, and
messages before readiness fail that worker generation. There are no URLs,
actions, images, sounds, Tauri commands, or arbitrary notification options.
Packages using this capability must declare a compatible range such as `^1.2`.

Delivery is best effort. Each registered worker has at most 16 queued notifications,
30 accepted notifications per minute, and 512 recent IDs remembered for ten minutes.
The host accepts at most 120 per minute globally; eight registered workers bound
the aggregate queue to 128. Valid duplicate, rate-limited, or overflowing messages
are dropped without failing the worker. The existing total frame-rate limit still
applies. Pending notifications expire after 30 seconds. Deduplication survives
automatic worker restarts; process/session replacement cannot revive a queued item.

The native dispatcher checks the original session epoch, running process generation,
stop signal, reaped state, and delivery age while holding the authority guard. The
application keeps one dedicated dispatcher and one pending signal, so slow OS
calls do not hold up the UI change-signal loop or create overlapping delivery tasks. Each started
dispatch checks live authentication before entering the authority guard. Actual
native submission happens inside the guard; the callback never defers unchecked
delivery to another application task. Linux notification-service waits are bounded
to two seconds; the macOS backend has its own two-second confirmation wait, whose
timeout can still return success. Both remain best-effort OS submission. Notification text is absent from manager
snapshots, webview events, and host logs. Freedesktop body markup is escaped so
worker text remains literal. Disable, uninstall, logout, and shutdown discard
pending work. Notifications already submitted to the operating system may remain
visible; OS permission, presentation timing, and notification-center retention are
outside the worker lifecycle contract. Native submission failures are not retried.

No plugin notification IPC or worker dependency is added to Android/iOS. Core
inverter notifications continue using their existing platform integration.

## Owned HTTP video (host API 1.3)

A package may declare `http_video` and `plugin_configuration`, with a matching
manifest declaration referencing one non-secret configuration field:

```json
{ "http_video": { "base_url_setting": "frigate_base_url" } }
```

The host derives an immutable grant from the same verified startup configuration
sent to that worker. An absent or blank base disables video requests while other
configured features can continue. A configured base must use HTTP(S), with no
userinfo, query, fragment, traversal, ambiguous encoded separators, or control
characters. Explicit ports and reverse-proxy prefixes are preserved. Invalid
candidate settings are rejected before an existing worker is stopped.

After configuration acknowledgment, the worker can submit:

```json
{
  "type": "http_video",
  "id": "frigate-clip-event-hash",
  "url": "http://frigate.example:5000/proxy/api/events/event-id/clip.mp4",
  "title": "Frigate Front camera motion detected"
}
```

IDs follow the bounded token grammar, titles use the notification plain-text
128-byte limit, and URLs are bounded to 2,048 bytes. The request must remain in
the configured origin and base path. The worker supplies no filesystem path,
window route, request headers, cookies, HA token, or core settings reference.
Downloads do not follow redirects or inherit the core HA credential lookup.
HTTP credentials and custom headers are outside this direct-URL contract.

Each worker has four pending requests, thirty admissions per minute, a ten-minute
512-ID history, and a 45-second title cooldown. Pending requests expire after
thirty seconds. The service independently bounds its queue to eight, transfers
to two, and active/download-reserved windows to eight. It reserves up to 256 MiB
per transfer within a 512 MiB media budget; completed smaller files release the
unused reservation. Valid excess traffic is dropped without failing the worker.
With `desktop_notifications`, admission also queues the existing bounded native
clip-available notification. HTTP or display failure can still follow admission.

The direct download policy retains eight attempts with 1/2/3/4/5/5/5-second retry
delays, a 15-second connect timeout, 60-second idle-read timeout, ten-minute overall
deadline, and 256 MiB per-file limit. Retryable responses include 400, 404, 408,
425, 429, and 5xx; empty or interrupted bodies can retry. Oversized responses fail
immediately. Playback begins only after the complete download succeeds.
Already-started disk operations are awaited before file cleanup, including after
cancellation; the network/retry deadline is not a forced interruption of filesystem I/O.

Each launch receives a fresh native instance identity and cancellation lease;
plugin ID, session epoch, and displayed generation alone are insufficient because
generation counters can repeat after registration. Disable, replacement, settings
restart, crash/restart, logout/expiry, uninstall, and shutdown revoke the original
lease. Long HTTP/disk operations never hold host authority locks. Cancellation
immediately denies media access and schedules owned-window destruction and file
cleanup. Native visibility is asynchronous, so already submitted display work can
briefly outlive its check; it cannot restore the revoked media authority.

Files live in a private sibling `desktop-plugin-media` directory under the package
manager's lifetime lease. The host creates hidden 330x186 windows, verifies their
ownership again before showing them, and waits for actual destruction acknowledgments
before releasing window slots. Failed native cleanup retains ownership and makes
shutdown fail visibly; a subsequent quit can retry. Closing one window does not
retire sibling media or reconnect core telemetry.

The player receives an opaque UUID through the `plugin-media` scheme. Requests are
bound to the exact requesting webview label and original running instance, with
live authentication checks before and after disk reads. No global temporary asset
scope is added. GET/HEAD support standard single byte ranges, at most 1 MiB per
range and four owned response buffers. Full GET for larger files is rejected;
native playback acceptance must establish range behavior on each supported webview.
Core window-targeting permissions are absent: close and drag use commands that
operate only on their native invoking window. Android/iOS include none of these
commands, routes, windows, workers, or media services.

The [native media smoke harness](native-plugin-media-smoke.md) exercises actual
desktop webview playback separately from the signed-worker/MQTT and HTTP-service
fixtures. It is an explicit feature-only example, not a production startup mode.

## Actions, cancellation, and results

Actions travel to the worker that contributed the selected action. They do not
invoke a Tauri command or publish a core MQTT message. The host assigns a
correlation ID and bounds pending requests and execution time:

```json
{
  "type": "action",
  "request_id": "request-42",
  "action_id": "refresh",
  "params": { "force": true },
  "deadline_ms": 5000
}
```

The worker answers with exactly one result or error using that ID:

```json
{ "type": "action_result", "request_id": "request-42", "value": { "updated": true } }
```

```json
{
  "type": "action_error",
  "request_id": "request-42",
  "code": "unavailable",
  "message": "The weather service did not respond."
}
```

The wire validator checks syntax, size, and identifiers. The host supervisor
owns correlation, pending request limits, deadlines, cancellation, and process
lifecycle. The host sends cancellation when it stops waiting:

```json
{ "type": "cancel", "request_id": "request-42" }
```

Cancellation is cooperative. It does not by itself prove that external work
was undone. Late results cannot complete a different request or resurrect an
expired request. Request state is isolated between worker process generations.

## Worker events and shutdown

A worker can send bounded data scoped to its own identity:

```json
{ "type": "event", "name": "refresh-complete", "data": { "items": 3 } }
```

`name` is not a Tauri event name. A host must not forward this string as an
arbitrary application event, interpret its data as a host command, or grant
access to core control transports. This protocol reserves the data envelope;
it does not introduce a general host capability broker.

The host requests cooperative shutdown with:

```json
{ "type": "shutdown" }
```

Workers should release their resources and exit. The host enforces its shutdown
budget and reaps the process when cooperation fails. Startup, cancellation, and
shutdown timeout values are supervisor policy, not worker-selected authority.

## Manifest metadata

The manifest contract defines package identity and compatibility:

```json
{
  "schema_version": 1,
  "plugin_id": "org.example.weather",
  "version": "0.1.0",
  "host_api": ">=1.0.0, <2.0.0",
  "target": "aarch64-apple-darwin",
  "entrypoint": "bin/weather-worker",
  "config_schema": {
    "type": "object",
    "properties": {
      "location": { "type": "string" }
    }
  },
  "permissions": ["dashboard_contributions", "network_http"],
  "inventory": [
    {
      "path": "bin/weather-worker",
      "size": 1024,
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }
  ],
  "signature": null
}
```

The example digest and file size are illustrative metadata, not a usable
package. A native development launcher can select a fixture executable through
trusted `WorkerSpec`. Application package selection instead goes through the
[verified preview and installation pipeline](plugin-packages.md); only that native
pipeline derives a worker executable from a verified installed package. Manifest
parsing by itself neither loads a package nor enforces its permission declarations.

Manifest parsing rejects unknown fields and permissions. The version
must be semantic version syntax; the host API requirement must match the
host's current API. Supported targets are:

- `aarch64-apple-darwin`, `x86_64-apple-darwin`;
- `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`;
- `aarch64-pc-windows-msvc`, `x86_64-pc-windows-msvc`.

`validate_for_target` additionally requires an exact match to the selected
host target. Mobile targets are invalid. A package identifies one target;
multi-target publishing uses separate package artifacts.

The manifest occupies at most 65,536 bytes. Its inventory contains 1–128 files
with at most 512 MiB of declared total content. Paths are portable relative
paths using `/`; absolute paths, traversal, empty components, backslashes,
drive prefixes, hidden components, reserved Windows names, and
case-insensitive collisions are rejected. The entrypoint must name a nonempty
inventory file. SHA-256 metadata is exactly 64 lowercase hexadecimal digits.
These checks do not read the listed files, reject symlinks in an installed
filesystem, or verify their digests. Those checks belong to package handling.

`config_schema` is bounded object-schema metadata, using the same depth and
node limits as message data and a 16,384-byte limit. Its root must declare
`"type": "object"`; schema references (`$ref`, `$dynamicRef`, `$recursiveRef`)
are unsupported. The package settings service further compiles the bounded
[declarative subset](plugin-settings.md) for its editor and startup configuration.
It does not fetch external schemas or migrate legacy feature configuration.

The permission vocabulary is `dashboard_contributions`,
`plugin_configuration`, `desktop_notifications`, `http_video`, `network_http`, and `network_mqtt`. Duplicate and
unknown permissions are rejected. These names declare feature requirements;
they do not grant Tauri commands, access to core MQTT controls, or operating
system isolation. Access to a network MQTT service from a native worker must
not be confused with a host API for publishing arbitrary core control messages.

A non-null `signature` object contains `algorithm: "ed25519"`, a bounded
`key_id`, and a `signature` string of exactly 128 lowercase hexadecimal digits.
The protocol parser validates its encoding only. The separate
[package pipeline](plugin-packages.md) defines canonical signed bytes, publisher
trust, cryptographic verification, and file verification. An unsigned or
syntactically signed manifest must never be described as a verified package based
on this parser alone.

## Compatibility and implementation

The Rust source of truth is `src-tauri/src/plugins/protocol.rs`. Worker and host
messages reject unknown variants and fields so unsupported capabilities fail
explicitly. Adding a wire field requires a coordinated protocol change; the
host API range alone does not bypass wire compatibility.

Unit tests cover the version and identity handshake, unknown fields and
operations, malformed and oversized frames, bounded action data and results,
contribution limits, duplicate IDs, finite metrics, incompatible host APIs,
mobile target rejection, unsafe paths, invalid inventory metadata, schema
references, and signature encoding without claiming signature verification.

## Application lifecycle and authority

Tauri owns one desktop `PluginHost` and application package service, shared by all
windows. A clean profile starts with no package workers. The current embedded
publisher policy is empty, so installation is disabled. After authentication, the
application may restore packages already recorded as enabled, verifying their
archives and installed payloads before starting fresh workers. Disabled packages
stay stopped; legacy HA/camera settings never imply installation. There is no
executable-path or start-worker IPC. `WorkerSpec` is a trusted native API, exercised
by a separately compiled fixture executable. The package manager constructs it only after package
verification; a manually constructed spec itself is not a package trust decision.

Only authenticated `main` and `config` windows can call `get_plugin_snapshot` or
`plugin_action`. The latter accepts a worker ID, an advertised action ID, and the
exact advertised parameters; it cannot select a core Tauri or MQTT command.
The host rechecks the process generation, current session epoch, and deadline
before dispatch. A logout or authentication-policy change synchronously revokes
the old epoch, clears contributions, and stops its workers. Logging in again
can restore enabled packages in a new epoch, but cannot revive old processes or
authorize their delayed responses. Background session expiry is checked at
one-second intervals while workers or package operations are active; each IPC
call independently requires a current session.

The fixed `plugin-host-update` event contains no worker data. The app coalesces
updates to at most 20 refresh signals per second, and windows retrieve an
authorized snapshot. The desktop dashboard renders text, metrics, status, and
preset actions; an empty host renders no plugin panel. The desktop Plugins settings tab manages package lifecycle through separate
settings-window-only IPC, including native selection and a single-use verified
preview token. The management frontend coalesces snapshot requests and the native
service caches inventory metadata by revision. Configuration-capable packages also
use a native-validated declarative settings editor with isolated encrypted storage
and startup configuration delivery. Worker-driven settings contributions, request-time
secret access and general network/media host services remain future work. Scoped
HTTP video uses the owned transfer and window contract above. Native
desktop notifications use the permission and delivery contract above. The first
[Frigate worker](../desktop-plugins/frigate/README.md) owns its MQTT connection;
`network_mqtt` is a declaration, not an OS firewall or a core MQTT host service.
The separate [Home Assistant worker](../desktop-plugins/home-assistant/README.md)
owns its authenticated REST/WebSocket connection and emits only declarative
connection/entity cards. Its `network_http` declaration similarly grants no
generic host proxy and provides no OS sandbox. Both workers reuse a small bounded
stdio library, while identity negotiation, configuration and network behavior
stay feature-owned. Their crates, binaries and manifests are excluded from mobile.

Normal application exit waits for package initialization and transactions, stops
and reaps workers, drains owned media/windows, and releases the package-store lease
before permitting exit.
Repeated quit requests continue waiting for the same cleanup. Shutdown also
prevents subsequent registration and actions. Runtime policy caps registered
workers, in-flight actions, pipe queues, startup time, message/update rates,
and restart attempts. These are protocol and process-management bounds, not
operating-system CPU or memory quotas. Forced termination of the host or an
operating-system failure is outside the normal shutdown contract.

The implementation uses cancellable asynchronous pipe I/O and direct process
management; see [Tokio process lifecycle](https://docs.rs/tokio/latest/tokio/process/index.html)
and [Tauri run events](https://docs.rs/tauri/latest/tauri/enum.RunEvent.html).

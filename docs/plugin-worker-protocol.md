# Desktop plugin worker protocol

This document describes the first worker contract for optional desktop features.
The host API starts at **1.0.0**, independently of the application version. The
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
  enforces its own deadline even when a worker does not cooperate.
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
  "host_api_version": "1.0.0",
  "plugin_id": "org.example.weather"
}
```

The worker must acknowledge that exact identity and both selected versions
before sending data:

```json
{
  "type": "ready",
  "protocol_version": 1,
  "host_api_version": "1.0.0",
  "plugin_id": "org.example.weather"
}
```

A missing or mismatched acknowledgment fails startup. The worker's complete
supported host API range belongs in its manifest; `ready.host_api_version`
acknowledges the negotiated version and is not a version range. Identity comes
from the host's selected worker, not from an arbitrary frame claiming to be a
different plugin.

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
are unsupported. The contract does not fetch external schemas or implement a
configuration editor or configuration migration engine.

The permission vocabulary is `dashboard_contributions`,
`plugin_configuration`, `network_http`, and `network_mqtt`. Duplicate and
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
service caches inventory metadata by revision. Plugin-provided settings, media,
and notification services remain later contribution surfaces; package management
does not implement those services.

Normal application exit waits for package initialization and transactions, stops
and reaps workers, and releases the package-store lease before permitting exit.
Repeated quit requests continue waiting for the same cleanup. Shutdown also
prevents subsequent registration and actions. Runtime policy caps registered
workers, in-flight actions, pipe queues, startup time, message/update rates,
and restart attempts. These are protocol and process-management bounds, not
operating-system CPU or memory quotas. Forced termination of the host or an
operating-system failure is outside the normal shutdown contract.

The implementation uses cancellable asynchronous pipe I/O and direct process
management; see [Tokio process lifecycle](https://docs.rs/tokio/latest/tokio/process/index.html)
and [Tauri run events](https://docs.rs/tauri/latest/tauri/enum.RunEvent.html).

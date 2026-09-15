# Home Assistant worker

`inverter-home-assistant-worker` is the separate desktop package
`inverter-desktop.home-assistant`, version 0.11.0, requiring host API `^1.6`.
Connection status and selected entity states are read-only by default. Optional
sensor-prefix discovery fills unused state slots without granting actions. Explicit
optional action lists enable fixed button presses, scene activation, media
transport, on/off controls, cover Open/Close/Stop, bounded numbers and cover positions. There is
no generic service proxy, core MQTT/IGW connection, inverter-control alias lookup,
camera authority or dependency on the bundled HA client.

Each selected entity's state and explicitly enabled controls appear in one
desktop card. The worker links controls to the existing state contribution with
`state_id`, using exact configured entity ownership. Equal friendly names do not
merge entities. Cover transport and position controls share the same state card;
read-only selections and discoveries remain without controls. The flat snapshot,
stable contribution/action IDs and 32-state/31-control limits are unchanged.
Availability and capability changes withdraw or restore controls under the same
state anchor; grouping adds no service authority and does not alter input revisions.
An explicit read-only dishwasher profile combines its assigned running and runtime
readings in the existing running entity card. Explicit washer and dryer profiles
show their reported remaining time in each selected entity's existing card.
Entity-picker UI and legacy configuration migration remain separate work.

The existing desktop HA integration remains bundled until its remaining features
have package parity. Android and iOS contain neither this worker nor the shared
worker protocol library, its package metadata, or the desktop plugin manager.
The production publisher policy remains empty, so installation is disabled in
shipped builds. This work creates no production publisher keys and adds no
application or worker code-signing/notarization prerequisite. Package signatures use the existing
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
  double escapes and control characters are rejected. HTTPS uses certificate
  verification and the matching secure WebSocket scheme. Redirects are not followed.
- `watch_entities`: optional string, default empty, at most 4,096 UTF-8 bytes.
  Separate entity IDs with commas or newlines. Blank entries are ignored and
  duplicate IDs count once. The ordered union with action targets and appliance roles is limited to
  32 entities: watched IDs come first, followed by previously unseen button/scene
  targets, media-player targets, on/off targets, cover targets, number targets
  and cover-position targets, then dishwasher running/duration and washer/dryer
  remaining-time roles. New selections preserve older target indices.
  Each ID is at most 128 bytes with two nonempty `domain.object_id` parts using lowercase ASCII letters, digits and
  underscores. IDs are preserved literally, including names resembling inverter
  control flags; there is no core alias resolution.
- `discovery_prefixes`: optional string, default empty, at most 1,024 UTF-8 bytes.
  Select up to eight unique literal prefixes separated by commas or newlines.
  Surrounding whitespace and blank entries are ignored; duplicate prefixes count
  once. Each prefix is at most 128 bytes and starts with `sensor.` or
  `binary_sensor.`. The optional object prefix uses only lowercase ASCII letters,
  digits and underscores, for example `sensor.kitchen_` or `binary_sensor.door_`.
  Bare `sensor.` and `binary_sensor.` select their entire respective domains.
  Matching is literal and case-sensitive, with no glob, regular expression or
  other domain support. Only valid literal entity IDs can be discovered. These
  read-only targets fill remaining slots after all explicit selections; discovery
  never creates fixed actions or numeric inputs. Leave empty to disable it.
- `dishwasher_running_entity`: optional string, default empty, at most 128 raw
  UTF-8 bytes. Assign one literal entity ID to the read-only dishwasher profile.
  Surrounding whitespace is trimmed; list separators and multiple IDs are rejected.
  The ID uses the same grammar as explicit watched entities, without inferring a
  role from its domain or friendly name. Leave empty to disable the profile.
- `dishwasher_duration_entity`: optional string with the same single-ID format,
  empty default and 128-byte bound. Assign the entity reporting runtime since
  midnight. Requires a nonempty, different running entity. Both roles add explicit
  watched reads and count once in the 32-entity union, but never grant controls.
- `washer_remaining_entity` and `dryer_remaining_entity`: optional strings,
  default empty, each at most 128 raw UTF-8 bytes. Each assigns one literal entity
  reporting that appliance's remaining time, using the single-ID format above.
  Either role can be configured independently. The two remaining-time roles must
  differ from each other and from the dishwasher running entity: each state slot
  can have only one primary profile. They may overlap with watched/control
  selections or the dishwasher duration entity. New IDs are appended after the
  existing dishwasher roles; the same 32-state limit applies. Leave a role empty
  to disable its profile. Selection grants reads only.
- `action_entities`: optional string, default empty, at most 4,096 UTF-8 bytes.
  Explicitly select up to 16 unique literal `button.*` or `scene.*` IDs separated
  by commas or newlines. Targets are also watched within the total 32-entity
  limit. Only this list enables button/scene actions; selecting an entity for reads never does.
  Other domains and arbitrary service definitions are rejected.
- `media_player_entities`: optional string, default empty, at most 4,096 UTF-8
  bytes. Explicitly select up to four unique literal `media_player.*` IDs separated
  by commas or newlines. Each selected player offers Play, Pause and Stop and is
  watched within the shared 32-entity limit. This list does not alter button/scene
  action indices. Watching a media player alone grants no transport actions.
- `binary_entities`: optional string, default empty, at most 4,096 UTF-8 bytes.
  Explicitly select up to eight unique literal `switch.*`, `input_boolean.*` or
  `light.*` IDs separated by commas or newlines. Targets are also watched. Each
  offers Turn on and Turn off while its observed state is exactly `on` or `off`.
  Other domains, generic toggle, brightness and color settings are not supported.
  Selecting one of these entities only in `watch_entities` grants no write access.
- `cover_entities`: optional string, default empty, at most 4,096 UTF-8 bytes.
  Explicitly select up to four unique literal `cover.*` IDs separated by commas
  or newlines. Targets are also watched. Open, Close and Stop are available only
  for operations the entity supports while HA reports a known cover state.
  Watching a cover alone grants no actions. This list grants no position writes;
  those require the separate selection below. Tilt, speed and toggle are unsupported.
- `number_entities`: optional string, default empty, at most 4,096 UTF-8 bytes.
  Explicitly select up to four unique literal `number.*` IDs separated by commas
  or newlines. Targets are also watched. Eligible observations supply a bounded
  input using HA's current minimum, maximum and step. `input_number.*` is not
  supported in this slice. Watching a number alone grants no write access.
- `cover_position_entities`: optional string, default empty, at most 4,096 UTF-8
  bytes. Explicitly select up to four unique literal `cover.*` IDs separated by
  commas or newlines. Targets are also watched. Position inputs require a known
  cover state, a valid observed position and HA's set-position capability.
  Selecting a position target does not grant Open, Close or Stop.
- `ha_token`: required write-only token, 1–4,096 bytes of visible ASCII without
  whitespace or control characters. It is not read from core configuration or
  placed in arguments, inherited environment, dashboard contributions or logs.
  The token retains its account's Home Assistant permissions; this package does
  not create a restricted server-side token. Reads and explicitly selected
  button/scene/media/on-off/cover/number actions use that token.

All six action lists together may produce at most 31 controls: count one
per button or scene, three per media player, two per on/off target and three per
cover, even if a selected cover supports fewer operations, and one per number
or cover-position target. Alongside
connection status and up to 32 state cards, this keeps each snapshot within the
host's 64-contribution limit. Duplicate IDs count once within their list. All
previously valid configurations remain within this budget when new fields are empty.
Omitted or empty new fields also preserve the prior serialized 32 KiB
configuration validation boundary.
The two numeric fields, `discovery_prefixes` and all appliance roles use host API
1.5's explicit `omitEmpty` schema option:
empty values remain visible in settings, use the empty default when saved, and
add no bytes to startup configuration. This preserves native storage/envelope
limits as well as worker-side validation; nonempty selections are counted normally.

Explicit selections use individual initial REST reads at
`/api/states/<entity_id>` beneath the configured prefix. Only enabled discovery
with free state slots adds the bounded collection read described below. The
worker never sends WebSocket `get_states`. Live updates use `subscribe_events`
with `event_type: state_changed`. **That server stream covers all entities**:
the worker filters it locally to explicit selections and eligible discoveries.
This is not server-side subscription filtering or a token-level access restriction.

When `watch_entities`, all six action lists, `discovery_prefixes` and all appliance roles are empty,
the worker still establishes the authenticated WebSocket connection for status,
but makes no entity REST reads or event subscription. A change to any list is
applied through the normal settings restart. Entity state
and labels are bounded plain data; the host renders contributions, and the worker
supplies no frontend code or arbitrary navigation URLs.

The worker subscribes before starting initial reads. A live update or deletion
received during a slow initial read wins over that older REST result. Numeric
states become metric cards; other values remain plain text, with explicit unknown
and unavailable states. Disconnect clears stale values. Dashboard updates are
coalesced to four per second, independently of socket reads and heartbeat checks.
Authentication rejection stops reconnect attempts until settings restart the
worker; network failures reconnect with a bounded delay.

## Read-only dishwasher profile

Select `dishwasher_running_entity` and optionally `dishwasher_duration_entity`
in the plugin's settings. Selection is explicit and independent of the bundled
HA settings. The profile uses the running entity's existing state ID and friendly
title; the duration entity card stays independently visible and can itself have
an explicitly configured remaining-time profile. It adds no state slot,
contribution kind, action, automatic discovery or host API requirement.

The summary labels the operating state as `State: Running` for `on` or `running`,
and `State: Idle` for `off` or `idle`, after trimming and ASCII case folding for
these comparisons. Other accepted states remain literal after `State:` instead
of being guessed idle. Unknown, unavailable or missing primary state uses the
existing status card. An empty, overlong or malformed primary value shows
Unavailable. Accepted source state strings are trimmed, at most 128 UTF-8 bytes,
and contain no embedded control characters; fields are kept whole.

A valid duration reading adds a new line, for example
`Runtime since midnight: 01:23:45`. It remains visible while idle because it is
cumulative runtime, not remaining time. The worker accepts a complete trimmed
source state string of at most 128 UTF-8 bytes, with no embedded controls. Empty,
unknown, unavailable, off, idle and nonfinite numeric strings are omitted.
Other literal and finite numeric strings are kept as reported; the worker does
not parse duration formats, append an attribute unit, infer units, convert values
or start a countdown. Invalid duration removes only the detail, not the primary
state. The combined text remains within the existing 512-byte limit.

Updates or deletion of either role refresh the composite. Each source keeps its
own live-over-initial-REST ordering, and reconnect, disconnect or authentication
failure clears retained readings. A settings restart replaces the previous
instance normally. The overlay preserves raw entity observations and action
eligibility: a profile alone grants no writes, while separately selected controls
keep their existing authority, IDs, parameters and state references. Reads use
the existing individual state endpoints and filtered `state_changed` events.
There are no new service routes or core MQTT commands.

## Read-only washer and dryer profiles

Select `washer_remaining_entity` and/or `dryer_remaining_entity` in the plugin's
settings. The two roles work independently and use their selected entity's
existing state ID and friendly title. For example, a source state of `10:02`
produces `Remaining time: 10:02`; `0` and `1.500` remain exactly those strings.
The worker preserves complete trimmed strings of at most 128 UTF-8 bytes with no
embedded controls. Attributes do not affect this projection. It does not infer
activity from digits, parse duration formats, append assumed units, convert
values or tick a local countdown.

Known `unknown` and `unavailable` readings use neutral Unknown and Unavailable
status cards; `off` and `idle` use Idle, after trimming and ASCII case folding.
Missing, malformed, empty, oversized or nonfinite numeric readings show
Unavailable. Other complete literals remain reported data after `Remaining time:`.
A zero reading remains visible. This differs from the bundled section's digit
heuristic and does not claim its visibility behavior or full appliance UI parity.

Each accepted source update or deletion replaces the displayed reading. Literal
changes such as `1.50` to `1.500` publish even if a generic numeric metric would
compare equal. Both profiles retain the existing live-over-initial-REST ordering
and clear with session teardown or settings replacement. If one source is also
the dishwasher duration, its update changes both projections before publication;
its independently visible state slot shows the explicitly selected laundry profile.
No profile stores raw attributes or changes the original action/numeric observation.

A remaining-time selection never grants an action. To enable an appliance's
existing HA start/pause buttons, separately select their literal `button.*` IDs
in `action_entities`; existing action availability and state grouping apply to
those button entities. Profiles reuse the existing `state_changed` subscription and add no service
endpoint, action ID, permission, state slot or core MQTT command. Bundled `ha_washer_*` and `ha_dryer_*`
settings are retained separately and are not automatically migrated.

## Read-only weather summaries

Add a literal `weather.*` entity to `watch_entities` to display its observed
condition and temperature in one existing text card. Temperature values must be
finite JSON numbers, and the entity must supply an explicit `temperature_unit`.
Numbers retain their parsed JSON number token, limited to 32 bytes. Conditions are
limited to 128 UTF-8 bytes and units to 32; empty, invalid or overlong fields are
omitted rather than partially displayed. Invalid main conditions show Unavailable.
Control characters are removed before applying field byte limits; surrounding
whitespace is trimmed from accepted fields. No float formatting rounds a value.
The worker does not assume Celsius, use a sensor's `unit_of_measurement`, convert
units or grant any weather controls. Friendly names and stable state IDs follow
the same rules as other explicit selections. Weather is not included in sensor
discovery.

When the state already contains a legacy `attributes.forecast` array, the worker
examines only its first five entries. It displays valid supplied date labels,
conditions and finite high/low temperatures with the entity's explicit unit.
Dates must be calendar-valid dates or zoned timestamps. Each complete forecast
entry starts on a new line and is included only if it fits the 512 UTF-8 byte
summary limit. Malformed fields are omitted; temperature values are kept whole
and no extra state slots are allocated.
Attribute-only updates replace the card, including removal of a previously
observed temperature or forecast. Unknown, unavailable, missing and disconnected
entities use the existing status card instead of retaining weather details.

Modern Home Assistant [forecasts use a separate API](https://developers.home-assistant.io/docs/core/entity/weather/)
and are not part of the entity state. This compatibility projection neither
retrieves nor promises a forecast when the state lacks one. Separate forecast
subscriptions and the bundled forecast layout remain pending. All weather data
comes from the same initial REST state read and filtered `state_changed` updates;
there are no added network requests, endpoints, subscriptions or service calls.

## Read-only sensor discovery

Discovery is disabled by default. When prefixes are configured and explicit
targets occupy fewer than 32 state slots, each authenticated connection makes
one GET `<base>/api/states`. This response contains the all-entity collection,
not a server-filtered prefix result. The request has a 15-second deadline, a
1 MiB response limit and at most 4,096 source items. Only matching
`sensor.*`/`binary_sensor.*` state projections are retained. Discovery adds no
permissions, host API version, frontend contribution kind or write capability.

All explicit watched and control targets retain their ordered indices and
priority. Discovered targets fill at most `32 - explicit target count` state
slots, excluding any explicit target. Initial matches are selected in lexical
entity-ID order. Existing limits remain 32 state cards, 31 controls and 64 total
contributions, with the same two concurrent service requests. A full explicit
selection skips the collection request entirely.

The worker subscribes before the collection read. While it is pending, a bounded
buffer keeps the latest projected live update or deletion for at most 128 matching
entity IDs. Those updates take precedence over the older snapshot. Buffer overflow,
an oversized or invalid response, timeout or another collection failure disables
discovery for that connection and shows a warning in the existing connection card.
Explicit reads and controls continue; a 401/403 authentication rejection still
halts the whole session and cancels its pending work. A later connection starts a
fresh discovery attempt. There is no periodic collection refresh or retry within
the same connection.

After bootstrap, live additions use free discovery slots and deletions release
their slots. Unknown or unavailable states stay visible as read-only status
cards; malformed live states remove their discovered row. At capacity, unseen
matches are discarded and the connection card
shows a limit indication until reconnect. The worker keeps no hidden catalog or
overflow queue to backfill later: a newly free slot needs a later matching live
update or the next connection's snapshot. Disconnect clears discovered state,
and reconnect resamples the collection. Discovery failures do not change explicit
action IDs, presets, numeric grants or core transport ownership.

Discovery does not add an entity picker, infer appliance roles, retrieve forecasts or provide
grouped household layouts or automatic migration of bundled HA settings. The
bundled desktop integration remains available while that parity work continues.

## Explicit button and scene actions

A configured `button.*` target offers a press button; a `scene.*` target offers
activation. Labels identify the selected target. The worker publishes the action
only while HA is connected and that entity has been observed and is not deleted
or unavailable. An `unknown` state before first button/scene use is valid. The
worker rechecks availability at dispatch, independently of the host's Running
state, and resolves the target from configuration rather than caller parameters.

Each preset has empty params and a stable `ha-action-<index>` ID in configured
action order. The fixed request is POST `<base>/api/services/button/press` or
`<base>/api/services/scene/turn_on`, with only `{"entity_id":"<literal target>"}`
in its JSON body. Explicit URL ports/prefixes and verified TLS are preserved.
Redirects and automatic HTTP retries are disabled. No action uses the legacy
`perform_action` path, core alias resolution or a fallback to MQTT.

At most two service requests can be active; additional requests are rejected
without a service backlog. Responses are limited to 1 MiB and never forwarded to
the dashboard. The worker honors the original 1–30,000 ms deadline from receipt
(the app normally supplies five seconds, minus time spent in host queues),
correlates results, and keeps live reads and heartbeat running during a stalled POST. Cancellation, disconnect,
authentication rejection and worker teardown stop pending local work.

A timeout, dropped response or cancellation can follow an operation HA already
accepted. The worker never retries a service POST automatically or claims that
cancellation undoes it. The dashboard reports that the result could not be
confirmed and asks the user to check state. State cards continue to follow HA
updates, rather than assuming the HTTP result proves a physical device change.

Host API 1.4 binds each dashboard click to the actual displayed worker instance
and exact preset. A settings restart or reinstall cannot redirect an old click
to a newly configured target, even if a generation counter or action ID repeats.
All six action lists must be empty to preserve the existing read-only behavior.

## Explicit media-player transport

Each configured, connected and observed media player offers three fixed actions:
`ha-media-<index>-play`, `ha-media-<index>-pause` and `ha-media-<index>-stop`.
Missing, deleted, `unknown` and `unavailable` players offer no actions. Labels
identify the literal selected player; a state card continues to follow HA updates.

Presets always have empty params. The worker sends POST beneath
`<base>/api/services/media_player/`, choosing exactly `media_play`, `media_pause`
or `media_stop`, with only `{"entity_id":"<literal target>"}` in the JSON body. Callers cannot select an
arbitrary operation or target. Existing button/scene indices remain unchanged.
All actions share the same two-request concurrency limit, response bound,
remaining deadline, cancellation, no-retry and unknown-outcome handling described
above. A successful response does not establish physical playback or undo effects
after cancellation.

Without on/off, cover or numeric targets, connection status, 32 state cards, 16 button/scene actions
and 12 media actions produce at most 61 contributions. The combined limits above
allow up to 64 when additional controls are selected. The maximum snapshot must also
fit the existing 64 KiB frame limit. These actions need no new host UI
contribution, permission or protocol version.

## Explicit on/off controls

Each selected switch, helper or light supplies `ha-binary-<index>-on` and
`ha-binary-<index>-off`, in configured order, without changing existing button,
scene or media IDs. Both commands remain available when HA reports either exact
`on` or `off` state. Missing, deleted, unknown, unavailable or malformed states
withdraw both actions, as does a disconnected session. The worker rechecks the
current state when admitting a command.

Each preset has empty parameters. The worker chooses one of six fixed routes:
`switch/turn_on`, `switch/turn_off`, `input_boolean/turn_on`,
`input_boolean/turn_off`, `light/turn_on` or `light/turn_off`, beneath
`<base>/api/services/`. The JSON body contains only the literal `entity_id`.
These match the official [switch actions](https://www.home-assistant.io/integrations/switch/#list-of-actions),
[input boolean actions](https://www.home-assistant.io/integrations/input_boolean/#list-of-actions)
and [light actions](https://www.home-assistant.io/integrations/light/).

Commands set an explicit state; they never infer an inverse state or call
`toggle`. A successful HTTP response does not modify the displayed state. Only
HA reads and state events update that value, so an accepted request whose device
has not changed still shows the previous state. All commands share the established
concurrency, deadline, cancellation, authentication and no-retry rules.

An HA entity named `input_boolean.do_not_supply_charger` remains a literal HA
target. It is never resolved as an inverter-control alias or published to a core
MQTT topic. Any behavior attached to that entity inside HA remains owned by HA.

## Explicit cover controls

Each selected cover has stable `ha-cover-<index>-open`,
`ha-cover-<index>-close` and `ha-cover-<index>-stop` presets with empty parameters.
Existing button, scene, media and on/off IDs remain unchanged. The worker derives
exactly `cover/open_cover`, `cover/close_cover` or `cover/stop_cover` beneath
`<base>/api/services/`, sending only the literal `entity_id` in the JSON body.
These follow the official [cover actions](https://www.home-assistant.io/integrations/cover/#list-of-actions).

The current connected session must have observed exact `open`, `closed`,
`opening` or `closing` state. Each operation also requires its corresponding
bit in the unsigned integer `attributes.supported_features`: Open 1, Close 2 or
Stop 8. Missing or malformed features grant no controls; unrelated bits grant
no extra operations. See the [HA cover feature contract](https://developers.home-assistant.io/docs/core/entity/cover/#supported-features)
and [feature constants](https://github.com/home-assistant/core/blob/dev/homeassistant/components/cover/const.py).
Supported operations remain available in all four known states, including while
the cover is moving. Unknown, unavailable, deleted or malformed observations
withdraw commands. Capability changes update controls even when the displayed
state and name stay the same; current state and features are rechecked at admission.

A successful service response never synthesizes cover movement or position.
Only HA reads and events change the displayed state. All actions share the
existing deadlines, two-request limit, authentication and no-retry rules.
Canceling a request stops local waiting; it neither reverses device movement nor
sends Stop. Stop requires a separate explicit click and is rejected while both
request slots are occupied. The user must check HA state after an unknown result.

## Explicit bounded numeric inputs

Each eligible number supplies `ha-number-<index>-set`; each eligible position
target supplies `ha-cover-position-<index>-set`. Both use the host API 1.5
`number_input` contribution. The UI shows the observed value, bounds and step,
keeps edited drafts separate from live state, and submits only after Apply.

For numbers, HA must report a numeric state and numeric `min`, `max` and `step`
attributes. The worker parses decimal literals exactly, including exponent
notation, and admits only values representable with at most six decimal places
and integer coefficients bounded to 10^15. The state must lie on the grid
anchored at the minimum. Precision derives from the bounds and step, so eligible
value updates alone retain the input revision. Unsupported precision, malformed
metadata, non-string units or off-grid observations retain the read-only state card
but withdraw its numeric input. String units follow existing display normalization:
strip controls, trim and bound to 32 UTF-8 bytes, with empty or null meaning no unit.
Revisions track the effective published unit. The worker also rejects reserved JSON number
objects before deserialization, preventing objects from masquerading as numbers.

Position inputs require exact `open`, `closed`, `opening` or `closing` state,
an unsigned feature mask containing bit 4 (`SET_POSITION`), and integer
`current_position` from 0 through 100. Their range is 0–100 with step 1 and unit `%`.
Other cover features do not grant position authority.

Only `{input_revision, value_scaled}` is accepted for these operations. The
worker checks the currently eligible and published grant, converts the integer
coefficient to an exact JSON number, then calls `number/set_value` with exactly
`{entity_id, value}`, or `cover/set_cover_position` with exactly
`{entity_id, position}`. Targets and service routes cannot be supplied by callers.
These follow HA's [number action](https://www.home-assistant.io/actions/number.set_value/)
and [cover-position action](https://www.home-assistant.io/actions/cover.set_cover_position/).

Changes to eligibility, bounds, step, precision or unit rotate the revision.
Withdrawal and restoration also require a new revision; ordinary observed-value
updates do not. Both host and worker reject stale revisions. Numeric operations
share the existing two request slots, deadlines, cancellation and no-retry rules.
Service success does not change the displayed observation: only HA reads and
events do. Cancellation never issues Stop or restores a previous number.
Tilt, speed and other parameterized services remain pending. No new permission,
core MQTT route, production publisher key or mobile dependency is introduced.

The wire behavior follows the official
[WebSocket API](https://developers.home-assistant.io/docs/api/websocket/) and
[REST API](https://developers.home-assistant.io/docs/api/rest/).

The `network_http` permission describes this trusted native executable's direct
HTTP/WebSocket behavior. It is not an OS network sandbox or a host API for arbitrary
requests. The package declares only `plugin_configuration`,
`dashboard_contributions` and `network_http`.

## Build and stage

The worker has its own Cargo workspace and checked-in lockfile. It depends on
the local `desktop-plugins/worker-protocol` crate for bounded stdio framing,
JSON number-object rejection and output. Identity, configuration, HA authentication and networking remain in
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

The separately selected native acceptance tests require an actual compiled
worker, a fresh desktop frontend build, and a private Mosquitto executable for
an independent core telemetry probe. It supplies its own temporary HTTP and
WebSocket HA fixture; no production Home Assistant is contacted:

```bash
INVERTER_HOME_ASSISTANT_WORKER=/absolute/path/inverter-home-assistant-worker \
MOSQUITTO_BIN=/absolute/path/mosquitto \
CARGO_BUILD_JOBS=2 cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
  plugins::home_assistant_integration_tests::signed_home_assistant_package_lifecycle \
  -- --exact --ignored --test-threads=1
# Repeat with signed_home_assistant_package_actions, signed_home_assistant_package_media
# signed_home_assistant_package_binary, signed_home_assistant_package_cover
# signed_home_assistant_package_numeric and signed_home_assistant_package_discovery
# for their respective controls and read-only discovery.
```

Ordinary native tests do not build another crate or silently launch this external
fixture. CI explicitly selects it and rejects zero-test success. Its temporary
signed package uses disposable fixture trust; it does not provision production
publishers. Process tests, signed-package lifecycle, hosted CI and a real HA
installation are separate evidence. Demonstrated local checks and remaining
hosted/runtime boundaries are recorded in [TODO.md](../../TODO.md).

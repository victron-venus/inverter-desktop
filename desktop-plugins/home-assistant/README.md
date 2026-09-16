# Home Assistant worker

`inverter-home-assistant-worker` is the optional desktop package
`inverter-desktop.home-assistant`, version **0.13.0**, requiring host API **^1.8**.
It owns its HA HTTP/WebSocket session, selected state reads, explicitly configured
household commands, compact dashboard presentation, entity picker catalog and
optional notifications. The host renders bounded data and invokes exact current
contribution descriptors; the package supplies no frontend code or remote assets.

Core inverter controls remain in MQTT/IGW and do not require this package. A
literal HA entity, including `input_boolean.do_not_supply_charger`, always stays
an HA target. No HA command resolves an inverter-control alias or falls back to
core MQTT. Android and iOS include neither this worker nor the desktop plugin
runtime, package manager or presentation system.

Configured HTTPS downloads use the exact saved archive SHA-256, version and
target. Manual signed packages retain their publisher policy. Neither mode
requires application or worker code signing or notarization.

## Settings and ownership

Settings and write-only secrets use the encrypted per-plugin store. The host
sends the exact settings revision through the startup pipe; validation completes
before network access. Saving settings restarts an enabled worker. Disable,
logout, uninstall and application shutdown cancel the worker's local work.

- `ha_base_url`: required complete HTTP(S) address, maximum 2,048 UTF-8 bytes.
  Include the actual port and optional proxy prefix; no port is inferred.
  Credentials, query, fragment, whitespace, backslashes and ambiguous encoded
  separators/dot segments are rejected. Safe encoded prefixes are preserved.
  HTTPS verifies certificates; redirects are not followed.
- `ha_token`: required write-only token, 1–4,096 visible ASCII bytes. It is sent
  only to this configured HA origin. It is absent from process arguments,
  inherited environment, contributions and logs. Its existing HA account
  permissions still apply; the package does not restrict that server-side token.
- `watch_entities`: comma/newline-separated literal entity IDs, at most 8,255
  bytes. An ID is at most 128 bytes and consists of two nonempty lowercase ASCII
  letter/digit/underscore parts separated by one dot. Duplicates count once.
- `action_entities`: explicit `button.*`/`scene.*` targets, at most 63.
- `media_player_entities`: explicit media players, at most 21; three commands each.
- `binary_entities`: explicit switch/input_boolean/light/fan targets, at most 31;
  two commands each.
- `cover_entities`: explicit covers, at most 21; three commands each.
- `number_entities` and `cover_position_entities`: explicit numeric targets,
  at most 63 each; one input each. `input_number.*` is not supported.
- `dishwasher_running_entity`, `dishwasher_duration_entity`,
  `washer_remaining_entity`, `dryer_remaining_entity`: optional exact single
  entity IDs, maximum 128 bytes each. Duration requires a distinct dishwasher
  running target. Washer, dryer and dishwasher primary roles must differ.
- `discovery_prefixes`: up to eight literal sensor/binary_sensor prefixes, at
  most 1,024 bytes. Bare `sensor.`/`binary_sensor.` selects the domain; object
  prefixes accept lowercase letters, digits and underscores. No wildcard or regex.
- `discovery_domains`: comma/newline-separated domain names, maximum 256 bytes.
  Supports switch, light, input_boolean, fan, cover, lock, media_player, scene,
  script, number, sensor, binary_sensor, climate, button and weather. Discovery
  grants reads only, and fills free state slots after all explicit targets.
- `notify_home`: optional boolean, false by default.
- `dashboard_layout`: optional bounded JSON string, at most 24 KiB. The package
  manifest exposes a structured editor, entity selectors and explicit control positions;
  users do not need to edit raw JSON. Its default enables the compact view below.

Every entity list is bounded to 8,255 bytes. All explicit read, action, input,
layout and appliance targets share **64 unique state slots**. All command lists
and layout-primary targets together share **63 controls**. Count one per
button/scene, three per player, two per binary target, three per cover, one per
numeric input and one per unique layout-primary target. Cover reservations count
all three operations even if currently unsupported. An over-capacity configuration
is rejected; commands are never silently omitted to make it fit.

The complete configuration remains at most 32 KiB, contributions at most 128,
and each newline-delimited worker frame at most 64 KiB. New default values use host API 1.8 `omitDefault`: settings still display the
defaults, while exact defaults consume no stored/startup-envelope bytes. The
worker uses compact v1 when layout is absent, and skips that exact default during
its own byte validation. Empty discovery and false notifications also add no bytes.
These byte limits also apply to entity names and JSON escaping; individual field
maxima are not a promise that all maxima fit together.

## Compact presentation and explicit primary commands

A version-one layout has this shape:

```json
{
  "version": 1,
  "controls": [
    {
      "id": "home-lamp",
      "surface": "home",
      "order": 0,
      "label": "Desk lamp",
      "entity": "light.desk",
      "icon": "light"
    }
  ],
  "sections": {
    "sensors": true,
    "numbers": true,
    "covers": true,
    "media": true,
    "scenes": true,
    "weather": true,
    "washer": true,
    "dryer": true,
    "dishwasher": true
  },
  "appliances": { "washer_start": "button.washer_start", "washer_pause": "button.washer_pause" }
}
```

Controls preserve their configured labels, placement (`header` or `home`) and
order. IDs are unique ASCII alphanumeric/`-_.` strings, begin with an alphanumeric
character, and are at most 64 bytes; internal `ha-*` presentation IDs are reserved.
There are at most 64 placements. Two placements of the same entity share one
primary command. Icons are a finite host-owned set: home, plug, light, washer,
dryer, dishwasher, thermometer, gauge, blinds, play and cloud.

A control's trimmed, case-folded state is on for on/open/opening/unlocked,
unavailable for missing/empty/unknown/unavailable, and off otherwise. Unavailable
controls have no action reference. Every available reference points to an exact
action contribution in that same atomic snapshot; equal friendly names never
merge entities. Missing compact metadata produces no inferred HA dashboard.
Version 0.13 intentionally enables compact v1 when layout is omitted. An explicit
empty layout string opts out and preserves the old flat contributions wire shape;
the host does not mount duplicate flat panels. Older package versions remain
compatible with the host through their existing API ranges, but compact dashboard
features require version 0.13 or later. An explicitly pinned older archive is not
automatically replaced by an unpinned newer package.

Primary commands match the legacy explicit dispatch:

- Button: `button/press`; scene: `scene/turn_on`.
- Cover: `cover/set_cover_position` with `position: 0`.
- Number: `number/set_value` with `value: 0`.
- Switch, input_boolean, light, fan, media_player, lock, script, climate, sensor
  and binary_sensor: first GET the **current exact entity state**. Literal `on`
  selects that domain's `turn_off`; every other returned string selects `turn_on`.
  This deliberately preserves the old explicit domain routes, even if an HA
  integration does not implement a particular service. A missing, mismatched,
  malformed or failed fresh read sends no service operation. There is no retry,
  optimistic cached toggle or core MQTT fallback.

The fresh read and service POST share the original action deadline. Callers
cannot change the target, route or fixed request parameters. Settings changes
replace the worker instance, so an old visible descriptor cannot target a newly
configured entity.

Collapsed sensor, number, cover, media and scene groups reference exact read
contributions and explicit action/input contributions. Read discovery never
adds a command. Unknown/unavailable covers remain visible; unavailable rows in
other groups are hidden. Numeric display preserves reported literal precision.
Appliance summaries keep remaining/runtime strings without inferred units or
countdowns. Washer/dryer activity follows the legacy nonzero-digit rule;
dishwasher activity requires case-folded `on` or `running`. Section visibility
is also applied. Start/pause references are independent and use exact targets
from `appliances` (`washer_start`, `washer_pause`, `dryer_start`, `dryer_pause`).

Weather includes the first available selected weather entity, its condition,
temperature/unit and up to five valid supplied forecast entries. Missing units
use the legacy compact Celsius default. Temperatures are finite numeric tokens,
not rounded floats. The worker projects `attributes.forecast` when present; it
does not invent forecasts or issue a newer HA forecast subscription. Flat-mode
weather remains its previous bounded text summary with explicit supplied units.

## Read lifecycle, discovery and entity choices

The worker authenticates WebSocket, subscribes to `state_changed`, then starts
individual REST reads for explicit targets. That HA stream contains all entity
events; the worker filters locally. This is not server-side access restriction.
Live updates/deletions received while initial reads are pending win over older
REST responses. Disconnect clears stale state and current command availability.
Updates are coalesced to four per second; heartbeats and network reads remain
independent of slow host output or service requests.

Enabled discovery with free capacity requests GET `<base>/api/states` once per
connection. A compact layout requests that same bounded collection for its
settings catalog even if all 64 read slots are occupied. Limits are 15 seconds,
1 MiB and 4,096 source entities. Discovery fills slots lexically after explicit
reads; live bootstrap updates override the collection. Overflow or malformed
bootstrap disables discovery for that connection and shows a status warning.
Later live additions use free slots; unseen overflow does not grant writes or
create a hidden state backlog. Reconnect samples again.

Supported catalog entities are independent of selected read slots. The worker
publishes `settings_choices` events in contiguous bounded pages, one per publish
tick, with one revision and a final completion marker. Labels are bounded plain
friendly names; the host exposes choices only to authenticated plugin settings.
Catalog choices grant no reads or writes. The structured layout editor uses the
same catalog for exact target selection.

With no explicit targets and no discovery, the worker still connects for status.
Compact layout may fetch its catalog but does not subscribe merely to populate
choices. Network failures reconnect with bounded delay; authentication rejection
stops reconnect attempts until settings restart the worker.

## Explicit sidebar commands and numeric inputs

Button/scene grants allow `unknown` (common before first use), but require an
observed nondeleted/non-unavailable target. Media grants offer fixed Play/Pause/
Stop routes, and require known state. Binary grants offer fixed Turn on/Turn off
routes only for exact observed on/off. Covers offer Open/Close/Stop only while
state is open/closed/opening/closing and the corresponding HA supported-feature
bit (1/2/8) is set. Cover position requires the set-position bit and observed
position. Number inputs require finite current state, bounds and positive step.
Inputs carry a fresh revision and validated bounds; stale, out-of-range or off-step
submissions are rejected before HTTP. Capability changes revoke old grants.

All services are fixed POST paths below `<base>/api/services/`, with exact
`entity_id` and, for numeric setters, the validated value/position. At most two
service requests run concurrently; overload rejects without a backlog. Deadlines
are 1–30,000 ms from receipt, response bodies are bounded to 1 MiB and never
forwarded to the UI. Redirects and automatic retries are disabled. Cancellation,
disconnect and teardown stop local requests. A submitted request may already
have reached HA: timeout/cancellation does not undo it or prove device state.
Only subsequent HA observations change displayed state.

## Notifications and verification

Opt-in notifications cover switch, input_boolean, light, fan and binary_sensor;
binary sensors notify only on exact `on`. First observations and unchanged states
are suppressed, with a 60-second per-entity cooldown. Notifications use bounded
friendly names (otherwise the object ID), a bounded pending queue and no HA
credentials. Disconnect clears pending notifications and baselines; a reconnect
snapshot does not announce changes. They follow selected/discovered read scope.

Tests use disposable local HTTP/WebSocket/TLS fixtures and actual worker pipes;
no production HA server or device operation is required:

```sh
cargo test --manifest-path desktop-plugins/home-assistant/Cargo.toml --locked --all-targets
cargo clippy --manifest-path desktop-plugins/home-assistant/Cargo.toml --locked --all-targets -- -D warnings
cargo fmt --manifest-path desktop-plugins/home-assistant/Cargo.toml --check
```

The suite covers exact routes/bodies, fresh-read failures, stale descriptors,
authentication and cancellation, bootstrap ordering, transport bounds, compact
identity/visibility, catalog pages, notification suppression and maximum frames.
This establishes local protocol behavior; it does not assert physical appliance
or live automation execution.

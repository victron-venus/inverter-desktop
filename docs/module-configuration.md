# Module configuration and portable settings

The shared application configuration can retain versioned module namespaces without
loading or enabling a module. Desktop and mobile use the same persistence format:

```json
{
  "modules": {
    "example.integration": {
      "schema_version": 1,
      "values": {
        "server": "https://service.example.invalid",
        "layout": [{ "kind": "compact", "visible": true }]
      },
      "secrets": {
        "access_token": "example-local-credential"
      }
    }
  }
}
```

`schema_version` is a positive 32-bit integer that versions the module's payload,
not the application or plugin package. Unknown module IDs, payload fields and future schema versions are retained
without interpretation. The built-in `victron.energy-tariff` namespace is
interpreted by the frontend tariff editor; its version 1 `values.plan` contains
a validated tariff version 2 object or null to inherit the controller tariff.
See [electricity tariffs](electricity-tariffs.md). `values` must be a JSON object and
retains nested unknown fields. `secrets` is an optional string map. Unknown fields
outside these three envelope fields are rejected because core cannot classify
them as safe for a portable export. Debug output contains only the version and
field counts, never payload values or credentials.

Module authors must put every credential and credential-bearing URL in `secrets`.
`values` is explicitly portable, non-secret data; core cannot infer credentials
from arbitrary future field names. Both maps remain inside the existing encrypted
local configuration. A namespace is not executable code and does not authorize
installation or activation.

## Desktop handover

Built-in workers use their exact package IDs as namespace keys, with
`schema_version: 1`. The native desktop migration planner recognizes only those
IDs and runs for explicitly installed or declared packages. It retains legacy
HA/camera fields as inputs, plans the complete selection/layout before writing,
and rejects unsupported or oversized mappings instead of silently dropping them.
Core/mobile load and save remain passive.

Configuration pins remain authoritative. An app-only upgrade keeps an older HA
package's existing settings intact when its schema cannot express the compact
layout; the handover marker stays unset until a compatible package is selected.
An explicit restore into an incompatible installed schema is rejected. Updating
the app never silently replaces the package pin or truncates the planned layout.

The encrypted `SettingsStore` record becomes authoritative after handover. A
module namespace can seed an absent record during restoration; it does not
continually overwrite edited worker settings. Before a desktop portable export,
the native host snapshots current installed-plugin public values into their
namespaces. Credentials stay in dedicated secret storage and are omitted from
that snapshot. A clean restore therefore restores selections and layout but
requires credentials before startup. An import changing an installed record's public settings is rejected before
configuration persistence. Snapshot validation and the synchronous restore commit
share one package operation lock, excluding concurrent settings changes or removal.
The merge normalizes installed namespace shadows to that authoritative public
snapshot; old shadow credentials cannot make a new backup conflict with itself.
Unknown noninstalled namespaces retain their local credential binding rules below.
Integrated acceptance remains tracked in [TODO](../TODO.md).

## Save, reset, and portable backups

- Core/mobile reads return unknown namespaces and payload versions with only
  public `values`; `get_config` removes module secrets after any native migration
  has been persisted. The frontend never needs these credentials for a core edit.
  Native saves merge locally retained namespaces that an older client omits.
  Omitted individual secret keys are also retained when the namespace's version
  and public values are unchanged. Changing that binding without explicitly
  providing every locally stored credential rejects the save before any write.
- Resetting core defaults retains module values and secrets. Explicit removal of
  module namespaces is not exposed by the core editor.
- Export keeps module IDs, versions, and `values`, and omits every `secrets` map.
  Existing core credential and authentication redaction remains unchanged.
- Import preserves local namespaces absent from the backup. Secret-free incoming
  namespaces may be added or replace their existing public settings.
- Imported module credentials are rejected, including legacy/manual files that
  contain a nonempty `secrets` map. Malformed envelopes are rejected without
  displaying their fields or contents.
- When a local namespace has secrets, incoming version and values must match
  exactly. Matching imports retain the local namespace and its credentials. A
  mismatch rejects the entire import before any settings are saved. Core cannot
  determine whether an unknown value changes a server or credential scope, so it
  must not pair retained credentials with new imported values.

Core never activates a namespace. Desktop package installation, revision-bound
settings changes, and authorized worker startup remain separate native operations.
Legacy `camera_live_urls` may contain credentials in URL queries: core IPC omits
the map, ordinary save/reset preserves it locally, and portable backups exclude
it. Its migration target is the camera plugin's dedicated secret mapping.

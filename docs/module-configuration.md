# Passive module configuration

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
not the application or plugin package. Core does not interpret a module ID,
payload fields, or a future schema version. `values` must be a JSON object and
retains nested unknown fields. `secrets` is an optional string map. Unknown fields
outside these three envelope fields are rejected because core cannot classify
them as safe for a portable export. Debug output contains only the version and
field counts, never payload values or credentials.

Module authors must put every credential and credential-bearing URL in `secrets`.
`values` is explicitly portable, non-secret data; core cannot infer credentials
from arbitrary future field names. Both maps remain inside the existing encrypted
local configuration. This namespace is not the installable plugin's settings
store and is not automatically copied into a worker.

## Save, reset, and portable backups

- Core/mobile reads return unknown namespaces and payload versions with only
  public `values`; `get_config` removes module secrets after any native migration
  has been persisted. The frontend never needs these credentials for a core edit.
  Native saves merge locally retained namespaces that an older client omits.
  Omitted individual secret keys are also retained when the namespace's version
  and public values are unchanged. Changing that binding without explicitly
  providing every locally stored credential rejects the save before any write.
- Resetting core defaults retains module values and secrets. Explicit removal of
  module namespaces is not exposed in this prerequisite.
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

This foundation does not migrate legacy HA/camera settings, install packages,
activate modules, transfer credentials to workers, or expose module-specific UI.
Those steps require module-owned migration and retention behavior separately.

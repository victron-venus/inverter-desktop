# Desktop plugin settings

Verified installed desktop packages have isolated settings. Installation can use
an exact archive pin in application configuration or a manually reviewed signed
package. Configured downloads need no publisher or application signing keys;
the empty publisher policy keeps manual signed-file selection unavailable.
Legacy Home Assistant/camera settings are not migrated automatically. Android
and iOS preserve package declarations as dormant configuration data but include
none of this editor, native commands, storage adapter, or worker protocol.

## Application declarations and restoration

The application configuration's `desktop_plugins` list records package IDs,
exact versions, enabled preferences, and per-platform HTTPS URLs and SHA-256
digests. It contains download metadata, not plugin connection settings or secrets.
See [declaring plugins](plugin-packages.md#declaring-plugins-in-application-configuration)
for the schema and automatic installation behavior.

The exact archive pin selects desired content, so an explicit declaration change
may install rebuilt bytes at the same worker version or intentionally select an
older version. The previous active package is retained for rollback. Existing
pins stay unchanged until edited or imported; application upgrades do not follow
newer package releases automatically.

An application upgrade or reinstall that preserves its data preserves both the
declarations and the encrypted plugin records. A portable application backup
includes declarations; restoring it on a clean installation lets desktop fetch
the packages again. It does not restore plugin settings or credentials from a
wiped installation. Missing required settings leave a newly installed package
stopped and visible in the manager. Saving valid settings retries activation when
the declaration requests `enabled: true`. A declaration with `enabled: false`
keeps the package stopped after settings changes.

Removing a declaration releases configuration management and retains the current
installed package and its settings. Uninstall remains an explicit separate action.
To reinstall a damaged but manageable package, remove the declaration, uninstall
with settings retained, then restore the declaration and retry. Corrupt store
inventory is reported and never automatically erased to force restoration.
Ordinary credential-free backup/import policy is unchanged: exported declarations
are portable, while passwords, tokens, and local authentication policy are not
exported. Unknown declaration metadata survives mobile save/export roundtrips;
desktop rejects unknown declaration fields before interpreting or installing them.

## Editor and lifecycle

**Configuration → Plugins → Settings** is available for an installed package
whose manifest declares `plugin_configuration`. Native code reverifies its
archive before reading or changing settings. Only the authenticated settings
window can call `get_plugin_settings` and `save_plugin_settings`.

The editor supports text, password, boolean, numeric, and enumerated fields.
Descriptions and titles are plain text. Secrets start blank; a presence indicator
shows whether one is stored. Leaving a secret unchanged preserves it; entering a
replacement updates it; explicit clearing removes it. Password drafts are cleared
on save, cancel, editor teardown, and session changes. No stored secret is sent
back to the webview, returned as a default, or included in runtime snapshots.

The native response revision binds the active archive SHA-256 and encrypted data
revision. Saving fails without overwriting current data if another save, update, or
rollback changed either bound revision. The editor must be reopened. Package
lifecycle actions and settings changes share one owned native operation lock;
a cancelled IPC caller does not abandon an in-progress authorized transaction.

Settings save commits before activation. An enabled worker is stopped and starts
again with the new values; a manually disabled worker stays stopped. A configured
package awaiting required settings is retried according to its declaration. If activation fails,
the saved values and enabled preference remain, and the UI reports the restart
failure separately. Authentication revocation prevents a stale operation from
committing or delivering configuration into a later session. A committed change
remains available to the next authenticated startup.

## Declarative schema subset

The manifest's `config_schema` is a flat JSON Schema subset. The native compiler
rejects unsupported keywords instead of silently ignoring constraints.

```json
{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "endpoint": {
      "type": "string",
      "title": "Endpoint",
      "description": "Server address",
      "minLength": 1,
      "maxLength": 2048
    },
    "token": { "type": "string", "title": "Token", "writeOnly": true, "minLength": 1 },
    "port": { "type": "integer", "minimum": 1, "maximum": 65535, "default": 443 },
    "enabled": { "type": "boolean", "default": true },
    "mode": { "type": "string", "enum": ["local", "remote"], "default": "local" }
  },
  "required": ["endpoint", "token"]
}
```

Root keywords are `type`, `properties`, `required`, `additionalProperties`, `title`,
and `description`. `type` must be `object`; `additionalProperties` may be absent
or false. At most 32 fields are accepted. Field keys use ASCII letters, digits,
underscores, periods, or hyphens, start with an alphanumeric character, occupy
1–64 bytes, and exclude JavaScript
prototype names. Titles occupy at most 128 bytes and descriptions at most 512.

Field types are `string`, `boolean`, `number`, and `integer`. Strings support
`minLength`, `maxLength`, and at most 32 unique string `enum` choices. Length bounds
count Unicode characters, while every string also has a hard 16,384-byte UTF-8
limit in host API 1.7 (4,096 bytes in earlier hosts). Packages needing the larger
limit must require a compatible host API. The complete transmitted configuration
and encrypted settings data retain their separate 32 KiB limits.
Numeric fields support finite `minimum` and `maximum`; integers must be
integral and within the JavaScript safe-integer range. Optional `default` values must satisfy their field constraints.

Host API 1.5 adds the opt-in field keyword `omitEmpty`. It may be true only for
an optional, public string with an explicit `default` of `""`. The settings editor
still shows an empty value, while saving represents it through the schema default
instead of a stored empty key. Empty values are also omitted from the worker's
startup configuration; nonempty values are stored and sent normally. Workers using this
keyword must treat the absent field as empty and require a compatible host API
such as `^1.5`. Absent or false preserves the previous default-delivery behavior.
Required fields, secrets, other types and missing or nonempty defaults cannot
opt in. Validation still applies before omission, and the complete transmitted
configuration and encrypted settings data retain their separate 32 KiB limits.
This permits new optional selections without enlarging existing configuration
or saved data when those selections are empty. Existing stored empty values
are normalized on the next explicit save; opening settings does not rewrite data.

`writeOnly: true` denotes a secret string. Secrets cannot have defaults or enum
choices. The `required` list checks presence; use `minLength: 1` to reject empty
strings. Opening settings permits incomplete setup. Save and worker startup
enforce required values. An optional field omitted from a save takes its default
if one exists; otherwise it is cleared. Omitted secret changes preserve the
existing secret. Secret changes accept a replacement string or explicit null.

Arrays, nested objects, references, remote schema loading, regular expressions,
conditional schemas, scripts, and arbitrary frontend code are unsupported. More
complex camera/entity configuration needs a later contract extension; this
checkpoint does not claim feature parity for those integrations.

Unknown fields already stored on disk survive schema changes but are withheld
from both the editor and worker. Fields once recorded as secret keep a permanent
classification marker even when cleared. A future schema cannot expose such a
field as public; a migration must use a new field name or explicit data deletion.

## Encrypted storage and recovery

Records live under the private, exclusively leased package store's `settings/`
directory, outside package payloads and staging. Filenames use SHA-256 of the full
validated plugin ID, avoiding platform-specific reserved filenames. Each record
has a versioned binary envelope containing a fresh random nonce and AES-256-GCM
ciphertext. Revision, ordinary values, secret values, and classification markers
are encrypted together. Authenticated associated data binds both a separate
plugin-settings format/domain and the exact plugin ID, rejecting substitution
between plugins or with core configuration.

The encryption key comes from the application's existing protected key provider.
This introduces no extra keychain entry or change to legacy credential migration.
Plugin records are separate from portable application exports. Merely constructing the store or
reading missing settings does not create files or request a key. A corrupt record
or unavailable key is an explicit failure; neither silently resets settings.

Plaintext is limited to 32 KiB per record, encrypted files to 64 KiB, retained
records to 64, and the settings directory to 8 MiB including transaction overhead.
The enclosing package-store quota also applies. Reads/writes reject symbolic or
hard links, reparse points, special files, oversized files, and unsafe directory
entries. Unix directories/files are private to the user.

Save prepares and syncs ciphertext in an exclusively created temporary file while
holding the package operation lock. Only the final commit runs within the original
authentication epoch. An abandoned prepared write removes its own pending file;
startup recovery cleans recognized pending transactions after obtaining the store
lease. The previous record survives validation, encryption, or preparation failure.

Uninstall retains settings unless deletion was explicitly selected. Requested
deletion happens after worker cleanup, within the authorized uninstall operation;
a deletion failure leaves the installed record available for retry. Deletion and
package inventory updates are separate filesystem changes: a later inventory
write failure can leave a stopped installed record with settings already cleared.
Core configuration and other plugin records are never part of this deletion.

## Retained data inventory and cleanup

The desktop Plugins panel provides an on-demand stored-data inventory. It displays
record sizes, total usage, and storage limits. Opening or refreshing this view
does not decrypt records, request a credential key, or create a missing settings
directory. High-frequency worker updates do not trigger filesystem scans.
The manager snapshot includes a data revision that invalidates cached views after
package/settings activity and session changes. It is an invalidation token, not
a count of successful writes; failed lifecycle operations may also advance it.
Worker contribution events keep the same revision and cause no data scan.

The authenticated settings window uses `get_retained_plugin_data` to read the
inventory and `delete_retained_plugin_data` to delete a confirmed record. The
native response contains opaque record IDs, ciphertext revisions, byte counts,
and an optional installed plugin ID. It contains no values, secrets, filesystem
paths, or claimed ciphertext health status. Pending transactions count toward
byte usage but are not exposed as deletable records.

Installed owners are identified from the native package inventory, including
disabled packages and packages whose payload no longer verifies. Their data
cannot be deleted through retained-data cleanup. Uninstall with explicit settings
deletion, or uninstall first and then remove the retained record. Cleanup neither
stops workers nor changes installed package state or core transports.

After uninstall, only a one-way hash of the plugin ID remains in the filename.
There is no name catalog, so the UI labels these records as unidentified stored
data and includes a shortened record identifier. It does not infer a former
plugin name. Reinstallation of the same plugin restores the owner association.

Deletion requires explicit confirmation and the ciphertext revision obtained
when the record was listed. Native code serializes the operation with package
installation, settings changes, and uninstall. It checks the current installed
owners again and rechecks the record bytes before unlinking. A changed or missing
record requires refreshing the view; an old confirmation cannot delete a newly
changed record or the data of a reinstalled package. Logout revokes queued
operations, including work whose IPC caller was cancelled.

If unlink succeeds but directory synchronization fails, the error explicitly says
the record was removed and durability could not be confirmed. The error stays
visible in the initiating window; refresh other open stored-data views explicitly
after this exceptional outcome. Repeated deletion still checks record existence
and revision rather than silently deleting a different record.

Corrupt or empty ciphertext can be removed without the encryption key, reclaiming
record and byte quota. This is limited to canonical, private, bounded regular
files. Symlinks, hard links, reparse points, nonprivate entries, arbitrary paths,
and oversized records remain errors. The API does not automatically repair or
delete unexpected filesystem entries. Android and iOS include neither this UI
nor its inventory and deletion commands.

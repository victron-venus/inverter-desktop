# Desktop plugins and the mobile core

Android and iOS ship the inverter core only. Desktop adds a generic package host;
Home Assistant, Frigate, Kerberos, and Ring are separately built and installed
workers. The core retains Victron/Cerbo telemetry, MQTT/IGW selection, inverter
MQTT flags, battery/solar/grid statistics, grid submeter and daemon setpoint
override, EV and water/pump controls, authentication, core notifications,
configuration, and app updates.

A desktop installation without optional packages starts no HA/camera connection
and shows no feature settings or contribution panels. Explicit `desktop_plugins`
declarations restore exact HTTPS archive/SHA-256 pins after authentication. The
Plugins tab manages installed packages and their encrypted settings. The manual
signed-file flow requires approved publishers and is unavailable with the empty
embedded policy. Old HA/camera configuration alone never authorizes installation.

## Frontend composition

`App.vue`, `Config.vue`, and core components remain shared. Build aliases resolve
`@features` to `src/features/desktop.ts` or `src/features/mobile.ts`, and translations
and data-only defaults use corresponding platform entries. Configuration does
not import a UI feature entry point. Mobile supplies empty feature slots without
importing plugin code, translations, metadata, or loaders.

Desktop imports generic components under `src/features/desktop/plugins/`.
`presentation.ts` projects host-validated descriptors into the existing header,
Home controls, and sidebar. Explicit control positions preserve mixed core/plugin
ordering, labels, unavailable states, and existing core flags. Compact groups,
appliance summaries, and weather use host-owned views; no generic flat entity
panel is mounted. Missing packages contribute nothing. Stale snapshots disable
actions and connection indicators rather than claiming current authority.

Every action resolves an exact contribution reference and goes through the
existing instance-bound native action route. Presentation never supplies a Tauri
command, arbitrary service, component, or script. Numeric gestures capture the
advertised revision and constraints; changed authority rejects the submission.
Core controls retain their direct MQTT/IGW path.

`PluginSettingsEditor.vue` renders manifest scalar fields and bounded structured
JSON-string fields as normal forms. Worker-published choice catalogs provide
optional entity suggestions; they grant no action authority. Secrets are never
read back. `PluginGroupActions.vue` uses native group enablement, so a stopped
camera worker can be re-enabled without dispatching an action to that process.
Native configuration-change events update only `desktop_plugins` in an open
settings draft, preserving dirty core edits.
The Plugins tab also persists individual enable/disable and removes a configured
package's declaration on confirmed uninstall. Ordinary core saves preserve the
latest declarations under the native configuration lock, so an old form or theme
save cannot reinstall a removed plugin. Explicit declaration edits require a
matching `desktopPluginsExpected` baseline; package authority remains native.

`PluginMedia.vue` accepts only an opaque native-owned media route. Snapshots close
after twelve seconds; video, drag, close, and lifecycle revocation remain native
owned. The former bundled `useHA`, entity discovery/settings, camera connection,
provider panels, and arbitrary local-file camera player have been removed.

## Native and persistence boundaries

The desktop-only `src-tauri/src/plugins/` host validates packages, settings,
presentation, choices, notifications, media, and group operations. HA REST/WS and
camera event parsing live only in their separate `desktop-plugins/` workspaces.
The bundled `ha_api.rs`, `ha_session.rs`, `camera.rs`, and
`mqtt/camera_events.rs` are removed. Core MQTT contains no camera subscription
lifecycle or HA command fallback. Unsupported non-core `perform_action` targets
are rejected on every platform; the seven daemon flags and historical
`input_boolean.<flag>` aliases retain their normalization.

All legacy configuration fields remain passive migration inputs. Native migration
can seed only an explicitly installed/declared matching built-in package. Exact
plugin IDs identify schema-version-1 module namespaces; encrypted plugin records
are authoritative after handover. Core reads omit module credentials and private
camera live URLs, while saves/resets preserve them locally. Portable exports
include public plugin values and declarations, never plugin secrets. See
[module configuration](module-configuration.md) and [plugin settings](plugin-settings.md).

Android/iOS compile no host, package store, publisher policy, verifier, worker,
choice/group IPC, or plugin media service. Worker crates have independent desktop
workspaces and reject mobile builds. Target-scoped dependencies, command handler
selection, and frontend aliases enforce this at build time. Mobile media CSP and
asset scope do not expose desktop media. Desktop workers cannot access the core
MQTT client through the plugin protocol.

## Build and verification

```bash
pnpm build
pnpm build:mobile
pnpm test:build-profiles
pnpm test:mobile
pnpm test
```

`scripts/frontend-profile.mjs` resolves native target hints rather than the build
machine's OS. Conflicting explicit profiles fail. Vite checks all loaded modules,
even imports removed later by tree shaking, and emits `dist/build-profile.json`.
Both profiles reject bundled provider entry points and worker/package source.
Mobile additionally rejects all generic desktop host modules. Native mobile
release builds require a valid mobile graph receipt, so stale desktop assets
cannot be embedded by running Cargo directly.

The native boundary verifier inspects final executables, rustc source dependency
files, resolved normal Cargo dependencies, and archive payload names:

```bash
python3 scripts/check-mobile-native-boundary.py --platform ios --artifact app.ipa
python3 scripts/check-mobile-native-boundary.py --platform android --artifact app.aab app.apk
```

It rejects every plugin manager/settings/choice/group/media command, host protocol
markers, all four worker identities, their binaries/manifests, and any compiled
host or provider source. Independent negative fixtures contaminate APK/AAB/IPA
payloads and real Vite graphs. Hosted mobile/release/Play jobs apply the same gates
to produced artifacts. For a locally built mobile library, use `--native-library`
and a matching `--target`, optionally `--target-dir` and `--profile`.

Graph checks, local fixtures, hosted artifacts, and installed device behavior are
separate evidence. They do not operate the user's household, provision publisher
trust, or prove native graphical behavior on every platform. Current results and
remaining delivery checks belong in [TODO](../TODO.md).

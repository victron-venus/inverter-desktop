# Desktop features and the mobile core

Android and iOS ship the inverter core only. Home Assistant integration, cameras,
and future package installation are exclusive to desktop. The core retains
Victron/Cerbo telemetry, MQTT/IGW selection, MQTT inverter-control flags, battery/
solar/grid statistics, EV and water/pump controls, authentication, notifications,
configuration, and app updates.

This implementation checkpoint separates feature source code and enforces mobile
exclusion. Desktop still bundles its existing HA/camera implementations. It does
not yet offer independently installable packages. The worker protocol, installer,
package migration, and delivery acceptance criteria remain open in [TODO](../TODO.md).

## Frontend composition

`App.vue`, `Config.vue`, and the core components remain shared. The build resolves
`@features` to `src/features/desktop.ts` or `src/features/mobile.ts`, and
`@feature-messages` to the corresponding translation composition. The desktop port
supplies feature panels, settings, setup, status, camera routes, and lifecycle.
The fixed mobile composition supplies empty contributions without importing those
implementations; it contains no plugin manager or package loader.

`@feature-defaults` resolves directly to the platform's data-only defaults module.
Core configuration must not import the UI contribution entry point: that would
create an initialization cycle through the settings components and lose defaults.

HA/camera presentation and connection code lives under `src/features/desktop/`.
The legacy `useHA`, camera-view, and entity-discovery entry points are reachable
only through the desktop composition. Core MQTT/IGW source selection and inverter
actions remain in core composables. Camera MQTT has its own connection lifecycle.

Core settings expose Cerbo device selectors separately from desktop integration
settings. On mobile, stored HA controls are unavailable in the UI but remain in
saved configuration. Editing core controls preserves hidden definitions and their
order/metadata, so returning to desktop does not destroy the user's configuration.
Legacy field names are passive compatibility data, not a mobile integration.

## Native composition

Rust compiles HA sessions, REST/WS clients, camera adapters/downloads/windows,
managed feature state, command registrations, and authorization entries only
under `cfg(desktop)`. Camera MQTT parsing is separated into
`src-tauri/src/mqtt/camera_events.rs`. WebSocket dependencies are target-scoped.
Shared HTTP, MQTT, authentication, and notification dependencies remain in core.

Mobile registers the core commands and rejects non-core control targets. The
seven daemon flag keys and legacy `input_boolean.<flag>` aliases retain the MQTT
normalization path. No mobile feature startup task or placeholder HA/camera command
is registered. Camera windows have desktop-only capabilities; mobile configuration
has no camera asset scope.

## Build and verification commands

```bash
pnpm build                    # Default desktop, or the target selected by Tauri
pnpm build:mobile             # Explicit standalone mobile frontend
pnpm test:build-profiles      # Target selection + actual bundler negative test
pnpm test:mobile              # Shared UI/actions/persistence with mobile aliases
pnpm test                     # Desktop/core regression suite
```

`scripts/frontend-profile.mjs` follows Tauri's platform/target hints, including
`androideabi`, rather than the build machine's OS. `INVERTER_BUILD_PROFILE` accepts
`desktop` or `mobile`; conflicting native target hints are rejected. The iOS
direct-cargo helper explicitly builds the mobile frontend. Mobile CI, release, and
Play jobs also select the mobile profile.

Vite examines its actual source module graph and emits `dist/build-profile.json`.
The mobile build fails if a desktop implementation enters that graph, including a
static import hidden behind an unused runtime branch. A native mobile release
also requires a valid mobile graph receipt, preventing a raw Cargo build from
embedding stale desktop assets.

The native boundary verifier examines final native payloads, rustc dependency
files, and the resolved Cargo normal-dependency tree. Mobile/release/Play workflows
run it on every produced app artifact before upload:

```bash
python3 scripts/check-mobile-native-boundary.py --platform ios --artifact app.ipa
python3 scripts/check-mobile-native-boundary.py --platform android --artifact app.aab app.apk
```

For a locally built mobile library, use `--native-library PATH --target TRIPLE`
with the appropriate `--target-dir` and `--profile` if they differ from Cargo's
defaults. The verifier never executes or installs the inspected artifact.

Frontend graph checks, native compilation/payload checks, and device behavior are
separate evidence. Local tests and CI do not verify the user's HA installation,
issue physical inverter commands, or prove usability on every mobile device.

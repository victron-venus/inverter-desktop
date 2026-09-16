# Ring desktop worker

`inverter-desktop.ring` version 0.1.0 requires desktop host API 1.8. It is independently installable and does not require Frigate or Home Assistant. Android and iOS builds reject this package at build time.

Default subscriptions are `ring/+/camera/+/motion/state;ring/+/camera/+/ding/state`. `ON`, `TRUE` and `1`, ignoring case and surrounding whitespace, admit an event. Other payloads and topic shapes are ignored. Retained ON messages retain the bundled Ring behavior. Broker host/port/TLS and write-only username/password are plugin-owned.

Each location/device/event has a fixed 20-second cooldown; suppressed repeats do not extend it. Motion and ding remain independent. Including location fixes the old bundled deduplication collision between equal device IDs in separate locations. Broker reconnects retain process-local history. Explicit `camera_labels` map `location/device` to a display name, replacing the old ambiguous HA entity-name heuristic without any HA dependency.

Without a snapshot template, admitted events notify only. To also open a snapshot, set public `snapshot_base_url`, private `snapshot_url_template`, and `snapshot_media_kind` (`jpeg`, `png`, `webp`, or `video`; default `jpeg`). Templates support `{location_id}`, `{device_id}`, and `{event}`. Identity substitutions are URL-component encoded; ambiguous traversal IDs and unknown placeholders are rejected. The resulting URL must stay under the configured HTTP(S) origin and path prefix. Query credentials in the private template are supported; fragments and URL userinfo are rejected.

An optional private `snapshot_bearer_token` is consumed by the host only for that explicit origin/path. The worker does not download media and never emits the bearer token. No core HA token is inherited. The host disables redirects, enforces typed media validation and applies the declared 20-second media cooldown. Snapshot URLs travel only through the private native worker channel, never public dashboard state.

Configuration limits are 16 provider-specific filters, a 4,096-byte filter list, 32 explicit camera names, 128-byte location/device identities, 96-byte names, a 2,048-byte base/template and a 4,096-byte bearer token. The entire configuration must fit in 32 KiB. MQTT payloads are capped at 16 KiB, recent identities at 512, and outgoing notifications and media requests each at 30 per minute. Bounded output drops excess events instead of blocking broker polling or shutdown.

Run `cargo test --locked --manifest-path desktop-plugins/ring/Cargo.toml` and `cargo clippy --locked --manifest-path desktop-plugins/ring/Cargo.toml --all-targets -- -D warnings`. Fixture tests cover exact topics, credentials, reconnect, retained events, snapshot scope and cooldown boundaries without accessing household devices.

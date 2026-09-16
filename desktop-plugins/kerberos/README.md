# Kerberos desktop worker

`inverter-desktop.kerberos` version 0.1.0 requires desktop host API 1.8. It is independently installable and does not require Frigate or Home Assistant. Android and iOS builds reject this package at build time.

The worker subscribes to native Agent motion topics. The default filters are `kerberos/agent/+;kerberos/hub/+`; custom filters may replace the final identity with an exact ID. The broker host, port, TLS setting and write-only username/password belong to this plugin. No core MQTT or HA credentials are inherited.

Standalone Agent messages must contain `motion`. Hub messages must contain an unencrypted, non-hidden `payload.action=motion`, an exact device identity, and a positive timestamp within 60 seconds behind or 10 seconds ahead of the current clock. Conflicting outer device IDs, retained messages, oversized payloads and legacy clip URLs are ignored.

Agent and Hub messages share a 15-second silence boundary per camera. Repeated movement extends the same episode. A fresh episode receives a distinct notification ID; MQTT reconnects keep the current process's episode history.

`camera_labels` is an optional JSON object mapping exact camera identities to names. `camera_live_urls` is a private JSON object mapping those identities to HTTP(S) pages. The editor replaces the complete private mapping without revealing saved URLs. Queries and fragments are allowed; embedded URL credentials are rejected. Both maps hold at most 32 cameras, with identities up to 128 UTF-8 bytes. Labels are limited to 96 bytes, live URLs to 2,048 bytes, the public map to 8,192 bytes, and the private map to 16,384 bytes. The complete configuration must fit in 32 KiB.

Motion produces a native notification, never an automatic live window. When a mapped notification is clicked, the host resolves its configured destination and owns the isolated live window. MQTT supplies identity only. The worker never emits the private URL. The host revokes click authority when the plugin stops, settings change, or authentication expires. Native clickable notification support depends on the host platform; macOS implements the existing Kerberos click behavior.

Transport limits are 16 provider-specific filters, a 4,096-byte filter list, 16 KiB incoming payloads, 512 recent camera identities and 30 outgoing notifications per minute. Unsupported global wildcard filters are rejected before any connection. Slow host output cannot delay shutdown or block MQTT polling. Network errors expose only generic connection status.

Run `cargo test --locked --manifest-path desktop-plugins/kerberos/Cargo.toml` and `cargo clippy --locked --manifest-path desktop-plugins/kerberos/Cargo.toml --all-targets -- -D warnings`. Unit tests cover silence/timestamp boundaries and bounded parsing; subprocess tests use local fixture brokers and do not contact real cameras.

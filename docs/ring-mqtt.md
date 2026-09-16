# Ring-MQTT camera motion (inverter-desktop)

## Discovered IDs (live ring-mqtt / HA)

Do **not** invent these — confirm from ring-mqtt logs or MQTT:

| Item                        | Value                                                             |
| --------------------------- | ----------------------------------------------------------------- |
| location_id                 | `18d65208-6816-4bbe-bf09-310b7a201feb`                            |
| Front Door camera device_id | `54e019cac69d`                                                    |
| Motion topic                | `ring/<location_id>/camera/<device_id>/motion/state` → `ON`/`OFF` |
| Ding topic                  | `ring/<location_id>/camera/<device_id>/ding/state` → `ON`/`OFF`   |
| Live RTSP (in-cluster)      | `rtsp://ring:<pass>@<pod-ip>:8554/<device_id>_live`               |
| Event RTSP (in-cluster)     | `rtsp://…:8554/<device_id>_event`                                 |

Chimes (Hallway) are not cameras: `c4f312386268`, `9c7613dca55f`.

## Optional package behavior

Ring runs in the separate [Ring worker](../desktop-plugins/ring/README.md), not in
the core MQTT client. Install/declare that package explicitly and configure its
MQTT endpoint, motion/ding filters, labels, and optional snapshot settings through
**Configuration → Plugins → Settings**. The generic Cameras toolbar starts/stops
installed camera packages without changing core telemetry.

An `ON` event produces a bounded notification. A configured HTTP(S) snapshot is
requested through the native media service and shown in an owned image window.
Optional proxy bearer authentication is a dedicated plugin secret scoped to the
configured base origin/path. There is no implicit lookup of the core HA token.
Native migration can seed legacy values only for an explicitly selected matching
package; old `camera_topic` and `ring_snapshot_url_template` fields remain passive
compatibility data and no longer start a bundled camera connection.

Kerberos and Frigate use their own packages and parsers. Their topic filters and
settings are isolated from Ring. Snapshot/video authority is revoked on worker
restart, disable, logout, or uninstall. Portable backups omit secrets, including
credential-bearing live destinations.

## Transport boundary

The owned media service downloads HTTP(S) snapshots or completed video. RTSP is
not supported by the webview player. A private cluster RTSP address therefore is
not a desktop playback URL; expose a suitable authenticated HTTP(S) endpoint if
needed and configure it explicitly in the package. Native live-view notification
actions are a separate scoped destination feature and currently have a macOS
backend.

The device IDs above are historical environment notes, not discovery defaults or
proof of current network reachability. Confirm current IDs and endpoints before
configuring a production package. Local fixtures and source extraction do not
verify that live Ring, HA proxy, or cluster endpoints are reachable.

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

## App behavior

1. Subscribe via existing HA MQTT `camera_topic` (semicolon list), e.g.

   `kerberos/desktop/events;frigate/events;ring/+/camera/+/motion/state;ring/+/camera/+/ding/state`

2. On Ring `ON`: OS notification; if `ring_snapshot_url_template` is set, open the camera window with that HTTP(S) URL (snapshot).

3. Kerberos JSON and Frigate `frigate/events` parsers are unchanged.

## RTSP from the Mac

`open_camera_video_window` downloads **HTTP/HTTPS** media and plays local mp4 **or** still images (jpeg/png). **RTSP is not supported** in the Webview `<video>` path.

As of 2026-09-11 from the Mac LAN:

- `ring-mqtt` Service is **ClusterIP** `10.43.0.193:8554` — not reachable
- Pod IP `10.42.2.114:8554` (node `h5` / `10.0.0.17`) — not reachable on `:8554`
- HA `http://ha:8123/api/camera_proxy/camera.front_door_snapshot` **is** reachable (use with HA long-lived token)

### Recommended LAN expose (for ha/k3s)

Minimal options (pick one), mirroring how the Mosquitto broker is published at VIP `10.10.10.10:1883`:

1. **LoadBalancer / externalIPs** on Service `ring-mqtt` port `8554` (e.g. VIP `10.10.10.11:8554`)
2. **NodePort** on a stable node (less ideal than VIP)
3. **Ingress TCP** (Traefik TCPEntryPoint) if you already terminate non-HTTP that way
4. Keep ClusterIP and point desktop at **HA camera_proxy / HLS** instead of RTSP

After RTSP is on the LAN, still prefer an **HTTP** snapshot or HLS URL for the desktop app unless/until an ffmpeg-based player is added.

### Suggested desktop config

```text
camera_topic: kerberos/desktop/events;frigate/events;ring/+/camera/+/motion/state;ring/+/camera/+/ding/state
ring_snapshot_url_template: http://ha:8123/api/camera_proxy/camera.front_door_snapshot
```

HA long-lived token (already in app config) is attached automatically when the snapshot URL host matches `ha_url`.

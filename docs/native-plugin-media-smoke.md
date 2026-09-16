# Native plugin media smoke test

`plugin-media-smoke` is an explicit desktop acceptance executable. It exercises
the production media service, opaque media route, Vue player, and native window
adapter with a private fixture grant. It is absent from normal builds and is
never selected by the production application, even when its feature is enabled.
Android and iOS cannot build this executable.

The harness creates temporary application state and uses a nonpersistent browser
profile. It does not initialize normal authentication, saved configuration,
keychain access, core MQTT, camera workers, or installed plugins. It initializes
an empty temporary package manager for the real bridge lifecycle. Its publisher
policy is empty and its settings key provider cannot supply a key. No production
publisher key, package installation, or new application-signing step is required.

## Run explicitly

Use a desktop graphical session and an actual decodable MP4, larger than 1 MiB,
lasting 8–40 seconds. Its duration must be at least the requested hold time plus
six seconds. The hold time is optional and defaults to zero; it accepts whole
seconds from zero through 30. Keep the desktop available while the harness
checks focus and native windows. It must first focus its own anchor window. If
the desktop prevents that, the run reports an explicit focus-precondition failure.
Rerun with a new evidence file and bring the Native Media Smoke anchor to the
foreground during initialization, then let the automated checks finish.

Serve only a disposable fixture directory on a loopback IP. For example, in a
separate terminal, with the clip at `/absolute/fixture-directory/prefix/clip.mp4`:

```bash
python3 -m http.server 8765 --bind 127.0.0.1 --directory /absolute/fixture-directory
```

The fixture URL must use plain HTTP and an explicit loopback IP. A `localhost`
hostname, credentials, query string, or fragment is rejected. The evidence path
must be an absolute path to a new file; its parent directory must already exist.

Build the desktop frontend from the repository root before running the example:

```bash
pnpm build
CARGO_BUILD_JOBS=2 cargo run --locked --manifest-path src-tauri/Cargo.toml \
  --example plugin-media-smoke --features native-media-smoke -- \
  --fixture-url http://127.0.0.1:8765/prefix/clip.mp4 \
  --evidence /absolute/new-evidence.json
```

To read the command's own usage without launching a window:

```bash
cargo run --locked --manifest-path src-tauri/Cargo.toml \
  --example plugin-media-smoke --features native-media-smoke -- --help
```

The explicit feature embeds the desktop assets, so rebuild them after player
changes. A stale desktop build is not evidence for changed frontend source.
Stop the fixture server after the test.

## What a pass establishes

The harness opens three players in sequence, including two simultaneously. It
requires decoded nonzero video dimensions, metadata, and playback time advancing
to at least 1.2 seconds. It checks native geometry, nonoverlapping windows within
the monitor work area, and preservation of the focused anchor window. It also exercises
bounded 1 MiB range responses, HEAD, and rejection of a different requesting
window.

The first player is closed through the production Vue Close button and its owned
IPC command. The second is destroyed after revoking its fixture lease. The last
must reach the actual video `ended` event and close automatically. Owned media
files must be removed and the bridge must exit successfully before the JSON can
report `passed: true`. Visible windows or a successful decode alone are not a
complete pass; retain the JSON and process result with the tested commit and
platform.

The signed-package, real Frigate worker, and Mosquitto tests are separate backend
acceptance. This executable does not prove package trust, MQTT behavior, OS
notification display, or playback on other operating systems. CI compiles the
opt-in example and runs `--help`; those checks do not launch its graphical test.
Current results and remaining acceptance work are recorded in [TODO.md](../TODO.md).

## Live preview mode

The same isolated harness supports the optional automatic preview path:

```text
plugin-media-smoke --live-fixture-url 'http://127.0.0.1:PORT/prefix/api/front?fps=2&height=360' --evidence /absolute/new-live-evidence.json
```

Serve a loopback MJPEG response with alternating visibly different frames. The
harness derives an explicit preview grant with a fifteen-second lifetime and no
bearer credential. It inspects decoded image dimensions and changing pixels by
native evaluation, checks window placement/focus, then verifies both timed expiry
and generation revocation. The external preview receives no IPC or frontend
observer script. Its URL must stay exact; redirects and new windows are denied.
Preview tests are separate from downloaded H.264/image tests and use no media
cache file. Run this mode explicitly to establish current graphical acceptance;
merely building it or passing the pure media tests is not playback evidence.

## Recorded macOS acceptance

The extraction checkpoint passed both graphical modes with the same isolated
executable (SHA-256
`55ea779afc86bd961f8ad56022accafc122c082b9a326981c6d209c4368f84d6`). Local evidence
files are `inverter-extraction-preview-w0ljgfb3-ready.json` and
`inverter-extraction-h264-w0ljgfb3-ready.json`; both report `passed: true`, no
failure, native exit code 0, and successful private-profile removal.

The MJPEG fixture produced distinct decoded 640x360 frames in visible, unfocused
windows without taking the anchor's focus. The first window expired after
15.055 seconds; generation revocation destroyed the second. The harness waits
through Wry's pre-navigation callback cancellation within its existing five-second
decode deadline; it still requires decoded dimensions and changing pixel values.

The H.264 fixture was 7,459,136 bytes, 640x360 pixels and 16 seconds long. All three
players decoded and advanced; the final player reached its actual `ended` event.
Two simultaneous 660x372 physical-pixel windows remained inside the monitor work
area, visible, unfocused and nonoverlapping. The focused anchor, bounded 1 MiB
`206` response, wrong-window rejection, real Vue Close button, revocation,
automatic end close and owned-file cleanup all passed. The disposable loopback
fixture server was stopped afterward.

This is source-checkpoint graphical evidence on macOS, separate from a signed
release installation or real camera/OS notification acceptance. Linux and Windows
graphical acceptance remains open; their build/help checks do not prove playback.

### Earlier clip checkpoint

The local `inverter-frigate-clips-native-smoke-3.json` reports `passed: true`, no
failure and native exit code 0. The actual H.264 fixture was 4,727,159 bytes,
640x360 pixels and 24 seconds long. All three players reported decoded dimensions
and playback progress; the final one reached its actual `ended` event at 24 seconds.

Two simultaneous native windows were 330x186, at y=30 and y=216, both visible,
unfocused and nonoverlapping. The anchor retained focus. The backend returned a
bounded 1 MiB `206` response and denied a different requesting window. The real
Vue Close control, lease revocation before native destruction, and automatic
video-end close all succeeded. Owned media files and the temporary profile were
removed before successful exit.

This acceptance exposed and verified fixes for two production defects: shared
camera placement now uses the monitor work area instead of the full display, and
macOS plugin windows suppress key-window eligibility only while being shown, then
restore normal user interaction. Linux and Windows graphical playback/window
acceptance remains pending. Their CI compile/help checks are not native playback
evidence, and this fixture does not establish OS notification display.

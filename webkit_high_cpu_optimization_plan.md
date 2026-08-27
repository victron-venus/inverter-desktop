# Implementation Plan: WebKit High CPU Optimization on macOS M1

## Problem Description

On macOS Apple Silicon (M1), `WebKit.WebContent` (the Tauri webview process) consumes up to **80% CPU**. This occurs even when the app is idling or minimized to the menu bar tray.

### Root Cause Analysis

1. **High-Frequency MQTT IPC Flood (Rust → WebKit)**:
   - Victron Cerbo GX broadcasts individual D-Bus topics (`N/<portal>/battery/...`, `N/<portal>/solarcharger/...`, `N/<portal>/tank/...`) continuously at rates up to 10–50 messages per second.
   - In `src-tauri/src/mqtt.rs` (lines 889, 909, 930), every single topic update immediately triggers `handle.emit("mqtt-state-update", &*guard)` without batching, rate limiting, or debouncing.
   - Every IPC event forces WebKit to deserialize JSON, dispatch JS events, and trigger Vue 3 reactivity diffing.
2. **Missing `WINDOW_HIDDEN` Guards on Device Messages**:
   - In `mqtt.rs` (lines 889, 909, 930), `WINDOW_HIDDEN` is **not checked**. When the window is hidden/minimized to the tray, WebKit continues processing dozens of state updates per second instead of going to sleep.
3. **ECharts Continuous Heavy Canvas Re-rendering**:
   - `src/composables/useChart.ts`: On every state update, up to 7,200 data points (4 series × 1,800 history entries) are recalculated and re-rendered with `smooth: true` (cubic spline) and `areaStyle` (filled polygons) on Canvas.
   - ECharts does not pause rendering when the window is hidden or when the data is unchanged.
4. **Redundant Frontend JavaScript Polling**:
   - `src/composables/useHA.ts`: `setInterval` timers (10s and 30s) poll HA over HTTP IPC even though Rust already maintains an active WebSocket connection.
5. **Vue Reactivity Churn**:
   - Expensive computeds with regex and string sorting (e.g. `haLoads`) re-execute on every state tick.

---

## Proposed Solution & Architecture

```mermaid
flowchart TD
    subgraph Rust_Backend["Rust Backend (src-tauri)"]
        Cerbo[Cerbo GX MQTT Stream] --> MQTT_Client[MqttClient Loop]
        MQTT_Client --> StateUpdate[Update InverterState Mutex]
        StateUpdate --> IsHidden{Window Hidden?}
        IsHidden -- Yes --> TrayOnly[Update Tray Icon Timer (1.5s)]
        IsHidden -- No --> Debouncer[IPC Throttle / Coalescer (500ms-1000ms)]
        Debouncer --> IPC_Emit[emit('mqtt-state-update')]
    end

    subgraph WebKit_Frontend["WebKit Webview (Vue 3)"]
        IPC_Emit --> VueState[useConnection: state.value]
        VueState --> Guard{Window Visible?}
        Guard -- Yes --> UI_Render[Vue Components Update]
        Guard -- Yes --> ChartThrottle[Chart Update (2s / LTTB Downsampled)]
        Guard -- No --> Sleep[Sleep: 0% CPU]
    end
```

---

## User Review Required

> [!IMPORTANT]
>
> - **Chart Resolution**: We propose using ECharts LTTB (Largest-Triangle-Three-Buckets) sampling and updating the chart every 2000ms instead of 1000ms. This looks identical visually to human eyes and preserves all peaks/dips, while cutting canvas path computation by over 80%.
> - **Background WebKit Suspension**: When the window is closed or hidden to the tray, IPC emissions to WebKit will be paused completely. The Rust backend will continue updating the system tray icon every 1.5s, monitoring battery/grid alarms, and keeping MQTT/HA connections alive. When the window is restored, a single `get_state` call instantly syncs the UI.

---

## Proposed Changes

### 1. Rust Backend (`src-tauri/src/mqtt.rs` & `src-tauri/src/lib.rs`)

#### [MODIFY] `src-tauri/src/mqtt.rs`

- Add IPC throttling / coalescing mechanism: batch incoming device topic updates and emit `mqtt-state-update` at a controlled cadence (e.g. every 500ms–1000ms) rather than on every single incoming MQTT packet.
- Add `WINDOW_HIDDEN` checks to all emit locations (lines 889, 909, 930) so hidden windows receive 0 IPC messages.
- Ensure critical alerts/alarms bypass throttling and trigger immediately.

#### [MODIFY] `src-tauri/src/lib.rs`

- When `window-shown` occurs, emit the latest state snapshot immediately so the UI is 100% current on wake.

---

### 2. Frontend Composables (`src/composables/`)

#### [MODIFY] `src/composables/useConnection.ts`

- Track `windowHidden` status in `useConnection`.
- Skip `processState()` processing when window is hidden.

#### [MODIFY] `src/composables/useChart.ts`

- Add downsampling (`sampling: 'lttb'`) to ECharts series definitions.
- Change `CHART_UPDATE_INTERVAL_MS` to 2000ms.
- Add `pauseChart()` and `resumeChart()` methods that disconnect/reconnect render hooks when `window-hidden` / `window-shown` fire.
- Optimize line smoothing (`smooth: 0.15` or `smooth: false` for faster rasterization).

#### [MODIFY] `src/composables/useHA.ts`

- Remove redundant JS `connInterval` (10s) and `appliancePoll` (30s) intervals since Rust handles WebSocket push notifications.
- Optimize `haLoads` computation by avoiding regex on unchanged keys.

---

### 3. Frontend App & Components (`src/App.vue` & `src/components/ChartPanel.vue`)

#### [MODIFY] `src/App.vue`

- On `window-hidden`: notify `useChart` to pause, set global visibility flag.
- On `window-shown`: resume `useChart` and request state refresh.

#### [MODIFY] `src/components/ChartPanel.vue`

- Disable `autoresize` observer when hidden or optimize resize listener.

---

## Verification Plan

### Automated Tests

- Run `pnpm test` (vitest unit tests).
- Run `cargo check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- Run `pnpm run format:check`.

### Manual Verification

1. **Activity Monitor / Instruments Inspection**:
   - Launch app: `pnpm tauri dev` or production build `./build-local.sh`.
   - Open macOS **Activity Monitor** -> Filter `inverter_dashboard` and `Inverter Desktop Helper (Renderer)` / `WebKit.WebContent`.
   - **Active Window**: WebKit CPU should drop from ~30–50% to **< 3–5%**.
   - **Minimized / Hidden to Tray**: WebKit CPU should drop to **0.0% – 0.1%** (zero background wakeups).
2. **Functionality Verification**:
   - Verify real-time updates of battery, solar, grid, consumption, setpoint.
   - Verify ECharts history chart continues recording and shows accurate data.
   - Verify Home Assistant toggles and water valve/pump buttons remain fully responsive.
   - Verify Tray icon updates every 1.5s with accurate numbers and bar charts.
   - Verify restoring window from tray immediately displays the latest state with zero lag.

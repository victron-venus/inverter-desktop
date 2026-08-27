# WebKit CPU Optimization Tasks (macOS M1)

Tracking document for eliminating high CPU usage in `WebKit.WebContent` (dropping from ~80% to <3% active, ~0% idle/tray) without losing any functionality.

---

## 1. Rust Backend IPC Throttling & Visibility Gating (`src-tauri/`)

- [x] **1.1. Rate-limit / coalesce MQTT IPC emits in `src-tauri/src/mqtt.rs`**
  - [x] Implement debounced / throttled state emission (500ms min interval for incoming Victron Cerbo GX device updates `N/...`).
  - [x] Ensure critical notifications/alarms bypass throttling and emit immediately.
- [x] **1.2. Enforce `WINDOW_HIDDEN` check across all MQTT emit locations in `src-tauri/src/mqtt.rs`**
  - [x] Add `!WINDOW_HIDDEN.load(Ordering::Relaxed)` guard via `emit_state_update` to console, Cerbo devices, and water topics.
  - [x] Ensure Rust internal state (`InverterState` and `cerbo_devices`) continues updating so tray icon (1.5s timer) and background alerts remain 100% operational when hidden.
- [x] **1.3. Instant state synchronization on window show in `src-tauri/src/lib.rs`**
  - [x] Ensure that when `set_window_hidden(false)` is invoked upon showing the window, the latest `InverterState` snapshot is immediately emitted to the frontend with `force: true`.

---

## 2. ECharts & Canvas Rendering Optimization (`src/composables/` & `src/components/`)

- [x] **2.1. Add downsampling and rendering optimizations in `src/composables/useChart.ts`**
  - [x] Enable ECharts `sampling: 'lttb'` (Largest-Triangle-Three-Buckets) on all line series to prevent drawing thousands of redundant canvas points.
  - [x] Increase `CHART_UPDATE_INTERVAL_MS` from 1000ms to 2000ms.
  - [x] Optimize spline curvature (`smooth: 0.2`) to reduce bezier curve tesselation on M1 GPU/CPU canvas rasterizer.
- [x] **2.2. Pause ECharts updates and history ingestion when hidden**
  - [x] Add `setChartPaused()` hook.
  - [x] Stop setting `chartOption.value` and skip canvas recalculations when `windowHidden` is active.
- [x] **2.3. Optimize `ChartPanel.vue` resize observer**
  - [x] Add `{ throttle: 300 }` to `autoresize` on `VChart` to avoid continuous layout passes during window resizing.

---

## 3. Frontend Reactivity & Background WebKit Suspension (`src/`)

- [x] **3.1. Suspend WebKit JS execution on `window-hidden` in `src/App.vue` & `src/composables/useConnection.ts`**
  - [x] Track global `isWindowHidden` reactive state.
  - [x] Gate `watch(() => state.value, ...)` so history points and UI calculations pause when window is hidden in tray.
- [x] **3.2. Remove redundant JS HTTP polling in `src/composables/useHA.ts`**
  - [x] Remove `connInterval` (10s) and `appliancePoll` (30s) `setInterval` calls in frontend, relying on Rust's WebSocket push loop.
- [x] **3.3. Optimize computed properties with regex/string sorting**
  - [x] Optimize `haLoads` and `haLoadsForConfig` in `src/composables/useHA.ts` with `loadNameCache` to avoid redundant regex formatting and sorting on every tick.

---

## 4. Verification & Benchmarking

- [x] **4.1. Automated Test Suite**
  - [x] Run `pnpm test` (vitest: 75/75 passed).
  - [x] Run `cd src-tauri && cargo check && cargo clippy --all-targets -- -D warnings && cargo test --all-targets` (17/17 passed, 0 warnings).
  - [x] Run `pnpm run format:check` (Prettier clean).
  - [x] Run `pnpm run build` (Vite + TypeScript clean build).
- [ ] **4.2. macOS M1 Activity Monitor Profiling**
  - [ ] Verify `WebKit.WebContent` CPU drops to **< 3–5%** with window open.
  - [ ] Verify `WebKit.WebContent` CPU drops to **0.0% – 0.1%** when window is hidden/minimized to tray.
  - [ ] Verify system tray icon continues updating smoothly every 1.5s with zero lag.
  - [ ] Verify restoring window from tray immediately displays up-to-date values without flicker or delay.

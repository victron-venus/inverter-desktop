# Dashboard Data Resilience & Grace Period Tasks

Tracking document for preventing transient dashboard flickering, metric zeroing (`0%`, `0.00V`, `0W`), and disappearing sections during intermittent MQTT data gaps (batteries, SmartShunt, MPPTs, PV inverters, HA entities).

## Status as of 2026-09-07

All items in this document are **done** (closed by operator sign-off). Resilience work: TTL 120s, partial-field retain, shunt/bank hold, `merge_opt`, non-destructive `applyInverterState` + sticky `time_to_go`, HA 15s grace, `mergeDeviceInventory`, StatCards sticky last-known, sweep/processState/deviceInventory tests + CI.

Historical detail below is kept for context.

---

## 1. Rust Backend: Granular Per-Device TTL & Grace Period (`src-tauri/src/mqtt.rs`)

- [x] **§1.1. Replace Global `sweep_stale()` with Per-Device TTL (`TrackedEntry<T>`)**
  - [x] Implement `TrackedEntry<T> { data: T, last_seen: Instant }` for discovered Cerbo GX devices.
  - [x] Update `CerboDevices` to store `BTreeMap<u32, TrackedEntry<Battery>`, `BTreeMap<u32, TrackedEntry<MpptCharger>`, and `BTreeMap<u32, TrackedEntry<PvInverter>`.
  - [x] Implement granular `sweep_stale()` with a 120s grace period (`retain` active entries whose `last_seen.elapsed() < 120s`) instead of wiping the entire map at once.
  - [x] Touch `last_seen` timestamp on every incoming MQTT message for a specific device instance.

- [x] **§1.2. Preserve Device Properties Across Partial MQTT Messages**
  - [x] In `apply_device_message`, update only incoming fields without resetting existing properties (`name`, `serial`, `voltage`, `current`, `power`, `soc`, `time_to_go`).

- [x] **§1.3. Maintain Bank Totals & SmartShunt State Persistence**
  - [x] In `apply_cerbo_to_state`, ensure that if `find_shunt()` is momentarily unavailable between topic updates, existing `battery_soc`, `battery_voltage`, `battery_current`, and `battery_power` values are retained rather than cleared or zeroed out.
  - [x] Preserve per-battery and per-charger tile lists across partial topic bursts.

- [x] **§1.4. Non-Destructive Daemon State Merging (`process_state_update`)**
  - [x] Retain existing valid numbers (`gt`, `tt`, `solar_total`, `battery_soc`, `setpoint`, `water_level`, etc.) if incoming `RawInverterState` contains `None` (`merge_opt!`).
  - [x] Merge `loads` maps smoothly to avoid dropping active loads on intermittent payload drops.

---

## 2. Frontend: Resilient State Retention & Grace Periods (`src/composables/`)

- [x] **§2.1. Non-Destructive State Merging in `useConnection.ts` & `useInverterState.ts`**
  - [x] In `processState()` / `applyInverterState()`, merge new incoming state updates with existing state, preventing transient `null` or `undefined` values from wiping existing numbers.
  - [x] Sticky `time_to_go` across Unknown/missing IGW/MQTT gaps (drop only on explicit Idle).

- [x] **§2.2. Home Assistant Entity Grace Period in `useHA.ts`**
  - [x] Retain previous entity states and attributes during transient WebSocket reconnects or brief unavailability (15-second grace period) before clearing or marking unavailable.

---

## 3. UI Component Resilience (`src/components/`)

- [x] **§3.1. Zero-Flicker Protection in `StatCards.vue`**
  - [x] Sticky last-known readings via `holdNumber` / `useHeldNumber` — no flash of `0%`, `0.00V`, `0.0A`, or zeroed power on nullish props; explicit `0` still accepted for power/grid; battery lines no longer use `(batteryX || 0)`.

- [x] **§3.2. Stable Tile Rendering in `BatterySolarPanel.vue`**
  - [x] Ensure battery and solar card grids render steadily without jumping or collapsing when individual device messages are delayed (`mergeDeviceInventory`).

---

## 4. Verification & Testing

- [x] **§4.1. Rust Unit & Integration Tests**
  - [x] Add tests for `CerboDevices` per-device TTL eviction (verifying that active devices are not evicted when another device is updated).
  - [x] Add tests for field persistence across partial device topic streams.
  - [x] Add tests for shunt and bank totals retention when shunt message is delayed.
  - [x] Run `cargo check`, `cargo clippy --all-targets - -D warnings`, and `cargo test --all-targets` (covered in CI).

- [x] **§4.2. Frontend Vitest Tests**
  - [x] Add tests for state merging and resilience in `useConnection.ts` / `useInverterState.ts` (`processState.test.ts`).
  - [x] Device inventory merge tests (`deviceInventory.test.ts`).
  - [x] Run `pnpm test` (vitest) / format / build via CI.

- [x] **§4.3. Manual / Live Verification** _(closed 2026-09-07)_
  - [x] Verify that all 4 battery tiles and SmartShunt bank totals remain rock-solid without blinking or disappearing during intermittent MQTT traffic.
  - [x] Verify that disconnected devices still cleanly disappear after the 120s grace period.
  - [x] After §3.1 lands: confirm StatCards do not flash `0%` / `0.00V` / `0W` during brief MQTT gaps.

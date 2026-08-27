# Dashboard Data Resilience & Grace Period Tasks

Tracking document for preventing transient dashboard flickering, metric zeroing (`0%`, `0.00V`, `0W`), and disappearing sections during intermittent MQTT data gaps (batteries, SmartShunt, MPPTs, PV inverters, HA entities).

---

## 1. Rust Backend: Granular Per-Device TTL & Grace Period (`src-tauri/src/mqtt.rs`)

- [ ] **1.1. Replace Global `sweep_stale()` with Per-Device TTL (`TrackedEntry<T>`)**
  - [ ] Implement `TrackedEntry<T> { data: T, last_seen: Instant }` for discovered Cerbo GX devices.
  - [ ] Update `CerboDevices` to store `BTreeMap<u32, TrackedEntry<Battery>>`, `BTreeMap<u32, TrackedEntry<MpptCharger>>`, and `BTreeMap<u32, TrackedEntry<PvInverter>>`.
  - [ ] Implement granular `sweep_stale()` with a 120s grace period (`retain` active entries whose `last_seen.elapsed() < 120s`) instead of wiping the entire map at once.
  - [ ] Touch `last_seen` timestamp on every incoming MQTT message for a specific device instance.

- [ ] **1.2. Preserve Device Properties Across Partial MQTT Messages**
  - [ ] In `apply_device_message`, update only incoming fields without resetting existing properties (`name`, `serial`, `voltage`, `current`, `power`, `soc`, `time_to_go`).

- [ ] **1.3. Maintain Bank Totals & SmartShunt State Persistence**
  - [ ] In `apply_cerbo_to_state`, ensure that if `find_shunt()` is momentarily unavailable between topic updates, existing `battery_soc`, `battery_voltage`, `battery_current`, and `battery_power` values are retained rather than cleared or zeroed out.
  - [ ] Preserve per-battery and per-charger tile lists across partial topic bursts.

- [ ] **1.4. Non-Destructive Daemon State Merging (`process_state_update`)**
  - [ ] Retain existing valid numbers (`gt`, `tt`, `solar_total`, `battery_soc`, `setpoint`, `water_level`, etc.) if incoming `RawInverterState` contains `None`.
  - [ ] Merge `loads` maps smoothly to avoid dropping active loads on intermittent payload drops.

---

## 2. Frontend: Resilient State Retention & Grace Periods (`src/composables/`)

- [ ] **2.1. Non-Destructive State Merging in `useConnection.ts` & `useInverterState.ts`**
  - [ ] In `processState()`, merge new incoming state updates with existing state, preventing transient `null` or `undefined` values from wiping existing numbers.

- [ ] **2.2. Home Assistant Entity Grace Period in `useHA.ts`**
  - [ ] Retain previous entity states and attributes during transient WebSocket reconnects or brief unavailability (15-second grace period) before clearing or marking unavailable.

---

## 3. UI Component Resilience (`src/components/`)

- [ ] **3.1. Zero-Flicker Protection in `StatCards.vue`**
  - [ ] Ensure formatting and displays maintain last-known valid readings without flashing `0%`, `0.00V`, or `0.0A`.

- [ ] **3.2. Stable Tile Rendering in `BatterySolarPanel.vue`**
  - [ ] Ensure battery and solar card grids render steadily without jumping or collapsing when individual device messages are delayed.

---

## 4. Verification & Testing

- [ ] **4.1. Rust Unit & Integration Tests**
  - [ ] Add tests for `CerboDevices` per-device TTL eviction (verifying that active devices are not evicted when another device is updated).
  - [ ] Add tests for field persistence across partial device topic streams.
  - [ ] Add tests for shunt and bank totals retention when shunt message is delayed.
  - [ ] Run `cargo check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets`.

- [ ] **4.2. Frontend Vitest Tests**
  - [ ] Add tests for state merging and resilience in `useConnection.ts` and `useInverterState.ts`.
  - [ ] Run `pnpm test` (vitest).
  - [ ] Run `pnpm run format:check` and `pnpm run build`.

- [ ] **4.3. Manual / Live Verification**
  - [ ] Verify that all 4 battery tiles and SmartShunt bank totals remain rock-solid without blinking or disappearing during intermittent MQTT traffic.
  - [ ] Verify that disconnected devices still cleanly disappear after the 120s grace period.

use chrono::Utc;
use rumqttc::{Client, MqttOptions, QoS, SubscribeFilter};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;

use crate::ha_api::HaEntityEntry;

const MQTT_KEEP_ALIVE_SECS: u64 = 60;
const KEEPALIVE_INTERVAL_SECS: u64 = 45;
/// Must stay above the burst of control requests we enqueue from inside
/// `connection.iter()` handlers. rumqttc `Client::subscribe` uses a *blocking*
/// send on this channel; if the handler fills it while the event loop is
/// stuck in that same handler, the MQTT thread deadlocks permanently.
/// `subscribe_portal_topics` historically issued one subscribe per filter; with
/// acload (3512a15) that became 11 > 10 and froze the loop right after
/// "Discovered Cerbo portal ID" — UI stuck at all zeros, no state emits.
const MQTT_QUEUE_CAPACITY: usize = 64;
const CONSOLE_MAX_LINES: usize = 50;
const MIN_STATE_EMIT_INTERVAL: Duration = Duration::from_millis(500);

/// Coalesce high-frequency MQTT state IPC: emit at most every
/// `MIN_STATE_EMIT_INTERVAL`, and when updates arrive during the quiet
/// window schedule a single trailing flush of the *latest* snapshot so the
/// UI never freezes on a dropped update (DROP throttle had no trailing emit).
struct StateEmitCoalesce {
    last_emit: Option<Instant>,
    pending: Option<InverterState>,
    flush_scheduled: bool,
}

static STATE_EMIT_COALESCE: std::sync::LazyLock<Mutex<StateEmitCoalesce>> =
    std::sync::LazyLock::new(|| {
        Mutex::new(StateEmitCoalesce {
            last_emit: None,
            pending: None,
            flush_scheduled: false,
        })
    });

/// Rate-limited counter so we can confirm mqtt-state-update IPC is flowing
/// without flooding the log (one line every ~5s).
fn note_state_emit(kind: &str) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static EMITS: AtomicU64 = AtomicU64::new(0);
    static LAST_LOG_MS: AtomicU64 = AtomicU64::new(0);
    let n = EMITS.fetch_add(1, Ordering::Relaxed) + 1;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let prev = LAST_LOG_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(prev) >= 5000
        && LAST_LOG_MS
            .compare_exchange(prev, now_ms, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        log::info!("mqtt-state-update emitted x{n} (last={kind})");
    }
}

/// Rate-limited confirmation that inverter/state MQTT messages are arriving
/// and parsing (one line every ~5s). Absence of this line with portal
/// discovery present means the daemon is not publishing or the MQTT loop
/// is stuck before handle_message.
fn note_inverter_state_recv(raw: &RawInverterState) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RECVS: AtomicU64 = AtomicU64::new(0);
    static LAST_LOG_MS: AtomicU64 = AtomicU64::new(0);
    let n = RECVS.fetch_add(1, Ordering::Relaxed) + 1;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let prev = LAST_LOG_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(prev) >= 5000
        && LAST_LOG_MS
            .compare_exchange(prev, now_ms, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        log::info!(
            "inverter/state received x{n} (gt={:?} tt={:?} soc={:?})",
            raw.gt,
            raw.tt,
            raw.battery_soc
        );
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InverterState {
    pub gt: Option<f64>,
    pub g1: Option<f64>,
    pub g2: Option<f64>,
    pub tt: Option<f64>,
    pub t1: Option<f64>,
    pub t2: Option<f64>,
    pub solar_total: Option<f64>,
    pub mppt_total: Option<f64>,
    pub battery_soc: Option<f64>,
    pub battery_power: Option<f64>,
    pub battery_voltage: Option<f64>,
    pub battery_current: Option<f64>,
    pub setpoint: Option<f64>,
    pub inverter_state: Option<String>,
    pub version: Option<String>,
    pub dashboard_version: Option<String>,
    pub uptime: Option<u64>,
    pub ha_connected: Option<bool>,
    pub ha_direct_connected: Option<bool>,
    pub dry_run: Option<bool>,
    pub ess_mode: Option<EssMode>,
    pub booleans: Option<std::collections::HashMap<String, bool>>,
    pub features: Option<std::collections::HashMap<String, bool>>,
    pub mppt_individual: Option<Vec<f64>>,
    pub mppt_chargers: Option<Vec<MpptCharger>>,
    pub pv_inverters: Option<Vec<PvInverter>>,
    pub pv_inverter_individual: Option<Vec<f64>>,
    pub batteries: Option<Vec<Battery>>,
    pub loads: Option<std::collections::HashMap<String, f64>>,
    /// Cerbo acload instance id → CustomName/ProductName. Loads stay keyed by
    /// stable instance id so power updates never flash raw ids once a name is known.
    pub load_names: Option<std::collections::HashMap<String, String>>,
    pub ui_config: Option<UiConfig>,
    pub daily_stats: Option<DailyStats>,
    pub solar_forecast: Option<SolarForecast>,
    pub ev_charging_kw: Option<f64>,
    pub ev_power: Option<f64>,
    pub car_soc: Option<f64>,
    pub ev_charging_power: Option<f64>,
    pub car_charging_power: Option<f64>,
    /// Cerbo MQTT has published at least one ev/<i>/... message for a
    /// configured ev_instance. Survives process_state_update clones/merges.
    pub ev_present: bool,
    /// Same for evcharger/<i>/... messages.
    pub evcharger_present: bool,
    pub water_level: Option<f64>,
    pub water_valve: Option<bool>,
    pub pump_switch: Option<bool>,
    pub dishwasher_running: Option<bool>,
    pub dishwasher_duration: Option<u64>,
    pub washer_time: Option<u64>,
    pub washer_power: Option<bool>,
    pub dryer_time: Option<u64>,
    pub dryer_power: Option<bool>,
    pub latest_version: Option<String>,
    pub console: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct RawInverterState {
    gt: Option<f64>,
    g1: Option<f64>,
    g2: Option<f64>,
    tt: Option<f64>,
    t1: Option<f64>,
    t2: Option<f64>,
    solar_total: Option<f64>,
    battery_soc: Option<f64>,
    // Canonical + short battery keys. Do NOT use #[serde(alias = "bp")] etc:
    // the daemon publishes BOTH forms in one object, and serde aliases treat
    // them as the same field → "duplicate field `battery_power`" and the
    // entire inverter/state payload is rejected (Consumption/Setpoint stay 0).
    battery_power: Option<f64>,
    battery_voltage: Option<f64>,
    battery_current: Option<f64>,
    /// Short key historically published alongside/instead of battery_power.
    bp: Option<f64>,
    /// Short key historically published alongside/instead of battery_voltage.
    bv: Option<f64>,
    /// Short key historically published alongside/instead of battery_current.
    bc: Option<f64>,
    setpoint: Option<f64>,
    inverter_state: Option<String>,
    version: Option<String>,
    dashboard_version: Option<String>,
    uptime: Option<u64>,
    ha_connected: Option<bool>,
    ha_direct_connected: Option<bool>,
    dry_run: Option<serde_json::Value>,
    ess_mode: Option<EssMode>,
    booleans: Option<std::collections::HashMap<String, serde_json::Value>>,
    features: Option<std::collections::HashMap<String, bool>>,
    mppt_individual: Option<Vec<f64>>,
    mppt_chargers: Option<Vec<MpptCharger>>,
    pv_inverters: Option<Vec<PvInverter>>,
    pv_inverter_individual: Option<Vec<f64>>,
    batteries: Option<Vec<Battery>>,
    loads: Option<std::collections::HashMap<String, f64>>,
    ui_config: Option<UiConfig>,
    daily_stats: Option<DailyStats>,
    solar_forecast: Option<SolarForecast>,
    // ev_charging_kw, ev_power, car_soc are intentionally absent —
    // EV telemetry comes only from Cerbo MQTT via apply_ev_message + EvCache,
    // not from the daemon's inverter/state payload.
    // battery_*, loads, grid/consumption, setpoint/mode, MPPT/PV/batteries
    // arrays, solar_total, and water may still appear in the JSON for
    // fallback; process_state_update skips merging them once Cerbo (or HA
    // for appliances) owns those tiles — same EV wipe-protection pattern.
    // water_* / washer_* / dryer_* / dishwasher_*: intentionally absent —
    // water from Cerbo tank/pump handlers; appliances from HA entities only.
    // Ignoring them in JSON prevents daemon zeros from being tempting to merge.
    latest_version: Option<String>,
    console: Option<Vec<String>>,
}

impl RawInverterState {
    /// Prefer canonical battery_* keys; fall back to short bp/bv/bc when the
    /// long form is absent. Safe when both are present (no serde duplicate).
    fn resolve_short_battery_keys(&mut self) {
        if self.battery_power.is_none() {
            self.battery_power = self.bp;
        }
        if self.battery_voltage.is_none() {
            self.battery_voltage = self.bv;
        }
        if self.battery_current.is_none() {
            self.battery_current = self.bc;
        }
    }
}

fn coerce_bool(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => {
            let s_low = s.to_lowercase();
            s_low == "true" || s_low == "1" || s_low == "on" || s_low == "online"
        }
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

/// Bank % from pack voltage — same paradigm as the HA "Battery %" template
/// sensor: linear 40-54.4 V, clamped to 0-100, rounded. The shunt's own SoC
/// counter reads a bogus 100% while charging, so the UI never shows it.
fn voltage_soc(voltage: f64) -> f64 {
    const V_MIN: f64 = 40.0; // V -> 0%
    const V_MAX: f64 = 54.4; // V -> 100% (absorption)
    (((voltage - V_MIN) / (V_MAX - V_MIN)) * 100.0)
        .clamp(0.0, 100.0)
        .round()
}

/// Victron VE.Bus /State codes — mirrors inverter-control INVERTER_STATES.
fn inverter_state_name(code: u32) -> String {
    match code {
        0 => "Off".into(),
        1 => "Low Power".into(),
        2 => "Fault".into(),
        3 => "Bulk".into(),
        4 => "Absorption".into(),
        5 => "Float".into(),
        6 => "Storage".into(),
        7 => "Equalize".into(),
        8 => "Passthru".into(),
        9 => "Inverting".into(),
        10 => "Power assist".into(),
        11 => "Power supply".into(),
        252 => "External control".into(),
        other => format!("? ({other})"),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EssMode {
    pub mode_name: Option<String>,
    pub is_external: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MpptCharger {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pv_voltage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<f64>,
}

/// AC PV inverter of any vendor (Tasmota plug, ESPHome, Fronius, ...):
/// published on the GX broker by dbus services as N/<portal>/pvinverter/<id>.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PvInverter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voltage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Vebus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l1_power: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l2_power: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ac_power: Option<f64>,
    /// Hub4/L1/AcPowerSetpoint (W).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setpoint: Option<f64>,
    /// VE.Bus /State label (Bulk, Absorption, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inverter_state: Option<String>,
}

/// systemcalc aggregates on com.victronenergy.system (MQTT N/.../system/0/...).
#[derive(Debug, Clone, Default)]
struct SystemTotals {
    g1: Option<f64>,
    g2: Option<f64>,
    t1: Option<f64>,
    t2: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Battery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soc: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voltage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_go: Option<String>,
}

/// Wrapper that tracks when a device was last seen via MQTT, enabling
/// per-device TTL eviction instead of a global map wipe.
struct TrackedEntry<T> {
    data: T,
    last_seen: Instant,
}

impl<T: Default> Default for TrackedEntry<T> {
    fn default() -> Self {
        Self {
            data: T::default(),
            last_seen: Instant::now(),
        }
    }
}

impl<T> TrackedEntry<T> {
    fn touch(&mut self) {
        self.last_seen = Instant::now();
    }
}

/// Devices discovered directly on the Cerbo GX MQTT broker
/// (N/<portal>/battery/..., N/<portal>/solarcharger/...,
/// N/<portal>/pvinverter/...), independent of the inverter-control daemon's
/// inverter/state payload. BTreeMap keeps a stable instance-ordered list for
/// the UI.
/// Tile identity for duplicate-name disambiguation: identical units ship one
/// shared ProductName ("SmartSolar Charger MPPT 100/20 48V" x N), which reads
/// as the same tile repeated.
trait DeviceIdentity {
    fn display_name(&self) -> Option<&str>;
    fn name_slot(&mut self) -> &mut Option<String>;
    fn serial(&self) -> Option<&str>;
}

impl DeviceIdentity for MpptCharger {
    fn display_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn name_slot(&mut self) -> &mut Option<String> {
        &mut self.name
    }
    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }
}

impl DeviceIdentity for PvInverter {
    fn display_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn name_slot(&mut self) -> &mut Option<String> {
        &mut self.name
    }
    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }
}

impl DeviceIdentity for Battery {
    fn display_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn name_slot(&mut self) -> &mut Option<String> {
        &mut self.name
    }
    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }
}

/// One Victron acload service (dbus-emporia-vue circuit, etc.) discovered on
/// the Cerbo GX MQTT broker. Watts stay keyed by instance; display name is
/// cached separately so UI never flickers back to bare ids.
#[derive(Debug, Clone, Default)]
struct AcLoad {
    power: Option<f64>,
    /// CustomName when published (preferred).
    custom_name: Option<String>,
    /// ProductName fallback when CustomName has not arrived yet.
    product_name: Option<String>,
}

impl AcLoad {
    fn display_name(&self) -> Option<&str> {
        self.custom_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| self.product_name.as_deref().filter(|s| !s.is_empty()))
    }
}

/// Devices discovered directly on the Cerbo GX MQTT broker
/// (N/<portal>/battery/..., N/<portal>/solarcharger/...,
/// N/<portal>/pvinverter/..., N/<portal>/acload/...), independent of the
/// inverter-control daemon's inverter/state payload. BTreeMap keeps a stable
/// instance-ordered list for the UI.
#[derive(Default)]
struct CerboDevices {
    batteries: BTreeMap<u32, TrackedEntry<Battery>>,
    chargers: BTreeMap<u32, TrackedEntry<MpptCharger>>,
    pv_inverters: BTreeMap<u32, TrackedEntry<PvInverter>>,
    vebus: BTreeMap<u32, TrackedEntry<Vebus>>,
    /// system/0 Ac/Grid + Ac/Consumption (preferred source for gt/tt).
    system: BTreeMap<u32, TrackedEntry<SystemTotals>>,
    acloads: BTreeMap<u32, TrackedEntry<AcLoad>>,
}

impl CerboDevices {
    /// How long a discovered entry survives without any fresh publish.
    const DEVICE_TTL_SECS: u64 = 120;

    /// Evict entries whose `last_seen` is older than the TTL. Unlike the old
    /// global map wipe, this preserves active entries when other devices are
    /// updated — only truly stale (disconnected) devices are removed.
    fn sweep_stale(&mut self) {
        let ttl = Duration::from_secs(Self::DEVICE_TTL_SECS);
        self.batteries
            .retain(|_, entry| entry.last_seen.elapsed() < ttl);
        self.chargers
            .retain(|_, entry| entry.last_seen.elapsed() < ttl);
        self.pv_inverters
            .retain(|_, entry| entry.last_seen.elapsed() < ttl);
        self.vebus
            .retain(|_, entry| entry.last_seen.elapsed() < ttl);
        self.system
            .retain(|_, entry| entry.last_seen.elapsed() < ttl);
        self.acloads
            .retain(|_, entry| entry.last_seen.elapsed() < ttl);
    }

    fn has_shunt(&self) -> bool {
        self.batteries.values().any(|e| {
            e.data
                .name
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains("shunt")
        })
    }

    /// systemcalc Ac/Grid seen — Cerbo owns gt/g1/g2 (vebus is fallback only).
    fn owns_grid(&self) -> bool {
        self.system
            .values()
            .any(|e| e.data.g1.is_some() || e.data.g2.is_some())
            || self.vebus.values().any(|e| {
                e.data.l1_power.is_some() || e.data.l2_power.is_some() || e.data.ac_power.is_some()
            })
    }

    /// systemcalc Ac/Consumption seen — Cerbo owns tt/t1/t2.
    fn owns_consumption(&self) -> bool {
        self.system
            .values()
            .any(|e| e.data.t1.is_some() || e.data.t2.is_some())
    }

    /// VE.Bus discovered — Cerbo owns Hub4 setpoint + /State label.
    fn owns_vebus_mode(&self) -> bool {
        self.vebus
            .values()
            .any(|e| e.data.setpoint.is_some() || e.data.inverter_state.is_some())
    }

    fn owns_chargers(&self) -> bool {
        !self.chargers.is_empty()
    }

    fn owns_pv(&self) -> bool {
        !self.pv_inverters.is_empty()
    }

    fn owns_batteries(&self) -> bool {
        !self.batteries.is_empty()
    }

    fn owns_solar(&self) -> bool {
        self.owns_chargers() || self.owns_pv()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiConfig {
    pub loads: Option<LoadsConfig>,
    pub home_buttons: Option<Vec<HomeButton>>,
    pub header_toggles: Option<Vec<HeaderToggle>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoadsConfig {
    pub hidden: Option<Vec<String>>,
    pub min_watts: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeButton {
    pub id: String,
    pub label: String,
    pub entity: String,
    pub state_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderToggle {
    pub id: String,
    pub label: String,
    pub entity: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyStats {
    pub produced_today: Option<f64>,
    pub produced_yesterday: Option<f64>,
    pub produced_dollars: Option<f64>,
    pub grid_kwh: Option<f64>,
    pub battery_in: Option<f64>,
    pub battery_out: Option<f64>,
    pub battery_in_yesterday: Option<f64>,
    pub battery_out_yesterday: Option<f64>,
    pub pv_inverter_daily: Option<Vec<f64>>,
    pub mppt_daily: Option<Vec<f64>>,
    pub pv_total_daily: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SolarForecast {
    pub date: Option<String>,
    pub generated_at: Option<String>,
    pub today_kwh: Option<f64>,
    pub tomorrow_kwh: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraEvent {
    pub agent_name: String,
    pub video_url: String,
    pub timestamp: Option<String>,
}

struct AlertState {
    triggered: bool,
    last_alert: Option<std::time::Instant>,
    last_notified_value: Option<f64>,
}

impl AlertState {
    fn new() -> Self {
        Self {
            triggered: false,
            last_alert: None,
            last_notified_value: None,
        }
    }

    fn should_alert(&mut self) -> bool {
        match self.last_alert {
            None => {
                self.triggered = true;
                self.last_alert = Some(std::time::Instant::now());
                true
            }
            Some(last) => {
                if last.elapsed() > std::time::Duration::from_secs(NOTIFICATION_COOLDOWN_SECS) {
                    self.last_alert = Some(std::time::Instant::now());
                    true
                } else {
                    false
                }
            }
        }
    }

    fn should_alert_value(&mut self, value: f64) -> bool {
        match self.last_notified_value {
            None => {
                self.triggered = true;
                self.last_notified_value = Some(value);
                true
            }
            Some(prev) => {
                if (prev - value).abs() > f64::EPSILON {
                    self.triggered = true;
                    self.last_notified_value = Some(value);
                    true
                } else {
                    false
                }
            }
        }
    }

    fn check_resolved(&mut self) {
        if self.triggered {
            self.triggered = false;
            self.last_alert = None;
            self.last_notified_value = None;
        }
    }
}

struct NotificationState {
    high_consumption: AlertState,
    low_water: AlertState,
    high_solar: AlertState,
    high_load: std::collections::HashMap<String, AlertState>,
}

/// Last-good Cerbo GX EV sample per field with a throttle window.
///
/// dbus-ev / dbus-evcharger publish at the inverter-control poll cadence
/// (every 2 s). The desktop EV tile flickered because process_state_update
/// ran on every inverter/state message and, while building its merged
/// snapshot, cloned the state from *before* apply_ev_message landed. The
/// resulting write-back wiped the freshly-populated EV fields and the tile
/// toggled on/off in a 2 s loop. SoC never showed because inverter/state
/// published car_soc=0 when no car was connected, and merge_opt!(car_soc)
/// happily overwrote the real 0 with the daemon's 0 — wait, 0 is a
/// perfectly cromulent value. The real issue is that *missing* SoC
/// (inverter publishes 0 as "not connected") is indistinguishable from a
/// legitimate 0, so we treat 0 as no-data for SoC and refuse to clobber a
/// cached real value.
///
/// Throttle: per-field, ignore a new sample if the cached sample is younger
/// than 8 s. The cache survives process_state_update so the tile keeps
/// showing the last good value between Cerbo publishes.
#[derive(Default)]
struct EvCache {
    car_soc: Option<(f64, Instant)>,
    car_charging_power: Option<(f64, Instant)>,
    ev_charging_power: Option<(f64, Instant)>,
    /// Presence bits survive process_state_update so the EV tile never
    /// disappears on a daemon merge that wipes the EV metrics.
    ev_present: bool,
    evcharger_present: bool,
}

const EV_CACHE_TTL: Duration = Duration::from_secs(8);

impl EvCache {
    /// Apply a new Cerbo sample; returns true if the cache was updated.
    /// - car_soc: 0 is treated as no-data (refused if cache already populated).
    /// - power: 0 is a legitimate idle value, accepted.
    /// - throttle: reject a sample if the existing cache is younger than TTL.
    fn update(&mut self, field: EvField, value: f64) -> bool {
        let now = Instant::now();
        let slot = match field {
            EvField::CarSoc => {
                if value <= 0.0 && self.car_soc.is_some() {
                    return false;
                }
                &mut self.car_soc
            }
            EvField::CarChargingPower => &mut self.car_charging_power,
            EvField::EvChargingPower => &mut self.ev_charging_power,
        };
        if let Some((_, prev_ts)) = slot {
            if now.duration_since(*prev_ts) < EV_CACHE_TTL {
                return false;
            }
        }
        *slot = Some((value, now));
        true
    }

    /// Mark presence for ev/evcharger when a matching MQTT message arrives
    /// (including value 0). Presence survives process_state_update merges
    /// because daemon never publishes these fields.
    fn set_presence(&mut self, kind: &str) {
        match kind {
            "ev" => self.ev_present = true,
            "evcharger" => self.evcharger_present = true,
            _ => {}
        }
    }

    /// Re-apply the cached values AND presence bits to `st` after a
    /// daemon merge wiped them. Presence bits ensure the EV tile stays
    /// visible even when SOC/power are 0 or absent.
    ///
    /// Always overwrite: if the cache has a value (even 0), copy it onto st.
    /// Presence bits are sticky — never cleared to false.
    fn restore_into(&self, st: &mut InverterState) {
        st.ev_present = self.ev_present || st.ev_present;
        st.evcharger_present = self.evcharger_present || st.evcharger_present;
        // Always overwrite: cache value (even 0) takes precedence over None.
        if let Some((v, _)) = self.car_soc {
            st.car_soc = Some(v);
        }
        if let Some((v, _)) = self.car_charging_power {
            st.car_charging_power = Some(v);
        }
        if let Some((v, _)) = self.ev_charging_power {
            st.ev_charging_power = Some(v);
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum EvField {
    CarSoc,
    CarChargingPower,
    EvChargingPower,
}

pub struct MqttClient {
    client: Arc<Mutex<Option<Client>>>,
    client_id: String,
    pub(crate) state: Arc<Mutex<InverterState>>,
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    app_handle: Option<tauri::AppHandle>,
    portal_id: Option<String>,
    /// Cerbo GX startstop device instances for (pump, valve) water topics.
    water_instances: Option<(u32, u32)>,
    /// Cerbo GX EV (vehicle) and evcharger instance pair for EV topics.
    /// Either side may be None — dbus-ev and dbus-evcharger are independent
    /// services, so the EV tile must populate if just one is configured.
    ev_instances: Option<(Option<u32>, Option<u32>)>,
    camera_topic: Option<String>,
    notifications: Arc<Mutex<NotificationState>>,
    alarms: Arc<Mutex<HashMap<String, u8>>>,
    status_event: String,
    ha_entity_states: Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
    /// Throttled last-good EV sample cache (see EvCache docs). Wrapped in
    /// Mutex so the run_mqtt_loop closure can hold an Arc clone.
    ev_cache: Arc<Mutex<EvCache>>,
    /// Cleared by [`Self::stop`] so leaked reconnect loops from a replaced
    /// client exit instead of discovering the portal N more times.
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}

/// Notification pushed by inverter-control on {prefix}/notifications.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct MqttNotification {
    pub id: String,
    pub level: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub ts: String,
}

/// Split CamelCase into words: "HighCellVoltage" -> ["High", "Cell", "Voltage"]
fn split_camel(s: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_uppercase() && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// "HighVoltage" -> "High voltage alarm"
fn pretty_alarm_name(alarm: &str) -> String {
    let mut words = split_camel(alarm);
    let mut out = words.first().cloned().unwrap_or_default();
    for w in words.drain(1..) {
        out.push(' ');
        out.push_str(&w.to_lowercase());
    }
    out.push_str(" alarm");
    out
}

fn match_mqtt_topic(topic: &str, pattern: &str) -> bool {
    if pattern == topic || pattern == "#" {
        return true;
    }
    let t_parts: Vec<&str> = topic.split('/').collect();
    let p_parts: Vec<&str> = pattern.split('/').collect();

    if pattern.ends_with("/#") {
        let prefix_len = p_parts.len() - 1;
        if t_parts.len() < prefix_len {
            return false;
        }
        return p_parts[..prefix_len]
            .iter()
            .zip(t_parts.iter())
            .all(|(p, t)| *p == "+" || *p == *t);
    }

    // Very basic MQTT wildcard matching for +
    if t_parts.len() != p_parts.len() {
        return false;
    }
    for (t, p) in t_parts.iter().zip(p_parts.iter()) {
        if *p != "+" && *p != *t {
            return false;
        }
    }
    true
}

use tauri_plugin_notification::NotificationExt;

const THRESHOLD_LOAD_W: f64 = 1500.0;
const THRESHOLD_CONSUMPTION_W: f64 = 1500.0;
const THRESHOLD_WATER_CM: f64 = 23.0;
const THRESHOLD_SOLAR_W: f64 = 3000.0;
const NOTIFICATION_COOLDOWN_SECS: u64 = 300;

fn fmt_watts(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}kW", v / 1000.0)
    } else {
        format!("{:.0}W", v)
    }
}

/// Resolve an HA entity's friendly_name, falling back to the entity_id.
fn entity_friendly_name(entry: &HaEntityEntry) -> Option<String> {
    entry
        .attributes
        .as_ref()
        .and_then(|a| a.get("friendly_name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Find the HA friendly name for a load key (e.g. `stove` → `sensor.stove_power`'s friendly name).
/// Matches full entity ids, exact trailing segments, or ids containing the load as a segment,
/// preferring the most specific (shortest) entity id.
fn load_friendly_name(
    load: &str,
    entity_states: &HashMap<String, HaEntityEntry>,
) -> Option<String> {
    let load_lower = load.to_lowercase();
    let mut best: Option<(String, usize)> = None;
    for (entity_id, entry) in entity_states {
        let Some(name) = entity_friendly_name(entry) else {
            continue;
        };
        let eid_lower = entity_id.to_lowercase();
        let matches = eid_lower == load_lower
            || eid_lower.ends_with(&load_lower)
            || eid_lower.ends_with(&format!(".{}", load_lower))
            || eid_lower.contains(&format!(".{}", load_lower));
        if matches && best.as_ref().is_none_or(|(_, len)| entity_id.len() < *len) {
            best = Some((name, entity_id.len()));
        }
    }
    best.map(|(name, _)| name)
}

impl MqttClient {
    pub fn new(
        host: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
        client_id: String,
    ) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            client_id,
            state: Arc::new(Mutex::new(InverterState::default())),
            host,
            port,
            username,
            password,
            app_handle: None,
            portal_id: None,
            water_instances: None,
            ev_instances: None,
            camera_topic: None,
            notifications: Arc::new(Mutex::new(NotificationState {
                high_consumption: AlertState::new(),
                low_water: AlertState::new(),
                high_solar: AlertState::new(),
                high_load: std::collections::HashMap::new(),
            })),
            alarms: Arc::new(Mutex::new(HashMap::new())),
            status_event: "mqtt-connection-status".to_string(),
            ha_entity_states: None,
            ev_cache: Arc::new(Mutex::new(EvCache::default())),
            shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Stop the background reconnect loop and disconnect the broker client.
    /// Call before replacing this client in `connect_mqtt` so orphaned loops
    /// do not keep discovering the portal and fighting for messages.
    pub fn stop(&self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut slot) = self.client.lock() {
            if let Some(client) = slot.take() {
                let _ = client.disconnect();
            }
        }
    }

    pub fn set_app_handle(&mut self, handle: tauri::AppHandle) {
        self.app_handle = Some(handle);
    }

    pub fn set_ha_entity_states(&mut self, states: Arc<Mutex<HashMap<String, HaEntityEntry>>>) {
        self.ha_entity_states = Some(states);
    }

    pub fn set_portal_id(&mut self, id: Option<String>) {
        self.portal_id = id;
    }

    pub fn set_water_instances(&mut self, instances: Option<(u32, u32)>) {
        self.water_instances = instances;
    }

    pub fn set_ev_instances(&mut self, instances: Option<(Option<u32>, Option<u32>)>) {
        self.ev_instances = instances;
    }

    pub fn set_camera_topic(&mut self, topic: Option<String>) {
        self.camera_topic = topic;
    }

    pub fn set_status_event(&mut self, event: String) {
        self.status_event = event;
    }

    pub fn get_state(&self) -> InverterState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn emit_state_update(
        app_handle: &Option<tauri::AppHandle>,
        state: &InverterState,
        force: bool,
    ) {
        let hidden = crate::ha_api::WINDOW_HIDDEN.load(std::sync::atomic::Ordering::Relaxed);
        if hidden {
            // Drop any queued trailing snapshot; window-shown force-emits fresh state.
            if let Ok(mut c) = STATE_EMIT_COALESCE.lock() {
                c.pending = None;
            }
            return;
        }
        let Some(ref handle) = app_handle else {
            return;
        };

        if force {
            if let Ok(mut c) = STATE_EMIT_COALESCE.lock() {
                c.last_emit = Some(Instant::now());
                c.pending = None;
            }
            let _ = handle.emit("mqtt-state-update", state);
            note_state_emit("force");
            return;
        }

        let mut schedule_delay: Option<Duration> = None;
        let mut emit_now = false;

        if let Ok(mut c) = STATE_EMIT_COALESCE.lock() {
            let now = Instant::now();
            let since_last = c.last_emit.map(|prev| now.duration_since(prev));
            let within_interval = since_last
                .map(|d| d < MIN_STATE_EMIT_INTERVAL)
                .unwrap_or(false);

            if within_interval {
                // Coalesce: keep latest snapshot and schedule one trailing flush.
                c.pending = Some(state.clone());
                if !c.flush_scheduled {
                    c.flush_scheduled = true;
                    let elapsed = since_last.unwrap_or(Duration::ZERO);
                    schedule_delay = Some(MIN_STATE_EMIT_INTERVAL.saturating_sub(elapsed));
                }
            } else {
                c.last_emit = Some(now);
                c.pending = None;
                emit_now = true;
            }
        } else {
            // Poisoned coalesce lock — still try to deliver the update.
            emit_now = true;
        }

        if emit_now {
            let _ = handle.emit("mqtt-state-update", state);
            note_state_emit("now");
            return;
        }

        if let Some(delay) = schedule_delay {
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(delay).await;
                let to_emit = {
                    if let Ok(mut c) = STATE_EMIT_COALESCE.lock() {
                        c.flush_scheduled = false;
                        let pending = c.pending.take();
                        if pending.is_some() {
                            c.last_emit = Some(Instant::now());
                        }
                        pending
                    } else {
                        None
                    }
                };
                if let Some(snapshot) = to_emit {
                    let still_hidden =
                        crate::ha_api::WINDOW_HIDDEN.load(std::sync::atomic::Ordering::Relaxed);
                    if !still_hidden {
                        let _ = handle.emit("mqtt-state-update", &snapshot);
                        note_state_emit("flush");
                    }
                }
            });
        }
    }

    pub fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let host = self.host.clone();
        let port = self.port;
        let username = self.username.clone();
        let password = self.password.clone();
        let client_id = self.client_id.clone();

        let state = self.state.clone();
        let app_handle = self.app_handle.clone();
        let portal_id = self.portal_id.clone();
        let water_instances_owned = self.water_instances;
        let ev_instances_owned = self.ev_instances;
        let cam_topic_owned = self.camera_topic.clone();
        let notifications = self.notifications.clone();
        let alarms = self.alarms.clone();
        let status_event = self.status_event.clone();
        let ha_entity_states = self.ha_entity_states.clone();
        let client_slot = self.client.clone();
        let ev_cache = self.ev_cache.clone();
        let shutdown = self.shutdown.clone();

        tauri::async_runtime::spawn(async move {
            loop {
                if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                    log::info!("MQTT client stopped, exiting reconnect loop");
                    break;
                }
                // Log error separately so `result` drops before the await
                {
                    let is_err = Self::run_mqtt_loop(
                        &host,
                        port,
                        &username,
                        &password,
                        &client_id,
                        state.clone(),
                        app_handle.clone(),
                        portal_id.clone(),
                        water_instances_owned,
                        ev_instances_owned,
                        cam_topic_owned.clone(),
                        notifications.clone(),
                        alarms.clone(),
                        ha_entity_states.clone(),
                        &status_event,
                        client_slot.clone(),
                        ev_cache.clone(),
                    )
                    .await
                    .is_err();
                    if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                        log::info!("MQTT client stopped after disconnect");
                        break;
                    }
                    if is_err {
                        log::error!("MQTT loop ended (err), reconnecting in 5s...");
                    } else {
                        log::info!("MQTT disconnected, reconnecting in 5s...");
                    }
                    // Connection lost or failed — clear the publish slot so
                    // publish_command reports the disconnect, then wait.
                    if let Ok(mut slot) = client_slot.lock() {
                        *slot = None;
                    }
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit(&status_event, false);
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_mqtt_loop(
        host: &str,
        port: u16,
        username: &Option<String>,
        password: &Option<String>,
        client_id: &str,
        state: Arc<Mutex<InverterState>>,
        app_handle: Option<tauri::AppHandle>,
        portal_id: Option<String>,
        water_instances: Option<(u32, u32)>,
        ev_instances: Option<(Option<u32>, Option<u32>)>,
        camera_topic: Option<String>,
        notifications: Arc<Mutex<NotificationState>>,
        alarms: Arc<Mutex<HashMap<String, u8>>>,
        ha_entity_states: Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
        status_event: &str,
        client_slot: Arc<Mutex<Option<Client>>>,
        ev_cache: Arc<Mutex<EvCache>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let keepalive_secs = MQTT_KEEP_ALIVE_SECS;
        let queue_cap = MQTT_QUEUE_CAPACITY;

        // Random suffix keeps the client ID unique so a second instance or a
        // stale broker session cannot kick this client off the broker.
        let client_id = format!("{}-{:06x}", client_id, rand::random::<u32>() & 0xFF_FFFF);
        let mut mqttoptions = MqttOptions::new(&client_id, (host.to_string(), port));
        mqttoptions.set_keep_alive(keepalive_secs as u16);

        if let (Some(u), Some(p)) = (username, password) {
            if !u.is_empty() && !p.is_empty() {
                mqttoptions.set_credentials(u, p.clone());
            }
        }

        let (client, mut connection) = Client::builder(mqttoptions).capacity(queue_cap).build();

        // Store the connected client so publish_command can use it.
        if let Ok(mut slot) = client_slot.lock() {
            *slot = Some(client.clone());
        }

        // Subscribe to topics using QoS 1 (AtLeastOnce)
        client.subscribe("inverter/state", QoS::AtLeastOnce)?;
        client.subscribe("inverter/console", QoS::AtLeastOnce)?;
        client.subscribe("inverter/notifications", QoS::AtLeastOnce)?;
        // Portal ID advertised by inverter-control (retained) - lets the app
        // find the N/<portal>/... water/alarms topics with no manual config.
        client.subscribe("inverter/portal", QoS::AtLeastOnce)?;

        // Victron alarms + dbus-pump water topics for a configured portal
        let mut active_portal: Option<String> = None;
        if let Some(id) = portal_id.as_deref().filter(|s| !s.is_empty()) {
            Self::subscribe_portal_topics(&client, id);
            Self::spawn_keepalive(client.clone(), id.to_string());
            active_portal = Some(id.to_string());
        }

        if let Some(ref cam_topic) = camera_topic {
            if !cam_topic.is_empty() {
                client.subscribe(cam_topic, QoS::AtMostOnce)?;
            }
        }

        // NOTE: use tokio net (async) instead of blocking rumqttc sync iter.
        // Since rumqttc's AsyncClient/disconnection requires refactor, keep
        // spawn_blocking for backward compat but treat EOF as reconnect signal.
        let state_c = state.clone();
        let app_c = app_handle.clone();
        let cam_c = camera_topic.clone();
        let water_c = water_instances;
        let ev_c = ev_instances;
        let notif_c = notifications.clone();
        let alarms_c = alarms.clone();
        let ha_states_c = ha_entity_states.clone();
        let cerbo_devices: Arc<Mutex<CerboDevices>> = Arc::new(Mutex::new(CerboDevices::default()));
        let cerbo_c = cerbo_devices.clone();
        let ev_cache_c = ev_cache.clone();
        let se = status_event.to_string();
        let con_result = tokio::task::spawn_blocking(move || {
            // Portal discovered at runtime via the retained inverter/portal
            // topic (inverter-control publishes it when no ID is configured).
            for event in connection.iter() {
                match event {
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish))) => {
                        // Use closures instead of closures capturing vars below
                        let topic = String::from_utf8_lossy(&publish.topic).to_string();
                        let payload = String::from_utf8(publish.payload.to_vec())
                            .unwrap_or_else(|_| String::new());

                        if topic == "inverter/portal" {
                            let id = payload.trim().to_string();
                            if !id.is_empty() && active_portal.as_deref() != Some(id.as_str()) {
                                log::info!("Discovered Cerbo portal ID {}", id);
                                active_portal = Some(id.clone());
                                Self::subscribe_portal_topics(&client, &id);
                                Self::spawn_keepalive(client.clone(), id);
                            }
                            continue;
                        }

                        Self::handle_message(
                            &topic,
                            &payload,
                            &state_c,
                            &app_c,
                            &cam_c,
                            &water_c,
                            &ev_c,
                            &notif_c,
                            &alarms_c,
                            &ha_states_c,
                            &cerbo_c,
                            &ev_cache_c,
                        );
                    }
                    Ok(rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(_))) => {
                        if let Some(ref handle) = app_c {
                            let _ = handle.emit(&se, true);
                        }
                    }
                    Ok(rumqttc::Event::Incoming(_)) => {}
                    Err(e) => {
                        log::error!("MQTT error: {:?}", e);
                        // Emit disconnect and return (exit for reconnect)
                        if let Some(ref handle) = app_c {
                            let _ = handle.emit(&se, false);
                        }
                        return Err(e.into());
                    }
                    _ => {}
                }
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await;

        match con_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(e) => return Err(e.into()),
        }

        // Connection ended cleanly (EOF) — signal reconnect
        if let Some(ref handle) = app_handle {
            let _ = handle.emit(status_event, false);
        }
        Ok(())
    }

    /// Subscribe the GX portal topics (alarms + dbus-pump water + active loads).
    ///
    /// Uses a single `subscribe_many` request so we never fill rumqttc's
    /// bounded request channel from inside `connection.iter()` (that deadlocks
    /// the event loop — see MQTT_QUEUE_CAPACITY).
    fn subscribe_portal_topics(client: &Client, id: &str) {
        let filters = [
            format!("N/{}/+/Alarms/#", id),
            // Water system published by dbus-pump on the GX (tank level %,
            // pump/valve startstop state).
            format!("N/{}/tank/+/Level", id),
            format!("N/{}/pump/+/State", id),
            // EV system: dbus-ev (vehicle SoC/Power) and dbus-evcharger (charger Power)
            // dbus-ev uses bus name com.victronenergy.evcharger.<N>, so Soc/Power land
            // under evcharger/<instance>/ path — subscribe both ev/ and evcharger/ to be safe.
            format!("N/{}/ev/+/Soc", id),
            format!("N/{}/ev/+/Ac/Power", id),
            format!("N/{}/evcharger/+/Soc", id),
            format!("N/{}/evcharger/+/Ac/Power", id),
            // Active loads: Victron acload services (dbus-emporia-vue etc.).
            // Wildcard covers Ac/Power + CustomName + ProductName so names can
            // arrive after watts without a second subscribe burst.
            format!("N/{}/acload/+/#", id),
            // Directly discovered GX devices: battery bank(s) + MPPT chargers
            // + AC PV inverters of any vendor, so the app finds them even
            // when inverter-control is down.
            format!("N/{}/battery/+/#", id),
            format!("N/{}/solarcharger/+/#", id),
            format!("N/{}/pvinverter/+/#", id),
            // VE.Bus: ActiveIn grid fallback, Hub4 setpoint, /State.
            format!("N/{}/vebus/+/#", id),
            // systemcalc: Ac/Grid + Ac/Consumption (tt/t1/t2) without daemon.
            format!("N/{}/system/+/#", id),
        ];
        let n = filters.len();
        let topics: Vec<SubscribeFilter> = filters
            .into_iter()
            .map(|path| SubscribeFilter::new(path, QoS::AtLeastOnce))
            .collect();
        if let Err(e) = client.subscribe_many(topics) {
            log::warn!("Failed to subscribe portal topics for {}: {:?}", id, e);
        } else {
            log::info!("Subscribed to {n} Cerbo portal topic filters for {id}");
        }
    }

    /// Periodic R/<portal>/keepalive publisher so the Cerbo GX MQTT broker
    /// keeps accepting this client.
    fn spawn_keepalive(client: Client, id: String) {
        let topic = format!("R/{}/keepalive", id);
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(KEEPALIVE_INTERVAL_SECS));
            loop {
                interval.tick().await;
                let _ = client.publish(&topic, QoS::AtMostOnce, false, "");
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_message(
        topic: &str,
        payload: &str,
        state: &Arc<Mutex<InverterState>>,
        app_handle: &Option<tauri::AppHandle>,
        camera_topic: &Option<String>,
        water_instances: &Option<(u32, u32)>,
        ev_instances: &Option<(Option<u32>, Option<u32>)>,
        notifications: &Arc<Mutex<NotificationState>>,
        alarms: &Arc<Mutex<HashMap<String, u8>>>,
        ha_entity_states: &Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
        cerbo_devices: &Arc<Mutex<CerboDevices>>,
        ev_cache: &Arc<Mutex<EvCache>>,
    ) {
        if topic == "inverter/state" {
            match serde_json::from_str::<RawInverterState>(payload) {
                Ok(mut raw) => {
                    raw.resolve_short_battery_keys();
                    note_inverter_state_recv(&raw);
                    Self::process_state_update(
                        raw,
                        state.clone(),
                        app_handle.clone(),
                        notifications.clone(),
                        ha_entity_states.clone(),
                        Some(cerbo_devices.clone()),
                        ev_cache.clone(),
                    );
                }
                Err(e) => {
                    log::warn!(
                        "Bad inverter/state payload ({} bytes): {}",
                        payload.len(),
                        e
                    );
                }
            }
        } else if topic == "inverter/notifications" {
            match serde_json::from_str::<MqttNotification>(payload) {
                Ok(mut notification) => {
                    // Ensure timestamp is present (add if missing)
                    if notification.ts.is_empty() {
                        notification.ts = Utc::now().to_rfc3339();
                    }
                    if let Some(ref handle) = app_handle {
                        let _ = handle.emit("mqtt-notification", &notification);
                        // Mirror to OS notification like local alerts
                        let _ = handle
                            .notification()
                            .builder()
                            .title(&notification.title)
                            .body(&notification.body)
                            .show();
                    }
                }
                Err(e) => log::warn!("Bad notification payload on {}: {}", topic, e),
            }
        } else if topic.starts_with("N/") && topic.contains("/Alarms/") {
            Self::handle_alarm_message(topic, payload, alarms, app_handle);
        } else if topic == "inverter/console" {
            let snapshot = {
                let mut guard = match state.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                let console = guard.console.get_or_insert_with(Vec::new);
                console.push(payload.to_string());
                if console.len() > CONSOLE_MAX_LINES {
                    console.remove(0);
                }
                guard.clone()
            };
            Self::emit_state_update(app_handle, &snapshot, false);
        } else if topic.starts_with("N/") && Self::parse_device_topic(topic).is_some() {
            // Directly discovered GX device value (battery/solarcharger).
            if let Some((kind, inst, path)) = Self::parse_device_topic(topic) {
                let applied = cerbo_devices
                    .lock()
                    .ok()
                    .map(|mut d| {
                        d.sweep_stale();
                        Self::apply_device_message(&mut d, kind, inst, path, payload)
                    })
                    .unwrap_or(false);
                if applied {
                    let snapshot = {
                        let mut guard = match state.lock() {
                            Ok(g) => g,
                            Err(_) => return,
                        };
                        if let Ok(d) = cerbo_devices.lock() {
                            Self::apply_cerbo_to_state(&d, &mut guard);
                        }
                        guard.clone()
                    };
                    Self::emit_state_update(app_handle, &snapshot, false);
                }
            }
        } else if topic.starts_with("N/") && Self::parse_water_topic(topic).is_some() {
            // dbus-pump on the GX: N/<portal>/tank/<i>/Level (%),
            // N/<portal>/pump/<i>/State (0 stopped, 1 running).
            if let (Some((kind, inst)), Some((pump_i, valve_i)), Some(value)) = (
                Self::parse_water_topic(topic),
                water_instances,
                Self::parse_cerbo_value(payload),
            ) {
                let snapshot = {
                    let mut guard = match state.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    match (kind, inst) {
                        ("tank", _) => guard.water_level = Some(value),
                        ("pump", i) if i == *pump_i => guard.pump_switch = Some(value >= 0.5),
                        ("pump", i) if i == *valve_i => guard.water_valve = Some(value >= 0.5),
                        _ => {}
                    }
                    guard.clone()
                };
                Self::emit_state_update(app_handle, &snapshot, false);
            }
        } else if topic.starts_with("N/") && Self::parse_ev_topic(topic).is_some() {
            // dbus-ev / dbus-evcharger on the GX:
            // N/<portal>/ev/<i>/Soc, /ev/<i>/Ac/Power (W),
            // N/<portal>/evcharger/<i>/Ac/Power (W).
            // Apply per-side: each kind matches its own configured instance;
            // either side may be None and the other still applies.
            if let (Some((kind, inst, path)), Some(value)) = (
                Self::parse_ev_topic(topic),
                Self::parse_cerbo_value(payload),
            ) {
                let snapshot = {
                    let (mut guard, mut cache) = match (state.lock(), ev_cache.lock()) {
                        (Ok(g), Ok(c)) => (g, c),
                        _ => return,
                    };
                    if Self::apply_ev_message(
                        &mut guard,
                        &mut cache,
                        kind,
                        inst,
                        path,
                        value,
                        ev_instances,
                    )
                    .is_none()
                    {
                        return;
                    }
                    guard.clone()
                };
                Self::emit_state_update(app_handle, &snapshot, false);
            }
        } else if topic.starts_with("N/") && Self::parse_acload_topic(topic).is_some() {
            // dbus-emporia-vue / Victron acload on the GX:
            // N/<portal>/acload/<i>/Ac/Power | CustomName | ProductName.
            // Cache under CerboDevices (stable instance key); overlay onto
            // state.loads / state.load_names so daemon merges cannot rename.
            let Some((inst, path)) = Self::parse_acload_topic(topic) else {
                return;
            };
            let applied = cerbo_devices
                .lock()
                .ok()
                .map(|mut d| {
                    d.sweep_stale();
                    Self::apply_acload_message(&mut d, inst, path, payload)
                })
                .unwrap_or(false);
            if applied {
                let snapshot = {
                    let mut guard = match state.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    if let Ok(d) = cerbo_devices.lock() {
                        Self::apply_cerbo_to_state(&d, &mut guard);
                    }
                    guard.clone()
                };
                Self::emit_state_update(app_handle, &snapshot, false);
            }
        } else if let Some(ref cam_t) = camera_topic {
            if match_mqtt_topic(topic, cam_t) {
                if let Some(ref handle) = app_handle {
                    if let Ok(cam_event) = serde_json::from_str::<CameraEvent>(payload) {
                        let _ = handle.emit("camera-event", cam_event);
                    } else {
                        let _ = handle.emit(
                            "camera-event",
                            CameraEvent {
                                agent_name: "Unknown Camera".to_string(),
                                video_url: payload.to_string(),
                                timestamp: None,
                            },
                        );
                    }
                }
            }
        }
    }

    /// Parse a dbus-pump water topic: N/<portal>/tank/<i>/Level or
    /// N/<portal>/pump/<i>/State -> Some((kind, instance)).
    fn parse_water_topic(topic: &str) -> Option<(&str, u32)> {
        let rest = topic.strip_prefix("N/")?;
        let mut it = rest.split('/');
        let _portal = it.next()?;
        let kind = it.next()?;
        let inst: u32 = it.next()?.parse().ok()?;
        match (kind, it.next()?) {
            ("tank", "Level") | ("pump", "State") => Some((kind, inst)),
            _ => None,
        }
    }

    /// Parse dbus-ev / dbus-evcharger topic:
    /// N/<portal>/ev/<i>/Soc, N/<portal>/ev/<i>/Ac/Power,
    /// N/<portal>/evcharger/<i>/Ac/Power -> Some((kind, instance, path)).
    fn parse_ev_topic(topic: &str) -> Option<(&str, u32, &str)> {
        // "N/portal/ev/22/Soc"        -> 5 parts: portal, ev, 22, Soc
        // "N/portal/ev/22/Ac/Power"   -> 6 parts: portal, ev, 22, Ac, Power
        // (the slash inside "Ac/Power" is part of the path, not a separator).
        let rest = topic.strip_prefix("N/")?;
        let mut it = rest.splitn(6, '/');
        let _portal = it.next()?;
        let kind = it.next()?;
        let inst: u32 = it.next()?.parse().ok()?;
        let p1 = it.next()?;
        let path = match it.next() {
            Some(p2) if p1 == "Ac" && p2 == "Power" => "Ac/Power",
            Some(_) => return None,
            None => p1,
        };
        match (kind, path) {
            ("ev", "Soc")
            | ("ev", "Ac/Power")
            | ("evcharger", "Soc")
            | ("evcharger", "Ac/Power") => Some((kind, inst, path)),
            _ => None,
        }
    }

    /// Parse a Victron acload topic:
    /// N/<portal>/acload/<instance>/Ac/Power|CustomName|ProductName
    /// -> Some((instance, path)).
    fn parse_acload_topic(topic: &str) -> Option<(u32, &str)> {
        let rest = topic.strip_prefix("N/")?;
        let mut it = rest.splitn(6, '/');
        let _portal = it.next()?;
        if it.next()? != "acload" {
            return None;
        }
        let inst: u32 = it.next()?.parse().ok()?;
        let p1 = it.next()?;
        let path = match it.next() {
            Some(p2) if p1 == "Ac" && p2 == "Power" => "Ac/Power",
            Some(_) => return None,
            None => p1,
        };
        match path {
            "Ac/Power" | "CustomName" | "ProductName" => Some((inst, path)),
            _ => None,
        }
    }

    /// Apply one acload MQTT message into CerboDevices.acloads.
    /// Returns true when the message mapped to a known path.
    fn apply_acload_message(
        devices: &mut CerboDevices,
        inst: u32,
        path: &str,
        payload: &str,
    ) -> bool {
        let entry = devices.acloads.entry(inst).or_default();
        entry.touch();
        let a = &mut entry.data;
        match path {
            "Ac/Power" => {
                a.power = Self::parse_cerbo_value(payload);
                true
            }
            "CustomName" => {
                if let Some(n) = Self::parse_cerbo_name(payload) {
                    a.custom_name = Some(n);
                }
                true
            }
            "ProductName" => {
                // CustomName wins; only fill product when custom is empty.
                if let Some(n) = Self::parse_cerbo_name(payload) {
                    a.product_name = Some(n);
                }
                true
            }
            _ => false,
        }
    }

    /// Cerbo flashmq JSON envelope: {"value": <number>}.
    fn parse_cerbo_value(payload: &str) -> Option<f64> {
        serde_json::from_str::<serde_json::Value>(payload)
            .ok()?
            .get("value")?
            .as_f64()
    }

    /// /TimeToGo arrives in seconds (null when idle -> parse_cerbo_value
    /// already yields None). Format like the inverter-control daemon does.
    fn format_time_to_go(secs: f64) -> Option<String> {
        let s = secs as u64;
        if s == 0 || s >= 86_400 * 14 {
            return None;
        }
        let h = s / 3600;
        let m = (s % 3600) / 60;
        Some(if h > 0 {
            format!("{h}h {m:02}m")
        } else {
            format!("{m}m")
        })
    }

    /// Charging/Discharging/Idle from current sign, ±0.5 A deadband —
    /// mirrors inverter_control's _battery_state.
    fn state_from_current(amps: f64) -> String {
        if amps > 0.5 {
            "Charging".to_string()
        } else if amps < -0.5 {
            "Discharging".to_string()
        } else {
            "Idle".to_string()
        }
    }

    /// Cerbo ProductName arrives as {"value": "<name>"}.
    fn parse_cerbo_name(payload: &str) -> Option<String> {
        let s = serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|v| v.get("value").and_then(|x| x.as_str()).map(String::from))
            .unwrap_or_else(|| payload.trim().to_string());
        (!s.is_empty()).then_some(s)
    }

    /// Parse N/<portal>/<kind>/<instance>/<path> for GX devices we discover
    /// ourselves (battery, solarcharger, pvinverter, vebus). Other kinds are left to
    /// their own handlers (tank/pump) or ignored.
    fn parse_device_topic(topic: &str) -> Option<(&str, u32, &str)> {
        let rest = topic.strip_prefix("N/")?;
        let (portal, rest) = rest.split_once('/')?;
        if portal.is_empty() {
            return None;
        }
        let (kind, rest) = rest.split_once('/')?;
        if kind != "battery"
            && kind != "solarcharger"
            && kind != "pvinverter"
            && kind != "vebus"
            && kind != "system"
        {
            return None;
        }
        let (inst, path) = rest.split_once('/')?;
        Some((kind, inst.parse().ok()?, path))
    }

    /// Apply one GX device message to the discovered-device maps.
    /// Returns true when the message mapped to a known value path.
    fn apply_device_message(
        devices: &mut CerboDevices,
        kind: &str,
        inst: u32,
        path: &str,
        payload: &str,
    ) -> bool {
        let val = Self::parse_cerbo_value(payload);
        match kind {
            "battery" => {
                let entry = devices.batteries.entry(inst).or_default();
                entry.touch();
                let b = &mut entry.data;
                match path {
                    "Soc" => b.soc = val,
                    "Dc/0/Voltage" => b.voltage = val,
                    // The GX MQTT bridge publishes no battery /State — derive
                    // it from current sign (±0.5 A, same as the daemon).
                    "Dc/0/Current" => {
                        b.current = val;
                        if let Some(a) = val {
                            b.state = Some(Self::state_from_current(a));
                        }
                    }
                    "Dc/0/Power" => b.power = val,
                    "ProductName" => b.name = Self::parse_cerbo_name(payload),
                    "Serial" => b.serial = Self::parse_cerbo_name(payload),
                    "TimeToGo" => b.time_to_go = val.and_then(Self::format_time_to_go),
                    _ => return false,
                }
                true
            }
            "solarcharger" => {
                let entry = devices.chargers.entry(inst).or_default();
                entry.touch();
                let m = &mut entry.data;
                match path {
                    "Pv/V" => m.pv_voltage = val,
                    "Dc/0/Current" => m.current = val,
                    "Yield/Power" => m.power = val,
                    "ProductName" => m.name = Self::parse_cerbo_name(payload),
                    "Serial" => m.serial = Self::parse_cerbo_name(payload),
                    _ => return false,
                }
                true
            }
            "pvinverter" => {
                let entry = devices.pv_inverters.entry(inst).or_default();
                entry.touch();
                let p = &mut entry.data;
                // Store instance on first discovery so the UI can distinguish
                // devices with identical names across different Cerbo instances.
                if p.instance.is_none() {
                    p.instance = Some(inst);
                }
                match path {
                    // Ac/Power is the device total; L1 Power equals it on
                    // single-phase units but is accepted as a fallback.
                    "Ac/Power" | "Ac/L1/Power" | "Ac/L2/Power" => p.power = val,
                    "Ac/L1/Voltage" | "Ac/L2/Voltage" => p.voltage = val,
                    "Ac/L1/Current" | "Ac/L2/Current" => p.current = val,
                    "ProductName" => p.name = Self::parse_cerbo_name(payload),
                    "Serial" => p.serial = Self::parse_cerbo_name(payload),
                    _ => return false,
                }
                true
            }
            "vebus" => {
                let entry = devices.vebus.entry(inst).or_default();
                entry.touch();
                let v = &mut entry.data;
                match path {
                    "Ac/L1/Power" => v.l1_power = val,
                    "Ac/L2/Power" => v.l2_power = val,
                    "Ac/ActiveIn/L1/Power" => v.l1_power = val, // fallback grid-in
                    "Ac/ActiveIn/L2/Power" => v.l2_power = val,
                    "Ac/Out/P" | "Ac/Power" => v.ac_power = val,
                    "Hub4/L1/AcPowerSetpoint" => v.setpoint = val,
                    "State" => {
                        if let Some(code) = val {
                            v.inverter_state = Some(inverter_state_name(code as u32));
                        }
                    }
                    _ => return false,
                }
                true
            }
            "system" => {
                let entry = devices.system.entry(inst).or_default();
                entry.touch();
                let s = &mut entry.data;
                match path {
                    "Ac/Grid/L1/Power" => s.g1 = val,
                    "Ac/Grid/L2/Power" => s.g2 = val,
                    "Ac/Consumption/L1/Power" => s.t1 = val,
                    "Ac/Consumption/L2/Power" => s.t2 = val,
                    _ => return false,
                }
                true
            }
            _ => false,
        }
    }

    /// Apply a dbus-ev / dbus-evcharger message to state.
    /// Returns true if the state was modified.
    /// Each side matches its own configured instance; either may be None.
    /// Apply a dbus-ev / dbus-evcharger message to state.
    /// Updates `ev_cache` (throttled, 8 s per field) and state simultaneously.
    /// Returns the field that was updated, or None if no state change occurred.
    fn apply_ev_message(
        st: &mut InverterState,
        cache: &mut EvCache,
        kind: &str,
        inst: u32,
        path: &str,
        value: f64,
        ev_instances: &Option<(Option<u32>, Option<u32>)>,
    ) -> Option<EvField> {
        let ev_i = ev_instances.as_ref().and_then(|(e, _)| *e);
        let evc_i = ev_instances.as_ref().and_then(|(_, c)| *c);
        let matched = match kind {
            "ev" => ev_i == Some(inst),
            "evcharger" => evc_i == Some(inst),
            _ => false,
        };
        if !matched {
            return None;
        }
        // Mark presence so the EV tile stays visible even when SOC/power are 0.
        cache.set_presence(kind);
        // When cache.update returns false (TTL or 0-clobber), still re-apply the
        // cached value to st so process_state_update's clone doesn't see None.
        match (kind, path) {
            ("ev", "Soc") => {
                if cache.update(EvField::CarSoc, value) {
                    st.car_soc = Some(value);
                    Some(EvField::CarSoc)
                } else if let Some((v, _)) = cache.car_soc {
                    st.car_soc = Some(v);
                    Some(EvField::CarSoc)
                } else {
                    None
                }
            }
            ("ev", "Ac/Power") => {
                if cache.update(EvField::CarChargingPower, value) {
                    st.car_charging_power = Some(value);
                    Some(EvField::CarChargingPower)
                } else if let Some((v, _)) = cache.car_charging_power {
                    st.car_charging_power = Some(v);
                    Some(EvField::CarChargingPower)
                } else {
                    None
                }
            }
            ("evcharger", "Ac/Power") => {
                if cache.update(EvField::EvChargingPower, value) {
                    st.ev_charging_power = Some(value);
                    Some(EvField::EvChargingPower)
                } else if let Some((v, _)) = cache.ev_charging_power {
                    st.ev_charging_power = Some(v);
                    Some(EvField::EvChargingPower)
                } else {
                    None
                }
            }
            ("evcharger", "Soc") => {
                // dbus-ev originally published Soc under com.victronenergy.evcharger;
                // some GX installs still use that name after the .ev rename.
                if cache.update(EvField::CarSoc, value) {
                    st.car_soc = Some(value);
                    Some(EvField::CarSoc)
                } else if let Some((v, _)) = cache.car_soc {
                    st.car_soc = Some(v);
                    Some(EvField::CarSoc)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// The SmartShunt is the ground-truth meter for the whole battery bank:
    /// the mqtt chains report per-string BMS views and virtual_chain is
    /// derived from the shunt itself (shunt - chain1 - chain2), so summing
    /// every battery service double-counts. The shunt's D-Bus/MQTT instance
    /// can change across GX reboots, so match by product name, not instance.
    fn find_shunt(batteries: &[Battery]) -> Option<&Battery> {
        batteries.iter().find(|b| {
            b.name
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains("shunt")
        })
    }

    /// Identical units ship one shared ProductName ("SmartSolar Charger MPPT
    /// 100/20 48V" x N), which reads as the same tile repeated. Suffix every
    /// duplicate with its serial tail (or broker instance when no serial is
    /// known) so each tile stays distinguishable; unique names pass through.
    fn disambiguate_names<T: DeviceIdentity>(items: &mut [T], instances: &[u32]) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for it in items.iter() {
            if let Some(n) = it.display_name() {
                *counts.entry(n.to_string()).or_insert(0) += 1;
            }
        }
        for (it, inst) in items.iter_mut().zip(instances) {
            let Some(name) = it.display_name().map(String::from) else {
                continue;
            };
            if counts.get(&name).copied().unwrap_or(1) <= 1 {
                continue;
            }
            let tail = match it.serial() {
                Some(s) if s.len() >= 4 && s.is_char_boundary(s.len() - 4) => {
                    s[s.len() - 4..].to_string()
                }
                _ => format!("#{}", inst),
            };
            *it.name_slot() = Some(format!("{} · {}", name, tail));
        }
    }

    /// Overlay discovered GX devices onto a state snapshot. When anything was
    /// found on the broker it wins over the daemon-provided arrays so the UI
    /// stays correct even with inverter-control down; empty maps leave the
    /// daemon data untouched.
    fn apply_cerbo_to_state(devices: &CerboDevices, st: &mut InverterState) {
        if !devices.batteries.is_empty() {
            let mut batteries: Vec<Battery> =
                devices.batteries.values().map(|e| e.data.clone()).collect();
            // Time-to-go is only meaningful while charging/discharging; hide
            // the stale value otherwise (daemon does the same gating).
            for b in &mut batteries {
                if !matches!(b.state.as_deref(), Some("Charging") | Some("Discharging")) {
                    b.time_to_go = None;
                }
            }
            // Bank totals come from the shunt alone; without it, leave the
            // daemon's system-aggregate values untouched rather than summing
            // overlapping battery services.
            if let Some(shunt) = Self::find_shunt(&batteries) {
                // Bank % is computed from pack voltage (HA "Battery %" paradigm):
                // the shunt's own SoC counter reads a bogus 100% while charging.
                // Computed here, not in the daemon, so it works with inverter-control down.
                st.battery_soc = shunt.voltage.map(voltage_soc).or(st.battery_soc);
                st.battery_voltage = shunt.voltage.or(st.battery_voltage);
                st.battery_current = Some(shunt.current.unwrap_or(0.0));
                st.battery_power = Some(shunt.power.unwrap_or(0.0));
            }
            Self::disambiguate_names(
                &mut batteries,
                &devices.batteries.keys().copied().collect::<Vec<_>>(),
            );
            st.batteries = Some(batteries);
        }
        if !devices.chargers.is_empty() {
            let mut chargers: Vec<MpptCharger> =
                devices.chargers.values().map(|e| e.data.clone()).collect();
            Self::disambiguate_names(
                &mut chargers,
                &devices.chargers.keys().copied().collect::<Vec<_>>(),
            );
            st.mppt_total = Some(devices.chargers.values().filter_map(|e| e.data.power).sum());
            st.mppt_chargers = Some(chargers);
        }
        if !devices.pv_inverters.is_empty() {
            let mut pv_inverters: Vec<PvInverter> = devices
                .pv_inverters
                .values()
                .map(|e| e.data.clone())
                .collect();
            Self::disambiguate_names(
                &mut pv_inverters,
                &devices.pv_inverters.keys().copied().collect::<Vec<_>>(),
            );
            st.pv_inverters = Some(pv_inverters);
        }
        // Chart/stat "solar total": prefer Cerbo maps; if only one side is on
        // Cerbo, keep the other side from existing state (daemon fallback).
        if devices.owns_solar() {
            let mppt = if devices.owns_chargers() {
                devices.chargers.values().filter_map(|e| e.data.power).sum()
            } else {
                st.mppt_total
                    .or_else(|| st.mppt_individual.as_ref().map(|v| v.iter().sum()))
                    .unwrap_or(0.0)
            };
            let pv = if devices.owns_pv() {
                devices
                    .pv_inverters
                    .values()
                    .filter_map(|e| e.data.power)
                    .sum()
            } else if let Some(ref invs) = st.pv_inverters {
                invs.iter().filter_map(|p| p.power).sum()
            } else {
                st.pv_inverter_individual
                    .as_ref()
                    .map(|v| v.iter().sum())
                    .unwrap_or(0.0)
            };
            st.solar_total = Some(mppt + pv);
        }
        // Prefer systemcalc grid/consumption (same paths as inverter-control).
        if let Some(entry) = devices.system.values().next() {
            let s = &entry.data;
            if let Some(g1) = s.g1 {
                st.g1 = Some(g1);
            }
            if let Some(g2) = s.g2 {
                st.g2 = Some(g2);
            }
            if let Some(t1) = s.t1 {
                st.t1 = Some(t1);
            }
            if let Some(t2) = s.t2 {
                st.t2 = Some(t2);
            }
            if let (Some(g1), Some(g2)) = (st.g1, st.g2) {
                st.gt = Some(g1 + g2);
            }
            match (st.t1, st.t2) {
                (Some(t1), Some(t2)) => st.tt = Some(t1 + t2),
                (Some(t1), None) => st.tt = Some(t1),
                (None, Some(t2)) => st.tt = Some(t2),
                _ => {}
            }
        }

        if let Some(entry) = devices.vebus.values().next() {
            let v = &entry.data;
            // Grid from vebus only when systemcalc has not filled it.
            if st.g1.is_none() {
                if let Some(l1) = v.l1_power {
                    st.g1 = Some(l1);
                }
            }
            if st.g2.is_none() {
                if let Some(l2) = v.l2_power {
                    st.g2 = Some(l2);
                }
            }
            if st.gt.is_none() {
                if let (Some(g1), Some(g2)) = (st.g1, st.g2) {
                    st.gt = Some(g1 + g2);
                } else if let Some(ac_power) = v.ac_power {
                    st.gt = Some(ac_power);
                }
            }
            // Live ESS setpoint + charger mode — chart/tile even without daemon.
            if let Some(sp) = v.setpoint {
                st.setpoint = Some(sp);
            }
            if let Some(ref mode) = v.inverter_state {
                st.inverter_state = Some(mode.clone());
            }
        }
        if !devices.acloads.is_empty() {
            // Stable instance-id keys for watts; names live in load_names so
            // power updates never rekey the map back to bare ids.
            let mut loads = std::collections::HashMap::new();
            let mut names = std::collections::HashMap::new();
            for (inst, entry) in &devices.acloads {
                let id = inst.to_string();
                if let Some(p) = entry.data.power {
                    loads.insert(id.clone(), p);
                }
                if let Some(n) = entry.data.display_name() {
                    names.insert(id, n.to_string());
                }
            }
            st.loads = Some(loads);
            // Preserve previously known names for instances that briefly lose
            // CustomName publishes; only overwrite/extend, never wipe known names
            // when the overlay has a partial name set.
            let dest = st.load_names.get_or_insert_with(Default::default);
            for (id, n) in names {
                dest.insert(id, n);
            }
        }
    }

    /// Track a Victron alarm topic (N/<portal>/<service>/Alarms/<Name>, value 0/1/2)
    /// and emit banner notifications on transitions. Value 0 clears the banner.
    fn handle_alarm_message(
        topic: &str,
        payload: &str,
        alarms: &Arc<Mutex<HashMap<String, u8>>>,
        app_handle: &Option<tauri::AppHandle>,
    ) {
        let value = serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|v| v.get("value").and_then(|x| x.as_u64()))
            .unwrap_or(0) as u8;

        let prev = {
            let mut map = match alarms.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let entry = map.entry(topic.to_string()).or_insert(0);
            let prev = *entry;
            *entry = value;
            prev
        };

        if prev == value {
            return;
        }

        let parts: Vec<&str> = topic.split('/').collect();
        // N/<portal>/<service_type>_<instance>/Alarms/<AlarmName>
        let alarm_name = parts.get(4).copied().unwrap_or(topic);
        let id = format!("victron-{}", topic);

        if let Some(ref handle) = app_handle {
            if value == 1 || value == 2 {
                let level = if value == 2 { "alarm" } else { "warning" };
                let state_txt = if value == 2 { "Alarm" } else { "Warning" };
                // Extract portal ID for more informative messages (topic format: N/<portal>/<service>/Alarms/<alarm_name>)
                let portal_id = parts.get(1).copied().unwrap_or("unknown");
                let _ = handle.emit(
                    "mqtt-notification",
                    MqttNotification {
                        id,
                        level: level.to_string(),
                        title: pretty_alarm_name(alarm_name), // Show specific alarm name as title
                        body: format!(
                            "{} on {}: {}",
                            pretty_alarm_name(alarm_name),
                            portal_id,
                            state_txt
                        ),
                        source: "victron".to_string(),
                        ts: Utc::now().to_rfc3339(), // Add timestamp
                    },
                );
            } else {
                let _ = handle.emit("mqtt-notification-clear", serde_json::json!({ "id": id }));
            }
        }
    }

    /// Display name for a load in notifications: HA friendly name when available,
    /// otherwise the raw load key.
    fn load_display_name(
        load: &str,
        ha_entity_states: &Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
    ) -> String {
        if let Some(states) = ha_entity_states {
            if let Ok(guard) = states.lock() {
                if let Some(name) = load_friendly_name(load, &guard) {
                    return name;
                }
            }
        }
        load.to_string()
    }

    fn process_state_update(
        raw: RawInverterState,
        state: Arc<Mutex<InverterState>>,
        app_handle: Option<tauri::AppHandle>,
        notifications: Arc<Mutex<NotificationState>>,
        ha_entity_states: Option<Arc<Mutex<HashMap<String, HaEntityEntry>>>>,
        cerbo_devices: Option<Arc<Mutex<CerboDevices>>>,
        ev_cache: Arc<Mutex<EvCache>>,
    ) {
        // Non-destructive merge: start from existing state, update only fields
        // present in the incoming payload. Prevents transient `None` values
        // from wiping valid data during partial MQTT messages.
        let mut new_state = {
            let guard = state.lock().unwrap_or_else(|e| e.into_inner());
            guard.clone()
        };

        // Merge numeric scalars — keep existing if incoming is None
        macro_rules! merge_opt {
            ($field:ident, $val:expr) => {
                if $val.is_some() {
                    new_state.$field = $val;
                }
            };
        }

        // Snapshot which Cerbo maps already own live tiles so daemon
        // zeros/partials cannot clobber them (same pattern as EV / shunt).
        let cerbo_flags = cerbo_devices.as_ref().and_then(|c| {
            c.lock().ok().map(|d| {
                (
                    d.has_shunt(),
                    d.owns_grid(),
                    d.owns_consumption(),
                    d.owns_vebus_mode(),
                    d.owns_chargers(),
                    d.owns_pv(),
                    d.owns_batteries(),
                    d.owns_solar(),
                    !d.acloads.is_empty(),
                )
            })
        });
        let (
            cerbo_has_shunt,
            cerbo_owns_grid,
            cerbo_owns_consumption,
            cerbo_owns_vebus_mode,
            cerbo_owns_chargers,
            cerbo_owns_pv,
            cerbo_owns_batteries,
            cerbo_owns_solar,
            cerbo_has_acloads,
        ) = cerbo_flags.unwrap_or((
            false, false, false, false, false, false, false, false, false,
        ));

        // Grid / consumption / solar: Cerbo systemcalc + chargers/PV first.
        if !cerbo_owns_grid {
            merge_opt!(gt, raw.gt);
            merge_opt!(g1, raw.g1);
            merge_opt!(g2, raw.g2);
        }
        if !cerbo_owns_consumption {
            merge_opt!(tt, raw.tt);
            merge_opt!(t1, raw.t1);
            merge_opt!(t2, raw.t2);
        }
        if !cerbo_owns_solar {
            merge_opt!(solar_total, raw.solar_total);
        }
        // Battery bank totals: Cerbo shunt owns them (same pattern as EV —
        // do not let daemon inverter/state overwrite shunt-derived W/V/A/%).
        // Fallback to daemon only when no Cerbo shunt has been discovered yet.
        if !cerbo_has_shunt {
            merge_opt!(battery_soc, raw.battery_soc);
            merge_opt!(battery_power, raw.battery_power);
            merge_opt!(battery_voltage, raw.battery_voltage);
            merge_opt!(battery_current, raw.battery_current);
        }
        if !cerbo_owns_vebus_mode {
            merge_opt!(setpoint, raw.setpoint);
            merge_opt!(inverter_state, raw.inverter_state);
        }
        merge_opt!(version, raw.version);
        merge_opt!(dashboard_version, raw.dashboard_version);
        merge_opt!(uptime, raw.uptime);
        merge_opt!(ha_connected, raw.ha_connected);
        merge_opt!(ha_direct_connected, raw.ha_direct_connected);
        merge_opt!(ess_mode, raw.ess_mode);
        if !cerbo_owns_chargers {
            merge_opt!(mppt_individual, raw.mppt_individual);
        }
        merge_opt!(ui_config, raw.ui_config);
        merge_opt!(daily_stats, raw.daily_stats);
        merge_opt!(solar_forecast, raw.solar_forecast);
        // EV + water come ONLY from Cerbo MQTT handlers (apply_ev_message /
        // tank+pump). Do NOT merge daemon values — they overwrite live tiles.
        // Washer/dryer/dishwasher: UI reads HA entities only — skip daemon.
        merge_opt!(latest_version, raw.latest_version);

        // Bool coercions — keep existing if incoming is None
        if let Some(ref v) = raw.dry_run {
            new_state.dry_run = Some(coerce_bool(v));
        }

        // Map coercions
        if let Some(map) = raw.booleans {
            new_state.booleans = Some(map.into_iter().map(|(k, v)| (k, coerce_bool(&v))).collect());
        }
        merge_opt!(features, raw.features);

        // Collection fields — Cerbo device maps win when discovered.
        if !cerbo_owns_chargers {
            merge_opt!(mppt_chargers, raw.mppt_chargers);
        }
        if !cerbo_owns_pv {
            merge_opt!(pv_inverters, raw.pv_inverters);
            merge_opt!(pv_inverter_individual, raw.pv_inverter_individual);
        }
        if !cerbo_owns_batteries {
            merge_opt!(batteries, raw.batteries);
        }
        // Active loads: Cerbo acload services own the map (instance-keyed +
        // load_names). Daemon loads are often name-keyed and would replace the
        // Cerbo map on every inverter/state → UI flickers ids ↔ names.
        if !cerbo_has_acloads {
            merge_opt!(loads, raw.loads);
        }

        // mppt_total from daemon mppt_individual only when Cerbo has no chargers.
        // Otherwise apply_cerbo_to_state owns mppt_total / solar_total.
        if !cerbo_owns_chargers {
            new_state.mppt_total = new_state.mppt_individual.as_ref().map(|v| v.iter().sum());
        }

        // Console: append new lines, cap at max
        if let Some(new_lines) = raw.console {
            let console = new_state.console.get_or_insert_with(Vec::new);
            console.extend(new_lines);
            if console.len() > CONSOLE_MAX_LINES {
                let drain = console.len() - CONSOLE_MAX_LINES;
                console.drain(..drain);
            }
        }

        // GX-discovered devices win over daemon arrays (see
        // apply_cerbo_to_state) so batteries/MPPTs survive daemon outages.
        if let Some(cerbo) = cerbo_devices.as_ref() {
            if let Ok(d) = cerbo.lock() {
                Self::apply_cerbo_to_state(&d, &mut new_state);
            }
        }

        // Skip alert/notification processing when window hidden (CPU/battery optimization)
        let hidden = crate::ha_api::WINDOW_HIDDEN.load(std::sync::atomic::Ordering::Relaxed);

        if !hidden {
            let mut alert_notifications: Vec<(String, String)> = Vec::new();
            if let Ok(mut alert_state) = notifications.lock() {
                let mut active_loads = std::collections::HashSet::new();
                if let Some(ref loads) = new_state.loads {
                    for (name, power) in loads {
                        if *power > THRESHOLD_LOAD_W {
                            active_loads.insert(name.clone());
                            let alert = alert_state
                                .high_load
                                .entry(name.clone())
                                .or_insert_with(AlertState::new);
                            if alert.should_alert() {
                                let display_name = new_state
                                    .load_names
                                    .as_ref()
                                    .and_then(|m| m.get(name).cloned())
                                    .unwrap_or_else(|| {
                                        Self::load_display_name(name, &ha_entity_states)
                                    });
                                let title = "High Load".to_string();
                                let body = format!("{}: {}", display_name, fmt_watts(*power));
                                alert_notifications.push((title.clone(), body.clone()));
                                // Also send as persistent banner
                                if let Some(ref handle) = app_handle {
                                    let alert_id = "high-load".to_string();
                                    let _ = handle.emit(
                                        "mqtt-notification",
                                        MqttNotification {
                                            id: alert_id,
                                            level: "alarm".to_string(),
                                            title: title.clone(),
                                            body: body.clone(),
                                            source: "system".to_string(),
                                            ts: Utc::now().to_rfc3339(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                    alert_state
                        .high_load
                        .retain(|name, _| active_loads.contains(name));
                }

                if let Some(tt) = new_state.tt {
                    if tt > THRESHOLD_CONSUMPTION_W {
                        if alert_state.high_consumption.should_alert() {
                            let title = "High Consumption".to_string();
                            let body = format!("Consumption: {}", fmt_watts(tt));
                            alert_notifications.push((title.clone(), body.clone()));
                            // Also send as persistent banner
                            if let Some(ref handle) = app_handle {
                                let alert_id = "high-consumption".to_string();
                                let _ = handle.emit(
                                    "mqtt-notification",
                                    MqttNotification {
                                        id: alert_id,
                                        level: "alarm".to_string(),
                                        title: title.clone(),
                                        body: body.clone(),
                                        source: "system".to_string(),
                                        ts: Utc::now().to_rfc3339(),
                                    },
                                );
                            }
                        }
                    } else {
                        alert_state.high_consumption.check_resolved();
                    }
                }
                if let Some(wl) = new_state.water_level {
                    if wl < THRESHOLD_WATER_CM {
                        if alert_state.low_water.should_alert_value(wl) {
                            let title = "Low Water".to_string();
                            let body = format!("Water level: {} cm", wl);
                            alert_notifications.push((title.clone(), body.clone()));
                            // Also send as persistent banner
                            if let Some(ref handle) = app_handle {
                                let alert_id = "low-water".to_string();
                                let _ = handle.emit(
                                    "mqtt-notification",
                                    MqttNotification {
                                        id: alert_id,
                                        level: "alarm".to_string(),
                                        title: title.clone(),
                                        body: body.clone(),
                                        source: "system".to_string(),
                                        ts: Utc::now().to_rfc3339(),
                                    },
                                );
                            }
                        }
                    } else {
                        alert_state.low_water.check_resolved();
                    }
                }
                if let Some(st) = new_state.solar_total {
                    if st > THRESHOLD_SOLAR_W {
                        if alert_state.high_solar.should_alert() {
                            let title = "High Solar".to_string();
                            let body = format!("Solar: {}", fmt_watts(st));
                            alert_notifications.push((title.clone(), body.clone()));
                            // Also send as persistent banner
                            if let Some(ref handle) = app_handle {
                                let alert_id = "high-solar".to_string();
                                let _ = handle.emit(
                                    "mqtt-notification",
                                    MqttNotification {
                                        id: alert_id,
                                        level: "alarm".to_string(),
                                        title: title.clone(),
                                        body: body.clone(),
                                        source: "system".to_string(),
                                        ts: Utc::now().to_rfc3339(),
                                    },
                                );
                            }
                        }
                    } else {
                        alert_state.high_solar.check_resolved();
                    }
                }
            }

            if let Some(ref handle) = app_handle {
                for (title, body) in &alert_notifications {
                    let _ = handle
                        .notification()
                        .builder()
                        .title(title)
                        .body(body)
                        .show();
                    let _ = handle.emit(
                        "notification",
                        serde_json::json!({ "title": title, "body": body }),
                    );
                }
            }
        }

        // Restore EV fields/presence BEFORE emitting so the UI never sees
        // the pre-restore state (no flash/blink on HA poll interval).
        if let Ok(cache) = ev_cache.lock() {
            cache.restore_into(&mut new_state);
        }
        // restore_into ran above BEFORE emit so the emitted snapshot already
        // carries cached EV values (prevents the blink where the clone from
        // before apply_ev_message lands sees null EV numbers).
        // Persist under the mutex *before* emit so concurrent Cerbo handlers
        // (acload/device/EV) merge onto the latest daemon state.
        if let Ok(mut guard) = state.lock() {
            *guard = new_state.clone();
        }
        Self::emit_state_update(&app_handle, &new_state, false);
    }

    /// Returns the current value of an inverter-control flag (true=on, false=off),
    /// or None when the flag has not been published yet.
    pub fn flag_state(&self, key: &str) -> Option<bool> {
        let state = self.state.lock().ok()?;
        let bools = state.booleans.as_ref()?;
        bools.get(key).copied()
    }

    pub fn publish_command(
        &self,
        action: &str,
        payload: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guard = self
            .client
            .lock()
            .map_err(|e| format!("Internal error: {}", e))?;
        let client = guard.as_ref().ok_or("MQTT client not connected")?;
        let topic = format!("inverter/cmd/{}", action);
        let payload_str = if payload.is_null() {
            String::new()
        } else {
            serde_json::to_string(&payload)?
        };
        client.publish(topic, QoS::AtLeastOnce, false, payload_str)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(friendly_name: &str) -> HaEntityEntry {
        HaEntityEntry {
            state: "on".to_string(),
            attributes: Some(serde_json::json!({ "friendly_name": friendly_name })),
        }
    }

    fn states() -> HashMap<String, HaEntityEntry> {
        let mut map = HashMap::new();
        map.insert("sensor.stove_power".to_string(), entry("Stove Power"));
        map.insert("switch.stove".to_string(), entry("Stove"));
        map.insert("sensor.washer_power_estimate".to_string(), entry("Washer"));
        map.insert("binary_sensor.dryer_running".to_string(), entry("Dryer"));
        map.insert("switch.shutoff_valve".to_string(), entry("Shutoff Valve"));
        map
    }

    #[test]
    fn raw_inverter_state_accepts_short_battery_aliases() {
        let mut raw: RawInverterState =
            serde_json::from_str(r#"{"gt":1.0,"bp":1200.0,"bv":52.4,"bc":-23.1}"#).expect("parse");
        raw.resolve_short_battery_keys();
        assert_eq!(raw.battery_power, Some(1200.0));
        assert_eq!(raw.battery_voltage, Some(52.4));
        assert_eq!(raw.battery_current, Some(-23.1));
    }

    /// Regression: daemon JSON includes BOTH canonical and short battery keys.
    /// serde `alias` treated them as one field and rejected the whole payload
    /// ("duplicate field `battery_power`"), zeroing Consumption/Setpoint.
    #[test]
    fn raw_inverter_state_accepts_canonical_and_short_battery_keys_together() {
        let json = r#"{
            "gt": 100.0,
            "tt": 2500.0,
            "setpoint": -500.0,
            "battery_power": 1800.5,
            "bp": 999.0,
            "battery_voltage": 53.2,
            "bv": 40.0,
            "battery_current": -12.5,
            "bc": 0.0,
            "battery_soc": 88.0
        }"#;
        let mut raw: RawInverterState = serde_json::from_str(json).expect("parse both keys");
        raw.resolve_short_battery_keys();
        // Prefer canonical when both present.
        assert_eq!(raw.battery_power, Some(1800.5));
        assert_eq!(raw.battery_voltage, Some(53.2));
        assert_eq!(raw.battery_current, Some(-12.5));
        assert_eq!(raw.gt, Some(100.0));
        assert_eq!(raw.tt, Some(2500.0));
        assert_eq!(raw.setpoint, Some(-500.0));
        assert_eq!(raw.battery_soc, Some(88.0));
    }

    #[test]
    fn system_consumption_and_vebus_setpoint_overlay_state() {
        let mut d = CerboDevices::default();
        assert!(MqttClient::apply_device_message(
            &mut d,
            "system",
            0,
            "Ac/Consumption/L1/Power",
            r#"{"value": 1200.0}"#
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "system",
            0,
            "Ac/Consumption/L2/Power",
            r#"{"value": 800.0}"#
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "system",
            0,
            "Ac/Grid/L1/Power",
            r#"{"value": -50.0}"#
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "system",
            0,
            "Ac/Grid/L2/Power",
            r#"{"value": 20.0}"#
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "vebus",
            276,
            "Hub4/L1/AcPowerSetpoint",
            r#"{"value": -615.0}"#
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "vebus",
            276,
            "State",
            r#"{"value": 3}"#
        ));

        let mut st = InverterState::default();
        // Daemon zeros must not win over Cerbo overlay.
        st.tt = Some(0.0);
        st.setpoint = Some(0.0);
        MqttClient::apply_cerbo_to_state(&d, &mut st);
        assert_eq!(st.t1, Some(1200.0));
        assert_eq!(st.t2, Some(800.0));
        assert_eq!(st.tt, Some(2000.0));
        assert_eq!(st.g1, Some(-50.0));
        assert_eq!(st.g2, Some(20.0));
        assert_eq!(st.gt, Some(-30.0));
        assert_eq!(st.setpoint, Some(-615.0));
        assert_eq!(st.inverter_state.as_deref(), Some("Bulk"));
    }

    /// Guardrail: portal subscribe burst must fit the rumqttc request channel
    /// even if someone reverts subscribe_many back to per-filter subscribe.
    /// Regression: 3512a15 added acload as the 11th filter while capacity was
    /// 10, deadlocking the MQTT thread inside the portal discovery handler.
    #[test]
    fn portal_topic_filter_count_fits_mqtt_queue_capacity() {
        // Keep in sync with subscribe_portal_topics filter list.
        const PORTAL_FILTER_COUNT: usize = 13;
        assert!(
            PORTAL_FILTER_COUNT < MQTT_QUEUE_CAPACITY,
            "portal filters ({PORTAL_FILTER_COUNT}) must leave headroom in              MQTT_QUEUE_CAPACITY ({MQTT_QUEUE_CAPACITY}) for concurrent              publishes/subscribes from inside connection.iter()"
        );
    }

    #[test]
    fn resolves_friendly_name_from_entity_id() {
        let map = states();
        assert_eq!(
            load_friendly_name("sensor.washer_power_estimate", &map).as_deref(),
            Some("Washer")
        );
    }

    #[test]
    fn resolves_friendly_name_from_bare_load_key() {
        let map = states();
        assert_eq!(load_friendly_name("stove", &map).as_deref(), Some("Stove"));
        assert_eq!(load_friendly_name("dryer", &map).as_deref(), Some("Dryer"));
    }

    #[test]
    fn prefers_most_specific_matching_entity() {
        let map = states();
        // Both switch.stove (Stove) and sensor.stove_power (Stove Power) match "stove";
        // switch.stove is the shorter/more specific entity id.
        assert_eq!(load_friendly_name("stove", &map).as_deref(), Some("Stove"));
    }

    #[test]
    fn returns_none_when_no_match() {
        let map = states();
        assert_eq!(load_friendly_name("no_such_load", &map), None);
    }

    #[test]
    fn parses_tank_level_topic() {
        assert_eq!(
            MqttClient::parse_water_topic("N/abc123/tank/21/Level"),
            Some(("tank", 21))
        );
    }

    #[test]
    fn parses_pump_state_topic() {
        assert_eq!(
            MqttClient::parse_water_topic("N/abc123/pump/2/State"),
            Some(("pump", 2))
        );
    }

    #[test]
    fn rejects_other_paths_and_services() {
        assert_eq!(MqttClient::parse_water_topic("N/abc/tank/21/Voltage"), None);
        assert_eq!(
            MqttClient::parse_water_topic("N/abc/solarcharger/0/State"),
            None
        );
        assert_eq!(MqttClient::parse_water_topic("N/abc/pump/x/State"), None);
    }

    #[test]
    fn parses_ev_soc_topic() {
        assert_eq!(
            MqttClient::parse_ev_topic("N/portal/ev/22/Soc"),
            Some(("ev", 22, "Soc"))
        );
    }

    #[test]
    fn parses_ev_power_topic() {
        assert_eq!(
            MqttClient::parse_ev_topic("N/portal/ev/22/Ac/Power"),
            Some(("ev", 22, "Ac/Power"))
        );
    }

    #[test]
    fn parses_evcharger_power_topic() {
        assert_eq!(
            MqttClient::parse_ev_topic("N/portal/evcharger/40/Ac/Power"),
            Some(("evcharger", 40, "Ac/Power"))
        );
    }

    #[test]
    fn apply_ev_message_pops_car_soc() {
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));
        assert!(MqttClient::apply_ev_message(
            &mut st, &mut cache, "ev", 22, "Soc", 66.0, &instances
        )
        .is_some());
        assert_eq!(st.car_soc, Some(66.0));
    }

    #[test]
    fn apply_ev_message_pops_evcharger_power() {
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));
        assert!(MqttClient::apply_ev_message(
            &mut st,
            &mut cache,
            "evcharger",
            40,
            "Ac/Power",
            7400.0,
            &instances
        )
        .is_some());
        assert_eq!(st.ev_charging_power, Some(7400.0));
    }

    #[test]
    fn apply_ev_message_ignores_wrong_instance() {
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));
        assert_eq!(
            MqttClient::apply_ev_message(&mut st, &mut cache, "ev", 99, "Soc", 66.0, &instances),
            None
        );
        assert!(st.car_soc.is_none());
        assert_eq!(
            MqttClient::apply_ev_message(
                &mut st,
                &mut cache,
                "evcharger",
                99,
                "Ac/Power",
                7400.0,
                &instances
            ),
            None
        );
        assert!(st.ev_charging_power.is_none());
    }

    #[test]
    fn apply_ev_message_partial_instances_ev_only() {
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        // ev_instance set, evcharger_instance absent
        let instances = Some((Some(22), None));
        assert!(MqttClient::apply_ev_message(
            &mut st, &mut cache, "ev", 22, "Soc", 55.0, &instances
        )
        .is_some());
        assert_eq!(st.car_soc, Some(55.0));
        // evcharger message must be ignored
        assert_eq!(
            MqttClient::apply_ev_message(
                &mut st,
                &mut cache,
                "evcharger",
                40,
                "Ac/Power",
                7400.0,
                &instances
            ),
            None
        );
        assert!(st.ev_charging_power.is_none());
    }

    #[test]
    fn apply_ev_message_sets_presence_for_ev() {
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));
        // Presence is set on the cache after apply_ev_message
        MqttClient::apply_ev_message(&mut st, &mut cache, "ev", 22, "Soc", 0.0, &instances);
        assert!(
            cache.ev_present,
            "cache.ev_present should be true after apply_ev_message with 0"
        );
        assert_eq!(st.car_soc, Some(0.0));
        // After restore_into, st.ev_present mirrors cache
        cache.restore_into(&mut st);
        assert!(
            st.ev_present,
            "st.ev_present should be true after restore_into"
        );
    }

    #[test]
    fn apply_ev_message_sets_presence_for_evcharger() {
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));
        MqttClient::apply_ev_message(
            &mut st,
            &mut cache,
            "evcharger",
            40,
            "Ac/Power",
            0.0,
            &instances,
        );
        assert!(
            cache.evcharger_present,
            "cache.evcharger_present should be true after apply_ev_message with 0"
        );
        assert_eq!(st.ev_charging_power, Some(0.0));
        // After restore_into, st.evcharger_present mirrors cache
        cache.restore_into(&mut st);
        assert!(
            st.evcharger_present,
            "st.evcharger_present should be true after restore_into"
        );
    }

    #[test]
    fn apply_ev_message_partial_instances_evcharger_only() {
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        // evcharger_instance set, ev_instance absent
        let instances = Some((None, Some(40)));
        assert!(MqttClient::apply_ev_message(
            &mut st,
            &mut cache,
            "evcharger",
            40,
            "Ac/Power",
            5500.0,
            &instances
        )
        .is_some());
        assert_eq!(st.ev_charging_power, Some(5500.0));
        // ev message must be ignored
        assert_eq!(
            MqttClient::apply_ev_message(&mut st, &mut cache, "ev", 22, "Soc", 55.0, &instances),
            None
        );
        assert!(st.car_soc.is_none());
    }

    #[test]
    fn apply_ev_message_no_instances_ignores_all() {
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances: Option<(Option<u32>, Option<u32>)> = None;
        assert_eq!(
            MqttClient::apply_ev_message(&mut st, &mut cache, "ev", 22, "Soc", 66.0, &instances),
            None
        );
        assert_eq!(
            MqttClient::apply_ev_message(
                &mut st,
                &mut cache,
                "evcharger",
                40,
                "Ac/Power",
                7400.0,
                &instances
            ),
            None
        );
        assert!(st.car_soc.is_none());
        assert!(st.ev_charging_power.is_none());
    }

    #[test]
    fn apply_ev_message_evcharger_soc_populates_car_soc() {
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));
        // dbus-ev may publish Soc under the evcharger bus name on installs
        // where the .ev rename hasn't reached the GX yet.
        assert!(MqttClient::apply_ev_message(
            &mut st,
            &mut cache,
            "evcharger",
            40,
            "Soc",
            72.5,
            &instances
        )
        .is_some());
        assert_eq!(st.car_soc, Some(72.5));
    }

    #[test]
    fn apply_ev_message_partial_default_when_serialized_old_config() {
        // Simulates the #302 regression for users whose saved config predates
        // the EV instance fields: serde defaults populate them to (22, 40).
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));
        assert!(MqttClient::apply_ev_message(
            &mut st, &mut cache, "ev", 22, "Soc", 66.0, &instances
        )
        .is_some());
        assert_eq!(st.car_soc, Some(66.0));
        assert!(MqttClient::apply_ev_message(
            &mut st,
            &mut cache,
            "evcharger",
            40,
            "Ac/Power",
            7400.0,
            &instances
        )
        .is_some());
        assert_eq!(st.ev_charging_power, Some(7400.0));
    }

    #[test]
    fn apply_ev_message_both_none_drops_all_messages() {
        // Belt-and-braces: if the auto-connect path ever regresses to pass
        // Some((None, None)) again, EVERY ev and evcharger message must drop.
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances: Option<(Option<u32>, Option<u32>)> = Some((None, None));
        assert_eq!(
            MqttClient::apply_ev_message(&mut st, &mut cache, "ev", 22, "Soc", 66.0, &instances),
            None
        );
        assert_eq!(
            MqttClient::apply_ev_message(
                &mut st,
                &mut cache,
                "evcharger",
                40,
                "Ac/Power",
                7400.0,
                &instances
            ),
            None
        );
        assert!(st.car_soc.is_none());
        assert!(st.ev_charging_power.is_none());
    }

    #[test]
    fn rejects_ev_other_paths() {
        assert_eq!(MqttClient::parse_ev_topic("N/portal/ev/22/Status"), None);
        assert_eq!(MqttClient::parse_ev_topic("N/portal/ev/22/Current"), None);
        assert_eq!(MqttClient::parse_ev_topic("N/portal/ev/22/Energy"), None);
        assert_eq!(MqttClient::parse_ev_topic("N/portal/ev/x/Ac/Power"), None);
        // dbus-ev publishes Soc under the evcharger bus name, so this is valid.
        assert_eq!(
            MqttClient::parse_ev_topic("N/portal/evcharger/40/Soc"),
            Some(("evcharger", 40, "Soc"))
        );
        assert_eq!(
            MqttClient::parse_ev_topic("N/portal/evcharger/40/Status"),
            None
        );
    }

    #[test]
    fn parses_cerbo_envelope() {
        assert_eq!(
            MqttClient::parse_cerbo_value("{\"value\": 66.0}"),
            Some(66.0)
        );
        assert_eq!(MqttClient::parse_cerbo_value("{\"value\": null}"), None);
        assert_eq!(MqttClient::parse_cerbo_value("not json"), None);
    }

    #[test]
    fn parses_device_topics() {
        assert_eq!(
            MqttClient::parse_device_topic("N/abc/battery/512/Soc"),
            Some(("battery", 512, "Soc"))
        );
        assert_eq!(
            MqttClient::parse_device_topic("N/abc/solarcharger/1/Dc/0/Current"),
            Some(("solarcharger", 1, "Dc/0/Current"))
        );
        assert_eq!(
            MqttClient::parse_device_topic("N/abc/pvinverter/369/Ac/L1/Voltage"),
            Some(("pvinverter", 369, "Ac/L1/Voltage"))
        );
        // Other kinds route elsewhere or are ignored.
        assert_eq!(MqttClient::parse_device_topic("N/abc/tank/21/Level"), None);
        assert_eq!(
            MqttClient::parse_device_topic("N/abc/vebus/256/Soc"),
            Some(("vebus", 256, "Soc"))
        );
        assert_eq!(MqttClient::parse_device_topic("N/abc/battery/x/Soc"), None);
    }

    #[test]
    fn discovers_battery_and_charger_from_gx() {
        let mut d = CerboDevices::default();
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "Soc",
            "{\"value\": 87.5}"
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "Dc/0/Voltage",
            "{\"value\": 51.2}"
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "ProductName",
            "{\"value\": \"SmartShunt 500A\"}"
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "Dc/0/Current",
            "{\"value\": 12.5}"
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "TimeToGo",
            "{\"value\": 108110.0}"
        ));
        // Idle battery (0 A within deadband): stale time-to-go must be hidden
        // even if a value was seen earlier.
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            513,
            "Dc/0/Current",
            "{\"value\": 0.1}"
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            513,
            "TimeToGo",
            "{\"value\": 7200.0}"
        ));
        // Discharging derivation from negative current.
        assert_eq!(MqttClient::state_from_current(-3.5), "Discharging");
        assert!(MqttClient::apply_device_message(
            &mut d,
            "solarcharger",
            2,
            "Yield/Power",
            "{\"value\": 1450}"
        ));
        // AC PV inverter of any vendor: V/I/P from the GX bridge paths.
        assert!(MqttClient::apply_device_message(
            &mut d,
            "pvinverter",
            369,
            "Ac/L1/Voltage",
            "{\"value\": 126.0}"
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "pvinverter",
            369,
            "Ac/L1/Current",
            "{\"value\": 1.29}"
        ));
        assert!(MqttClient::apply_device_message(
            &mut d,
            "pvinverter",
            369,
            "Ac/Power",
            "{\"value\": 163}"
        ));
        assert!(!MqttClient::apply_device_message(
            &mut d,
            "pvinverter",
            369,
            "StatusCode",
            "{\"value\": 0}"
        ));
        // Unknown path ignored, alarms payload not a number.
        assert!(!MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "Alarms/HighVoltage",
            "{\"value\": 1}"
        ));

        let b = &d.batteries[&512].data;
        assert_eq!(b.soc, Some(87.5));
        assert_eq!(b.voltage, Some(51.2));
        assert_eq!(b.name.as_deref(), Some("SmartShunt 500A"));
        assert_eq!(b.state.as_deref(), Some("Charging"));
        assert_eq!(b.time_to_go.as_deref(), Some("30h 01m"));
        assert_eq!(d.batteries[&513].data.state.as_deref(), Some("Idle"));
        // Raw value still stored; the Idle gate happens at overlay time.
        assert_eq!(d.batteries[&513].data.time_to_go.as_deref(), Some("2h 00m"));

        let mut st = InverterState::default();
        MqttClient::apply_cerbo_to_state(&d, &mut st);
        // Bank % is voltage-derived (HA paradigm), NOT the shunt's SoC counter
        // which reads bogus 100% while charging: ((51.2-40)/14.4)*100 -> 78.
        assert_eq!(st.battery_soc, Some(voltage_soc(51.2)));
        assert_eq!(st.battery_soc, Some(78.0));
        assert_eq!(st.batteries.as_ref().unwrap().len(), 2);
        assert_eq!(st.mppt_chargers.as_ref().unwrap().len(), 1);
        assert_eq!(st.mppt_total, Some(1450.0));
        // Discovered PV inverter surfaces with V/I/P; the legacy aggregate
        // mirrors its power so older UIs keep working.
        assert_eq!(st.pv_inverters.as_ref().unwrap().len(), 1);
        let inv = &st.pv_inverters.as_ref().unwrap()[0];
        assert_eq!(inv.voltage, Some(126.0));
        assert_eq!(inv.current, Some(1.29));
        assert_eq!(inv.power, Some(163.0));

        // Shunt SoC counter itself is untouched in the per-device tile list.
        let bats = st.batteries.clone().unwrap();
        assert_eq!(bats[0].soc, Some(87.5));
        // Idle battery keeps its state but loses the stale time-to-go.
        assert_eq!(bats[1].state.as_deref(), Some("Idle"));
        assert_eq!(bats[1].time_to_go, None);
    }

    /// Per-device TTL: entries survive sweep when recently seen, are evicted
    /// after the TTL window, and updating one entry doesn't affect others.
    #[test]
    fn cerbo_sweep_evicts_ghost_devices() {
        let mut d = CerboDevices::default();
        assert!(MqttClient::apply_device_message(
            &mut d,
            "solarcharger",
            9,
            "Yield/Power",
            "{\"value\": 5}"
        ));
        // Entry just created — survives sweep (last_seen < TTL).
        d.sweep_stale();
        assert_eq!(d.chargers.len(), 1);

        // Second entry: updating one device doesn't evict the other.
        assert!(MqttClient::apply_device_message(
            &mut d,
            "solarcharger",
            10,
            "Yield/Power",
            "{\"value\": 8}"
        ));
        d.sweep_stale();
        assert_eq!(d.chargers.len(), 2);

        // Both entries are fresh — neither is evicted.
        assert!(d.chargers.contains_key(&9));
        assert!(d.chargers.contains_key(&10));
    }

    #[test]
    fn cerbo_devices_override_daemon_but_empty_map_leaves_daemon_data() {
        let mut st = InverterState {
            batteries: Some(vec![Battery {
                name: Some("daemon".into()),
                soc: Some(10.0),
                ..Default::default()
            }]),
            mppt_chargers: None,
            battery_soc: Some(10.0),
            ..Default::default()
        };

        // Empty discovery → daemon data untouched.
        let empty = CerboDevices::default();
        MqttClient::apply_cerbo_to_state(&empty, &mut st);
        assert_eq!(
            st.batteries.as_ref().unwrap()[0].name.as_deref(),
            Some("daemon")
        );
        assert_eq!(st.battery_soc, Some(10.0));

        // Discovered non-shunt devices refresh the battery list but must NOT
        // become bank totals (summing overlapping services double-counts);
        // daemon totals stay untouched until a shunt shows up.
        let mut d = CerboDevices::default();
        MqttClient::apply_device_message(&mut d, "battery", 1, "Soc", "{\"value\": 90}");
        MqttClient::apply_device_message(&mut d, "battery", 1, "Dc/0/Current", "{\"value\": -3.5}");
        MqttClient::apply_cerbo_to_state(&d, &mut st);
        let bats = st.batteries.clone().unwrap();
        assert_eq!(bats.len(), 1);
        assert_eq!(bats[0].soc, Some(90.0));
        assert_eq!(st.battery_soc, Some(10.0));
        assert_eq!(st.battery_current, None);

        // Shunt-named device wins all bank totals.
        MqttClient::apply_device_message(
            &mut d,
            "battery",
            2,
            "ProductName",
            "\"SmartShunt 500A/50mV\"",
        );
        MqttClient::apply_device_message(&mut d, "battery", 2, "Soc", "{\"value\": 87}");
        MqttClient::apply_device_message(&mut d, "battery", 2, "Dc/0/Voltage", "{\"value\": 53.2}");
        MqttClient::apply_device_message(&mut d, "battery", 2, "Dc/0/Power", "{\"value\": 672}");
        MqttClient::apply_cerbo_to_state(&d, &mut st);
        // Voltage-derived % wins over the shunt's SoC counter (87 here).
        assert_eq!(st.battery_soc, Some(voltage_soc(53.2)));
        assert_eq!(st.battery_soc, Some(92.0));
        assert_eq!(st.battery_power, Some(672.0));

        // Missing voltage keeps the previous bank % instead of blanking it.
        let mut d_no_v = CerboDevices::default();
        MqttClient::apply_device_message(
            &mut d_no_v,
            "battery",
            3,
            "ProductName",
            "\"SmartShunt 500A/50mV\"",
        );
        MqttClient::apply_device_message(&mut d_no_v, "battery", 3, "Soc", "{\"value\": 100}");
        MqttClient::apply_cerbo_to_state(&d_no_v, &mut st);
        assert_eq!(st.battery_soc, Some(92.0));
    }

    #[test]
    fn voltage_soc_matches_ha_battery_percent() {
        // Parity with the HA template: linear 40-54.4 V, clamp 0-100, round.
        assert_eq!(voltage_soc(40.0), 0.0);
        assert_eq!(voltage_soc(54.4), 100.0);
        assert_eq!(voltage_soc(47.2), 50.0);
        assert_eq!(voltage_soc(30.0), 0.0); // below range clamps to 0
        assert_eq!(voltage_soc(60.0), 100.0); // above range clamps to 100
        assert_eq!(voltage_soc(51.2), 78.0); // whole numbers only
    }

    #[test]
    fn falls_back_when_friendly_name_missing() {
        let mut map = HashMap::new();
        map.insert(
            "sensor.plain".to_string(),
            HaEntityEntry {
                state: "on".to_string(),
                attributes: None,
            },
        );
        assert_eq!(load_friendly_name("plain", &map), None);
    }

    #[test]
    fn partial_device_serializes_without_nulls() {
        // The UI guards optional battery fields with `!== undefined`; a JSON
        // `null` passes that check and crashes rendering (null.toFixed).
        // Partially-discovered GX devices must omit the fields instead.
        assert_eq!(serde_json::to_string(&Battery::default()).unwrap(), "{}");
        assert_eq!(
            serde_json::to_string(&MpptCharger::default()).unwrap(),
            "{}"
        );
        assert_eq!(serde_json::to_string(&PvInverter::default()).unwrap(), "{}");
        // Fully-populated values still serialize.
        let b = Battery {
            soc: Some(87.5),
            ..Default::default()
        };
        assert_eq!(serde_json::to_string(&b).unwrap(), r#"{"soc":87.5}"#);
    }

    fn bat(name: &str, amps: f64) -> Battery {
        Battery {
            name: Some(name.to_string()),
            current: Some(amps),
            ..Default::default()
        }
    }

    #[test]
    fn finds_shunt_by_product_name_regardless_of_instance() {
        // Instance numbers change across GX reboots; the product name is stable.
        let batteries = vec![
            bat("JBD Chain 1", 4.0),
            bat("Virtual Battery", 4.4),
            bat("SmartShunt 500A/50mV", 12.9),
        ];
        assert_eq!(
            MqttClient::find_shunt(&batteries).unwrap().current,
            Some(12.9)
        );
    }

    #[test]
    fn no_shunt_when_only_chains_present() {
        let batteries = vec![bat("JBD Chain 1", 4.0), bat("JBD Chain 2", 4.3)];
        assert!(MqttClient::find_shunt(&batteries).is_none());
    }

    /// Partial topic streams: only some fields arrive per message. Existing
    /// fields must be preserved when a new message updates a different field.
    #[test]
    fn partial_device_messages_preserve_existing_fields() {
        let mut d = CerboDevices::default();
        // First message: only SoC arrives.
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "Soc",
            "{\"value\": 85.0}"
        ));
        assert_eq!(d.batteries[&512].data.soc, Some(85.0));
        assert_eq!(d.batteries[&512].data.voltage, None);

        // Second message: only voltage arrives — SoC must survive.
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "Dc/0/Voltage",
            "{\"value\": 52.1}"
        ));
        assert_eq!(d.batteries[&512].data.soc, Some(85.0));
        assert_eq!(d.batteries[&512].data.voltage, Some(52.1));

        // Third message: power arrives — both SoC and voltage survive.
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "Dc/0/Power",
            "{\"value\": -1200}"
        ));
        assert_eq!(d.batteries[&512].data.soc, Some(85.0));
        assert_eq!(d.batteries[&512].data.voltage, Some(52.1));
        assert_eq!(d.batteries[&512].data.power, Some(-1200.0));

        // Name arrives later — all numeric fields still intact.
        assert!(MqttClient::apply_device_message(
            &mut d,
            "battery",
            512,
            "ProductName",
            "{\"value\": \"SmartShunt 500A\"}"
        ));
        assert_eq!(
            d.batteries[&512].data.name.as_deref(),
            Some("SmartShunt 500A")
        );
        assert_eq!(d.batteries[&512].data.soc, Some(85.0));
        assert_eq!(d.batteries[&512].data.voltage, Some(52.1));
        assert_eq!(d.batteries[&512].data.power, Some(-1200.0));
    }

    /// Shunt bank totals survive when the shunt message is delayed.
    /// The cerbo overlay should keep existing bank values when the shunt
    /// is momentarily absent from the device map.
    #[test]
    fn shunt_bank_totals_retained_when_shunt_delayed() {
        let mut st = InverterState::default();

        // Phase 1: shunt present — sets bank totals.
        let mut d = CerboDevices::default();
        MqttClient::apply_device_message(
            &mut d,
            "battery",
            1,
            "ProductName",
            "\"SmartShunt 500A/50mV\"",
        );
        MqttClient::apply_device_message(&mut d, "battery", 1, "Dc/0/Voltage", "{\"value\": 53.0}");
        MqttClient::apply_device_message(&mut d, "battery", 1, "Dc/0/Current", "{\"value\": -5.0}");
        MqttClient::apply_device_message(&mut d, "battery", 1, "Dc/0/Power", "{\"value\": -265}");
        MqttClient::apply_cerbo_to_state(&d, &mut st);
        assert_eq!(st.battery_soc, Some(voltage_soc(53.0)));
        assert_eq!(st.battery_voltage, Some(53.0));
        assert_eq!(st.battery_current, Some(-5.0));
        assert_eq!(st.battery_power, Some(-265.0));

        // Phase 2: shunt missing (delayed / MQTT gap) — only a non-shunt battery.
        let mut d2 = CerboDevices::default();
        MqttClient::apply_device_message(&mut d2, "battery", 2, "Soc", "{\"value\": 90}");
        MqttClient::apply_cerbo_to_state(&d2, &mut st);
        // Bank totals from the shunt must survive.
        assert_eq!(st.battery_soc, Some(voltage_soc(53.0)));
        assert_eq!(st.battery_voltage, Some(53.0));
        assert_eq!(st.battery_power, Some(-265.0));

        // Phase 3: shunt returns with updated values.
        let mut d3 = CerboDevices::default();
        MqttClient::apply_device_message(
            &mut d3,
            "battery",
            1,
            "ProductName",
            "\"SmartShunt 500A/50mV\"",
        );
        MqttClient::apply_device_message(
            &mut d3,
            "battery",
            1,
            "Dc/0/Voltage",
            "{\"value\": 52.5}",
        );
        MqttClient::apply_device_message(&mut d3, "battery", 1, "Dc/0/Power", "{\"value\": -300}");
        MqttClient::apply_cerbo_to_state(&d3, &mut st);
        // Updated shunt values replace the old ones.
        assert_eq!(st.battery_soc, Some(voltage_soc(52.5)));
        assert_eq!(st.battery_voltage, Some(52.5));
        assert_eq!(st.battery_power, Some(-300.0));
    }

    #[test]
    fn flag_state_returns_current_value() {
        let client = MqttClient::new("localhost".into(), 1883, None, None, "test".into());
        // No booleans yet → None.
        assert_eq!(client.flag_state("only_charging"), None);
        {
            let mut st = client.state.lock().unwrap();
            st.booleans = Some(HashMap::from([("only_charging".into(), true)]));
        }
        assert_eq!(client.flag_state("only_charging"), Some(true));
        {
            let mut st = client.state.lock().unwrap();
            st.booleans = Some(HashMap::from([("only_charging".into(), false)]));
        }
        assert_eq!(client.flag_state("only_charging"), Some(false));
        assert_eq!(client.flag_state("never_set"), None);
    }

    #[test]
    fn publish_command_errors_when_client_is_none() {
        // Regression: clicks used to silently return Ok(()) when the client
        // was dropped during reconnect, leaving the UI with no feedback.
        let client = MqttClient::new("localhost".into(), 1883, None, None, "test".into());
        // self.client is None — connect() is the only thing that fills it.
        let err = client
            .publish_command("toggle", serde_json::json!({"entity": "only_charging"}))
            .unwrap_err();
        assert!(err.to_string().contains("not connected"));
    }

    #[test]
    fn publish_command_does_not_error_when_slot_is_some() {
        // After connect() builds the rumqtt Client it stores a clone in the Arc slot.
        // publish_command must be able to use it.  Test the slot-populated path by
        // directly filling the Arc slot with a dummy Client (no broker needed).
        let client = MqttClient::new("localhost".into(), 1883, None, None, "test".into());

        // Build a real Client so we exercise the Arc slot path (not the None path).
        let mqttoptions = rumqttc::MqttOptions::new("test-publish", ("localhost", 1883));
        let (dummy, _connection) = rumqttc::Client::builder(mqttoptions).build();
        {
            let mut slot = client.client.lock().unwrap();
            *slot = Some(dummy);
        }
        // Slot is Some — publish_command must not return "not connected".
        // (try_publish on a disconnected client queues the request in the channel
        // and returns Ok; the real broker connection is the EventLoop's job. We
        // only care that publish_command did not bail out before reaching the
        // client because the slot was None.)
        let result =
            client.publish_command("toggle", serde_json::json!({"entity": "only_charging"}));
        match result {
            Ok(()) => {}
            Err(e) => assert!(
                !e.to_string().contains("not connected"),
                "expected a real publish error, not 'not connected': {e}"
            ),
        }
    }

    /// Per-device TTL: updating one device must not evict other active entries.
    #[test]
    fn per_device_ttl_does_not_evict_active_peers() {
        let mut d = CerboDevices::default();
        // Create two devices.
        MqttClient::apply_device_message(&mut d, "battery", 1, "Soc", "{\"value\": 80}");
        MqttClient::apply_device_message(&mut d, "battery", 2, "Soc", "{\"value\": 90}");
        assert_eq!(d.batteries.len(), 2);

        // Update only device 1 — device 2 must survive the sweep.
        MqttClient::apply_device_message(&mut d, "battery", 1, "Soc", "{\"value\": 81}");
        d.sweep_stale();
        assert_eq!(d.batteries.len(), 2);
        assert_eq!(d.batteries[&1].data.soc, Some(81.0));
        assert_eq!(d.batteries[&2].data.soc, Some(90.0));
    }

    // -------------------------------------------------------------------------
    // EV cache integration tests
    // -------------------------------------------------------------------------

    #[test]
    fn ev_cache_throttles_same_field_within_window() {
        // Apply once → cache populated. Second call within 8 s → rejected by TTL.
        // But apply_ev_message still copies the cached value onto st and returns
        // Some (so process_state_update sees consistent state).
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));

        let mut st = InverterState::default();
        assert!(MqttClient::apply_ev_message(
            &mut st, &mut cache, "ev", 22, "Soc", 66.0, &instances
        )
        .is_some());
        assert_eq!(st.car_soc, Some(66.0));

        // Second call within the 8 s throttle window → cache not updated, but
        // cached value is re-applied to st and Some is returned.
        let mut st2 = InverterState::default();
        assert!(MqttClient::apply_ev_message(
            &mut st2, &mut cache, "ev", 22, "Soc", 70.0, &instances
        )
        .is_some());
        assert_eq!(st2.car_soc, Some(66.0)); // cached value preserved
    }

    #[test]
    fn ev_cache_restores_after_process_state_update() {
        // Simulate the race: apply_ev_message sets EV fields, then
        // process_state_update clones from before that and overwrites them.
        // The cache must restore the EV values after write-back.
        let state = Arc::new(Mutex::new(InverterState::default()));
        let ev_cache = Arc::new(Mutex::new(EvCache::default()));
        let cerbo_devices: Arc<Mutex<CerboDevices>> = Arc::new(Mutex::new(CerboDevices::default()));

        // Apply EV sample — populates both state and cache.
        {
            let mut guard = state.lock().unwrap();
            let mut cache = ev_cache.lock().unwrap();
            let instances = Some((Some(22), Some(40)));
            MqttClient::apply_ev_message(&mut guard, &mut cache, "ev", 22, "Soc", 66.0, &instances);
            MqttClient::apply_ev_message(
                &mut guard, &mut cache, "ev", 22, "Ac/Power", 3200.0, &instances,
            );
            MqttClient::apply_ev_message(
                &mut guard,
                &mut cache,
                "evcharger",
                40,
                "Ac/Power",
                7400.0,
                &instances,
            );
        }

        // process_state_update with a RawInverterState that has no EV fields
        // (simulates the inverter/state payload missing EV data). The clone-
        // and-merge starts from the pre-apply snapshot, so EV fields would be
        // wiped without cache restoration.
        let raw = RawInverterState {
            gt: Some(500.0),
            ..Default::default()
        };
        MqttClient::process_state_update(
            raw,
            state.clone(),
            None,
            Arc::new(Mutex::new(NotificationState {
                high_consumption: AlertState::new(),
                low_water: AlertState::new(),
                high_solar: AlertState::new(),
                high_load: std::collections::HashMap::new(),
            })),
            None,
            Some(cerbo_devices),
            ev_cache.clone(),
        );

        // EV fields AND presence bits must survive the daemon's merge.
        let guard = state.lock().unwrap();
        assert_eq!(guard.car_soc, Some(66.0));
        assert_eq!(guard.car_charging_power, Some(3200.0));
        assert_eq!(guard.ev_charging_power, Some(7400.0));
        assert!(guard.ev_present, "ev_present must survive daemon merge");
        assert!(
            guard.evcharger_present,
            "evcharger_present must survive daemon merge"
        );
    }

    #[test]
    fn ev_cache_zero_power_still_shows_section() {
        // Zero power is a legitimate idle value. The section should stay visible
        // via presence bits (set on cache, restored into state after daemon merge).
        let mut st = InverterState::default();
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));

        MqttClient::apply_ev_message(&mut st, &mut cache, "ev", 22, "Ac/Power", 0.0, &instances);
        assert!(cache.ev_present, "section visible via cache.ev_present");
        assert_eq!(st.car_charging_power, Some(0.0));

        MqttClient::apply_ev_message(
            &mut st,
            &mut cache,
            "evcharger",
            40,
            "Ac/Power",
            0.0,
            &instances,
        );
        assert!(
            cache.evcharger_present,
            "section visible via cache.evcharger_present"
        );
        assert_eq!(st.ev_charging_power, Some(0.0));

        // restore_into propagates presence to state
        let mut st2 = InverterState::default();
        cache.restore_into(&mut st2);
        assert!(st2.ev_present);
        assert!(st2.evcharger_present);
    }

    #[test]
    fn ev_cache_zero_soc_does_not_clobber_real_soc() {
        // inverter-control publishes car_soc=0 when no car is connected.
        // The cache must refuse to overwrite a real SoC with 0.
        let mut cache = EvCache::default();
        let instances = Some((Some(22), Some(40)));

        // First: real car SoC from Cerbo.
        let mut st = InverterState::default();
        assert!(MqttClient::apply_ev_message(
            &mut st, &mut cache, "ev", 22, "Soc", 66.0, &instances,
        )
        .is_some());
        assert_eq!(st.car_soc, Some(66.0));

        // Second: daemon publishes 0 → cache refuses update (preserves real).
        // apply_ev_message still copies cached value onto st2 and returns Some.
        let mut st2 = InverterState::default();
        assert_eq!(
            MqttClient::apply_ev_message(&mut st2, &mut cache, "ev", 22, "Soc", 0.0, &instances,),
            Some(EvField::CarSoc),
        );
        assert_eq!(st2.car_soc, Some(66.0)); // cached value preserved, not clobbered
        assert_eq!(st.car_soc, Some(66.0));
    }

    // -------------------------------------------------------------------------
    // Active loads (Cerbo acload) — stable instance keys + name cache
    // -------------------------------------------------------------------------

    #[test]
    fn parses_acload_power_and_name_topics() {
        assert_eq!(
            MqttClient::parse_acload_topic("N/portal/acload/81/Ac/Power"),
            Some((81, "Ac/Power"))
        );
        assert_eq!(
            MqttClient::parse_acload_topic("N/portal/acload/88/CustomName"),
            Some((88, "CustomName"))
        );
        assert_eq!(
            MqttClient::parse_acload_topic("N/portal/acload/88/ProductName"),
            Some((88, "ProductName"))
        );
        assert_eq!(
            MqttClient::parse_acload_topic("N/portal/acload/88/Ac/Energy/Forward"),
            None
        );
    }

    #[test]
    fn acload_custom_name_preferred_over_product_and_survives_power_update() {
        let mut d = CerboDevices::default();
        assert!(MqttClient::apply_acload_message(
            &mut d,
            81,
            "Ac/Power",
            "{\"value\": 420}"
        ));
        assert!(MqttClient::apply_acload_message(
            &mut d,
            81,
            "ProductName",
            "{\"value\": \"AC Load\"}"
        ));
        assert!(MqttClient::apply_acload_message(
            &mut d,
            81,
            "CustomName",
            "{\"value\": \"Kitchen\"}"
        ));

        let mut st = InverterState::default();
        MqttClient::apply_cerbo_to_state(&d, &mut st);
        assert_eq!(st.loads.as_ref().unwrap().get("81"), Some(&420.0));
        assert_eq!(
            st.load_names
                .as_ref()
                .unwrap()
                .get("81")
                .map(String::as_str),
            Some("Kitchen")
        );

        // Later power tick must NOT rekey loads or drop the cached name.
        assert!(MqttClient::apply_acload_message(
            &mut d,
            81,
            "Ac/Power",
            "{\"value\": 455}"
        ));
        MqttClient::apply_cerbo_to_state(&d, &mut st);
        assert_eq!(st.loads.as_ref().unwrap().get("81"), Some(&455.0));
        assert_eq!(
            st.load_names
                .as_ref()
                .unwrap()
                .get("81")
                .map(String::as_str),
            Some("Kitchen"),
            "name must survive power-only updates (no id flicker)"
        );
        // Map must stay instance-keyed — never rename the watts key to "Kitchen".
        assert!(st.loads.as_ref().unwrap().get("Kitchen").is_none());
    }

    #[test]
    fn process_state_update_does_not_replace_cerbo_loads_with_daemon_map() {
        let state = Arc::new(Mutex::new(InverterState::default()));
        let ev_cache = Arc::new(Mutex::new(EvCache::default()));
        let cerbo_devices: Arc<Mutex<CerboDevices>> = Arc::new(Mutex::new(CerboDevices::default()));

        {
            let mut d = cerbo_devices.lock().unwrap();
            MqttClient::apply_acload_message(&mut d, 81, "Ac/Power", "{\"value\": 100}");
            MqttClient::apply_acload_message(&mut d, 81, "CustomName", "{\"value\": \"Oven\"}");
            let mut st = state.lock().unwrap();
            MqttClient::apply_cerbo_to_state(&d, &mut st);
        }

        // Daemon publishes name-keyed loads — classic flicker source.
        let mut daemon_loads = std::collections::HashMap::new();
        daemon_loads.insert("Oven".to_string(), 999.0);
        daemon_loads.insert("Dryer".to_string(), 50.0);
        let raw = RawInverterState {
            gt: Some(1.0),
            loads: Some(daemon_loads),
            ..Default::default()
        };
        MqttClient::process_state_update(
            raw,
            state.clone(),
            None,
            Arc::new(Mutex::new(NotificationState {
                high_consumption: AlertState::new(),
                low_water: AlertState::new(),
                high_solar: AlertState::new(),
                high_load: std::collections::HashMap::new(),
            })),
            None,
            Some(cerbo_devices),
            ev_cache,
        );

        let guard = state.lock().unwrap();
        assert_eq!(
            guard.loads.as_ref().unwrap().get("81"),
            Some(&100.0),
            "Cerbo instance-keyed watts must survive daemon loads merge"
        );
        assert!(
            guard.loads.as_ref().unwrap().get("Oven").is_none(),
            "daemon name-keyed map must not replace Cerbo loads"
        );
        assert_eq!(
            guard
                .load_names
                .as_ref()
                .unwrap()
                .get("81")
                .map(String::as_str),
            Some("Oven")
        );
    }

    #[test]
    fn process_state_update_skips_daemon_battery_when_cerbo_shunt_present() {
        let state = Arc::new(Mutex::new(InverterState::default()));
        let ev_cache = Arc::new(Mutex::new(EvCache::default()));
        let cerbo_devices: Arc<Mutex<CerboDevices>> = Arc::new(Mutex::new(CerboDevices::default()));

        {
            let mut d = cerbo_devices.lock().unwrap();
            MqttClient::apply_device_message(
                &mut d,
                "battery",
                2,
                "ProductName",
                "\"SmartShunt 500A/50mV\"",
            );
            MqttClient::apply_device_message(
                &mut d,
                "battery",
                2,
                "Dc/0/Voltage",
                "{\"value\": 52.0}",
            );
            MqttClient::apply_device_message(
                &mut d,
                "battery",
                2,
                "Dc/0/Power",
                "{\"value\": -265.0}",
            );
            MqttClient::apply_device_message(
                &mut d,
                "battery",
                2,
                "Dc/0/Current",
                "{\"value\": -5.1}",
            );
            let mut st = state.lock().unwrap();
            MqttClient::apply_cerbo_to_state(&d, &mut st);
        }

        let raw = RawInverterState {
            battery_power: Some(9999.0),
            battery_voltage: Some(40.0),
            battery_current: Some(0.0),
            battery_soc: Some(11.0),
            gt: Some(10.0),
            ..Default::default()
        };
        MqttClient::process_state_update(
            raw,
            state.clone(),
            None,
            Arc::new(Mutex::new(NotificationState {
                high_consumption: AlertState::new(),
                low_water: AlertState::new(),
                high_solar: AlertState::new(),
                high_load: std::collections::HashMap::new(),
            })),
            None,
            Some(cerbo_devices),
            ev_cache,
        );

        let guard = state.lock().unwrap();
        assert_eq!(guard.battery_power, Some(-265.0));
        assert_eq!(guard.battery_voltage, Some(52.0));
        assert_eq!(guard.battery_current, Some(-5.1));
        // Voltage-derived SoC, not daemon 11%.
        assert_eq!(guard.battery_soc, Some(voltage_soc(52.0)));
        // No Cerbo system/vebus yet → daemon gt still merges as fallback.
        assert_eq!(guard.gt, Some(10.0));
    }

    fn empty_notifications() -> Arc<Mutex<NotificationState>> {
        Arc::new(Mutex::new(NotificationState {
            high_consumption: AlertState::new(),
            low_water: AlertState::new(),
            high_solar: AlertState::new(),
            high_load: std::collections::HashMap::new(),
        }))
    }

    #[test]
    fn process_state_update_skips_daemon_grid_when_cerbo_system_present() {
        let state = Arc::new(Mutex::new(InverterState::default()));
        let ev_cache = Arc::new(Mutex::new(EvCache::default()));
        let cerbo_devices: Arc<Mutex<CerboDevices>> = Arc::new(Mutex::new(CerboDevices::default()));

        {
            let mut d = cerbo_devices.lock().unwrap();
            assert!(MqttClient::apply_device_message(
                &mut d,
                "system",
                0,
                "Ac/Grid/L1/Power",
                "{\"value\": 120.0}",
            ));
            assert!(MqttClient::apply_device_message(
                &mut d,
                "system",
                0,
                "Ac/Grid/L2/Power",
                "{\"value\": 80.0}",
            ));
            assert!(MqttClient::apply_device_message(
                &mut d,
                "system",
                0,
                "Ac/Consumption/L1/Power",
                "{\"value\": 400.0}",
            ));
            assert!(MqttClient::apply_device_message(
                &mut d,
                "system",
                0,
                "Ac/Consumption/L2/Power",
                "{\"value\": 100.0}",
            ));
            let mut st = state.lock().unwrap();
            MqttClient::apply_cerbo_to_state(&d, &mut st);
        }

        let raw = RawInverterState {
            gt: Some(0.0),
            g1: Some(0.0),
            g2: Some(0.0),
            tt: Some(0.0),
            t1: Some(0.0),
            t2: Some(0.0),
            version: Some("daemon".into()),
            ..Default::default()
        };
        MqttClient::process_state_update(
            raw,
            state.clone(),
            None,
            empty_notifications(),
            None,
            Some(cerbo_devices),
            ev_cache,
        );

        let guard = state.lock().unwrap();
        assert_eq!(guard.g1, Some(120.0));
        assert_eq!(guard.g2, Some(80.0));
        assert_eq!(guard.gt, Some(200.0));
        assert_eq!(guard.t1, Some(400.0));
        assert_eq!(guard.t2, Some(100.0));
        assert_eq!(guard.tt, Some(500.0));
        assert_eq!(guard.version.as_deref(), Some("daemon"));
    }

    #[test]
    fn process_state_update_skips_daemon_setpoint_when_cerbo_vebus_present() {
        let state = Arc::new(Mutex::new(InverterState::default()));
        let ev_cache = Arc::new(Mutex::new(EvCache::default()));
        let cerbo_devices: Arc<Mutex<CerboDevices>> = Arc::new(Mutex::new(CerboDevices::default()));

        {
            let mut d = cerbo_devices.lock().unwrap();
            assert!(MqttClient::apply_device_message(
                &mut d,
                "vebus",
                276,
                "Hub4/L1/AcPowerSetpoint",
                "{\"value\": -1500.0}",
            ));
            assert!(MqttClient::apply_device_message(
                &mut d,
                "vebus",
                276,
                "State",
                "{\"value\": 9}",
            ));
            let mut st = state.lock().unwrap();
            MqttClient::apply_cerbo_to_state(&d, &mut st);
        }

        let raw = RawInverterState {
            setpoint: Some(0.0),
            inverter_state: Some("Off".into()),
            ess_mode: Some(EssMode {
                mode_name: Some("Optimized".into()),
                is_external: Some(true),
            }),
            ..Default::default()
        };
        MqttClient::process_state_update(
            raw,
            state.clone(),
            None,
            empty_notifications(),
            None,
            Some(cerbo_devices),
            ev_cache,
        );

        let guard = state.lock().unwrap();
        assert_eq!(guard.setpoint, Some(-1500.0));
        assert_eq!(guard.inverter_state.as_deref(), Some("Inverting"));
        // ess_mode remains daemon-only
        assert_eq!(
            guard.ess_mode.as_ref().and_then(|m| m.mode_name.as_deref()),
            Some("Optimized")
        );
    }

    #[test]
    fn process_state_update_skips_daemon_solar_when_cerbo_chargers_present() {
        let state = Arc::new(Mutex::new(InverterState::default()));
        let ev_cache = Arc::new(Mutex::new(EvCache::default()));
        let cerbo_devices: Arc<Mutex<CerboDevices>> = Arc::new(Mutex::new(CerboDevices::default()));

        {
            let mut d = cerbo_devices.lock().unwrap();
            assert!(MqttClient::apply_device_message(
                &mut d,
                "solarcharger",
                1,
                "Yield/Power",
                "{\"value\": 700.0}",
            ));
            assert!(MqttClient::apply_device_message(
                &mut d,
                "solarcharger",
                1,
                "ProductName",
                "{\"value\": \"SmartSolar\"}",
            ));
            assert!(MqttClient::apply_device_message(
                &mut d,
                "pvinverter",
                20,
                "Ac/Power",
                "{\"value\": 300.0}",
            ));
            let mut st = state.lock().unwrap();
            MqttClient::apply_cerbo_to_state(&d, &mut st);
        }

        let raw = RawInverterState {
            solar_total: Some(1.0),
            mppt_individual: Some(vec![1.0, 2.0]),
            mppt_chargers: Some(vec![MpptCharger {
                name: Some("daemon-mppt".into()),
                power: Some(1.0),
                ..Default::default()
            }]),
            pv_inverters: Some(vec![PvInverter {
                name: Some("daemon-pv".into()),
                power: Some(1.0),
                ..Default::default()
            }]),
            pv_inverter_individual: Some(vec![9.0]),
            ..Default::default()
        };
        MqttClient::process_state_update(
            raw,
            state.clone(),
            None,
            empty_notifications(),
            None,
            Some(cerbo_devices),
            ev_cache,
        );

        let guard = state.lock().unwrap();
        assert_eq!(guard.mppt_total, Some(700.0));
        assert_eq!(guard.solar_total, Some(1000.0)); // 700 + 300
        assert_eq!(
            guard
                .mppt_chargers
                .as_ref()
                .unwrap()
                .first()
                .and_then(|m| m.name.as_deref()),
            Some("SmartSolar")
        );
        assert!(guard
            .pv_inverters
            .as_ref()
            .unwrap()
            .iter()
            .all(|p| p.name.as_deref() != Some("daemon-pv")));
        assert!(guard.mppt_individual.is_none());
        assert!(guard.pv_inverter_individual.is_none());
    }

    #[test]
    fn process_state_update_does_not_merge_water_or_appliances_from_daemon() {
        let state = Arc::new(Mutex::new(InverterState {
            water_level: Some(77.0),
            water_valve: Some(true),
            pump_switch: Some(false),
            dishwasher_running: Some(false),
            dishwasher_duration: Some(0),
            washer_time: Some(0),
            dryer_time: Some(0),
            ..Default::default()
        }));
        let ev_cache = Arc::new(Mutex::new(EvCache::default()));

        // Daemon JSON still may contain water/appliance keys — they must be
        // ignored (fields removed from RawInverterState) so Cerbo/HA values stay.
        let raw: RawInverterState = serde_json::from_str(
            r#"{
                "water_level": 0,
                "water_valve": false,
                "pump_switch": true,
                "dishwasher_running": true,
                "dishwasher_duration": 99,
                "washer_time": 88,
                "dryer_time": 77,
                "washer_power": true,
                "dryer_power": true,
                "dry_run": true
            }"#,
        )
        .expect("extra appliance keys must not fail deserialize");
        MqttClient::process_state_update(
            raw,
            state.clone(),
            None,
            empty_notifications(),
            None,
            None,
            ev_cache,
        );

        let guard = state.lock().unwrap();
        // Cerbo/HA-owned: daemon keys must not overwrite
        assert_eq!(guard.water_level, Some(77.0));
        assert_eq!(guard.water_valve, Some(true));
        assert_eq!(guard.pump_switch, Some(false));
        assert_eq!(guard.dishwasher_running, Some(false));
        assert_eq!(guard.dishwasher_duration, Some(0));
        assert_eq!(guard.washer_time, Some(0));
        assert_eq!(guard.dryer_time, Some(0));
        assert!(guard.washer_power.is_none());
        assert!(guard.dryer_power.is_none());
        // Daemon-only flag still merges
        assert_eq!(guard.dry_run, Some(true));
    }
}

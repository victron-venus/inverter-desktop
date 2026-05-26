use rumqttc::{Client, MqttOptions, QoS};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;

const MQTT_KEEP_ALIVE_SECS: u64 = 60;
const KEEPALIVE_INTERVAL_SECS: u64 = 45;
const MQTT_QUEUE_CAPACITY: usize = 10;
const CONSOLE_MAX_LINES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InverterState {
    pub gt: Option<f64>,
    pub g1: Option<f64>,
    pub g2: Option<f64>,
    pub tt: Option<f64>,
    pub t1: Option<f64>,
    pub t2: Option<f64>,
    pub solar_total: Option<f64>,
    pub mppt_total: Option<f64>,
    pub tasmota_total: Option<f64>,
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
    pub tasmota_individual: Option<Vec<f64>>,
    pub mppt_chargers: Option<Vec<MpptCharger>>,
    pub batteries: Option<Vec<Battery>>,
    pub loads: Option<std::collections::HashMap<String, f64>>,
    pub ui_config: Option<UiConfig>,
    pub daily_stats: Option<DailyStats>,
    pub ev_charging_kw: Option<f64>,
    pub ev_power: Option<f64>,
    pub car_soc: Option<f64>,
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

#[derive(Deserialize)]
struct RawInverterState {
    gt: Option<f64>,
    g1: Option<f64>,
    g2: Option<f64>,
    tt: Option<f64>,
    t1: Option<f64>,
    t2: Option<f64>,
    solar_total: Option<f64>,
    battery_soc: Option<f64>,
    battery_power: Option<f64>,
    battery_voltage: Option<f64>,
    battery_current: Option<f64>,
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
    tasmota_individual: Option<Vec<f64>>,
    mppt_chargers: Option<Vec<MpptCharger>>,
    batteries: Option<Vec<Battery>>,
    loads: Option<std::collections::HashMap<String, f64>>,
    ui_config: Option<UiConfig>,
    daily_stats: Option<DailyStats>,
    ev_charging_kw: Option<f64>,
    ev_power: Option<f64>,
    car_soc: Option<f64>,
    water_level: Option<f64>,
    water_valve: Option<serde_json::Value>,
    pump_switch: Option<serde_json::Value>,
    dishwasher_running: Option<bool>,
    dishwasher_duration: Option<u64>,
    washer_time: Option<u64>,
    washer_power: Option<serde_json::Value>,
    dryer_time: Option<u64>,
    dryer_power: Option<serde_json::Value>,
    latest_version: Option<String>,
    console: Option<Vec<String>>,
}

fn coerce_bool(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => s == "true" || s == "1",
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EssMode {
    pub mode_name: Option<String>,
    pub is_external: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpptCharger {
    pub name: Option<String>,
    pub pv_voltage: Option<f64>,
    pub current: Option<f64>,
    pub power: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Battery {
    pub name: Option<String>,
    pub voltage: Option<f64>,
    pub current: Option<f64>,
    pub power: Option<f64>,
    pub soc: Option<f64>,
    pub state: Option<String>,
    pub time_to_go: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub loads: Option<LoadsConfig>,
    pub home_buttons: Option<Vec<HomeButton>>,
    pub header_toggles: Option<Vec<HeaderToggle>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyStats {
    pub produced_today: Option<f64>,
    pub produced_dollars: Option<f64>,
    pub grid_kwh: Option<f64>,
    pub battery_in: Option<f64>,
    pub battery_out: Option<f64>,
    pub battery_in_yesterday: Option<f64>,
    pub battery_out_yesterday: Option<f64>,
    pub tasmota_daily: Option<Vec<f64>>,
    pub mppt_daily: Option<Vec<f64>>,
    pub pv_total_daily: Option<f64>,
}

pub struct MqttClient {
    client: Option<Client>,
    state: Arc<Mutex<InverterState>>,
    host: String,
    port: u16,
    app_handle: Option<tauri::AppHandle>,
    portal_id: Option<String>,
    camera_topic: Option<String>,
}

fn match_mqtt_topic(topic: &str, pattern: &str) -> bool {
    if pattern == topic || pattern == "#" {
        return true;
    }
    let t_parts: Vec<&str> = topic.split('/').collect();
    let p_parts: Vec<&str> = pattern.split('/').collect();

    if t_parts.len() != p_parts.length() && !pattern.ends_with("/#") {
        // Simple match, might need adjustment for #
    }
    
    // Very basic MQTT wildcard matching for +
    if t_parts.len() != p_parts.len() { return false; }
    for (t, p) in t_parts.iter().zip(p_parts.iter()) {
        if *p != "+" && *p != *t {
            return false;
        }
    }
    true
}

use log::error;
use tauri_plugin_notification::NotificationExt;

const THRESHOLD_LOAD_W: f64 = 300.0;
const THRESHOLD_CONSUMPTION_W: f64 = 300.0;
const THRESHOLD_WATER_CM: f64 = 23.0;
const THRESHOLD_SOLAR_W: f64 = 3000.0;

impl MqttClient {
    pub fn new(host: String, port: u16) -> Self {
        Self {
            client: None,
            state: Arc::new(Mutex::new(InverterState {
                gt: None,
                g1: None,
                g2: None,
                tt: None,
                t1: None,
                t2: None,
                solar_total: None,
                mppt_total: None,
                tasmota_total: None,
                battery_soc: None,
                battery_power: None,
                battery_voltage: None,
                battery_current: None,
                setpoint: None,
                inverter_state: None,
                version: None,
                dashboard_version: None,
                uptime: None,
                ha_connected: None,
                ha_direct_connected: None,
                dry_run: None,
                ess_mode: None,
                booleans: None,
                features: None,
                mppt_individual: None,
                tasmota_individual: None,
                mppt_chargers: None,
                batteries: None,
                loads: None,
                ui_config: None,
                daily_stats: None,
                ev_charging_kw: None,
                ev_power: None,
                car_soc: None,
                water_level: None,
                water_valve: None,
                pump_switch: None,
                dishwasher_running: None,
                dishwasher_duration: None,
                washer_time: None,
                washer_power: None,
                dryer_time: None,
                dryer_power: None,
                latest_version: None,
                console: None,
            })),
            host,
            port,
            app_handle: None,
            portal_id: None,
        }
    }

    pub fn set_app_handle(&mut self, handle: tauri::AppHandle) {
        self.app_handle = Some(handle);
    }

    pub fn set_portal_id(&mut self, id: Option<String>) {
        self.portal_id = id;
    }

    pub fn set_camera_topic(&mut self, topic: Option<String>) {
        self.camera_topic = topic;
    }

    pub fn get_state(&self) -> InverterState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut mqttoptions = MqttOptions::new("inverter-dashboard-desktop", &self.host, self.port);
        mqttoptions.set_keep_alive(Duration::from_secs(MQTT_KEEP_ALIVE_SECS));

        let (client, mut connection) = Client::new(mqttoptions, MQTT_QUEUE_CAPACITY);

        // Subscribe to topics
        client.subscribe("inverter/state", QoS::AtMostOnce)?;
        client.subscribe("inverter/console", QoS::AtMostOnce)?;
        
        if let Some(ref cam_topic) = self.camera_topic {
            if !cam_topic.is_empty() {
                client.subscribe(cam_topic, QoS::AtMostOnce)?;
            }
        }

        // Clone client before storing — needed for keep-alive publisher
        let keepalive_client = client.clone();
        self.client = Some(client);

        let state = self.state.clone();
        let app_handle = self.app_handle.clone();
        let portal_id = self.portal_id.clone();
        let cam_topic_owned = self.camera_topic.clone();

        // Spawn a task to handle incoming messages
        tauri::async_runtime::spawn(async move {
            tokio::task::spawn_blocking(move || {
                for event in connection.iter() {
                    match event {
                        Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish))) => {
                            let topic = publish.topic.clone();
                            let payload = String::from_utf8(publish.payload.to_vec())
                                .unwrap_or_else(|_| String::new());

                            if topic == "inverter/state" {
                                if let Ok(raw) = serde_json::from_str::<RawInverterState>(&payload)
                                {
                                    let new_state = InverterState {
                                        gt: raw.gt,
                                        g1: raw.g1,
                                        g2: raw.g2,
                                        tt: raw.tt,
                                        t1: raw.t1,
                                        t2: raw.t2,
                                        solar_total: raw.solar_total,
                                        mppt_total: raw
                                            .mppt_individual
                                            .as_ref()
                                            .map(|v| v.iter().sum()),
                                        tasmota_total: raw
                                            .tasmota_individual
                                            .as_ref()
                                            .map(|v| v.iter().sum()),
                                        battery_soc: raw.battery_soc,
                                        battery_power: raw.battery_power,
                                        battery_voltage: raw.battery_voltage,
                                        battery_current: raw.battery_current,
                                        setpoint: raw.setpoint,
                                        inverter_state: raw.inverter_state,
                                        version: raw.version,
                                        dashboard_version: raw.dashboard_version,
                                        uptime: raw.uptime,
                                        ha_connected: raw.ha_connected,
                                        ha_direct_connected: raw.ha_direct_connected,
                                        dry_run: raw.dry_run.as_ref().map(coerce_bool),
                                        ess_mode: raw.ess_mode,
                                        booleans: raw.booleans.map(|map| {
                                            map.into_iter()
                                                .map(|(k, v)| (k, coerce_bool(&v)))
                                                .collect()
                                        }),
                                        features: raw.features,
                                        mppt_individual: raw.mppt_individual,
                                        tasmota_individual: raw.tasmota_individual,
                                        mppt_chargers: raw.mppt_chargers,
                                        batteries: raw.batteries,
                                        loads: raw.loads,
                                        ui_config: raw.ui_config,
                                        daily_stats: raw.daily_stats,
                                        ev_charging_kw: raw.ev_charging_kw,
                                        ev_power: raw.ev_power,
                                        car_soc: raw.car_soc,
                                        water_level: raw.water_level,
                                        water_valve: raw.water_valve.as_ref().map(coerce_bool),
                                        pump_switch: raw.pump_switch.as_ref().map(coerce_bool),
                                        dishwasher_running: raw.dishwasher_running,
                                        dishwasher_duration: raw.dishwasher_duration,
                                        washer_time: raw.washer_time,
                                        washer_power: raw.washer_power.as_ref().map(coerce_bool),
                                        dryer_time: raw.dryer_time,
                                        dryer_power: raw.dryer_power.as_ref().map(coerce_bool),
                                        latest_version: raw.latest_version,
                                        console: raw.console,
                                    };

                                    // Check thresholds and notify
                                    if let Some(ref handle) = app_handle {
                                        if let Some(ref loads) = new_state.loads {
                                            for (name, power) in loads {
                                                if *power > THRESHOLD_LOAD_W {
                                                    let _ = handle
                                                        .notification()
                                                        .builder()
                                                        .title("High Load")
                                                        .body(format!("{}: {}W", name, power))
                                                        .show();
                                                }
                                            }
                                        }
                                        if let Some(tt) = new_state.tt {
                                            if tt > THRESHOLD_CONSUMPTION_W {
                                                let _ = handle
                                                    .notification()
                                                    .builder()
                                                    .title("High Consumption")
                                                    .body(format!("Consumption: {}W", tt))
                                                    .show();
                                            }
                                        }
                                        if let Some(wl) = new_state.water_level {
                                            if wl < THRESHOLD_WATER_CM {
                                                let _ = handle
                                                    .notification()
                                                    .builder()
                                                    .title("Low Water")
                                                    .body(format!("Water level: {} cm", wl))
                                                    .show();
                                            }
                                        }
                                        if let Some(st) = new_state.solar_total {
                                            if st > THRESHOLD_SOLAR_W {
                                                let _ = handle
                                                    .notification()
                                                    .builder()
                                                    .title("High Solar")
                                                    .body(format!("Solar: {}W", st))
                                                    .show();
                                            }
                                        }
                                    }

                                    if let Ok(mut guard) = state.lock() {
                                        *guard = new_state.clone();
                                    }
                                    if let Some(ref handle) = app_handle {
                                        let _ = handle.emit("mqtt-state-update", &new_state);
                                    }
                                }
                            } else if topic == "inverter/console" {
                                if let Ok(mut guard) = state.lock() {
                                    let console = guard.console.get_or_insert_with(Vec::new);
                                    console.push(payload);
                                    if console.len() > CONSOLE_MAX_LINES {
                                        console.remove(0);
                                    }
                                }
                            } else if let Some(ref cam_t) = cam_topic_owned {
                                if match_mqtt_topic(&topic, cam_t) {
                                    if let Some(ref handle) = app_handle {
                                        let _ = handle.emit("camera-event", payload);
                                    }
                                }
                            }
                        }
                        Ok(rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(_))) => {
                            if let Some(ref handle) = app_handle {
                                let _ = handle.emit("mqtt-connection-status", true);
                            }
                        }
                        Ok(rumqttc::Event::Incoming(_)) => {}
                        Err(e) => {
                            error!("MQTT error: {:?}", e);
                            if let Some(ref handle) = app_handle {
                                let _ = handle.emit("mqtt-connection-status", false);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(ref handle) = app_handle {
                    let _ = handle.emit("mqtt-connection-status", false);
                }
            });
        });

        // Spawn keep-alive publisher for Cerbo GX
        if let Some(pid) = portal_id {
            let topic = format!("R/{}/keepalive", pid);
            tauri::async_runtime::spawn(async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(KEEPALIVE_INTERVAL_SECS));
                loop {
                    interval.tick().await;
                    let _ = keepalive_client.publish(&topic, QoS::AtMostOnce, false, "");
                }
            });
        }

        Ok(())
    }

    pub fn publish_command(
        &self,
        action: &str,
        payload: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = &self.client {
            let topic = format!("inverter/cmd/{}", action);
            let payload_str = if payload.is_null() {
                String::new()
            } else {
                serde_json::to_string(&payload)?
            };
            client.publish(topic, QoS::AtMostOnce, false, payload_str)?;
        }
        Ok(())
    }
}

use rumqttc::{Client, MqttOptions, QoS};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InverterState {
    pub gt: Option<f64>,
    pub g1: Option<f64>,
    pub g2: Option<f64>,
    pub tt: Option<f64>,
    pub t1: Option<f64>,
    pub t2: Option<f64>,
    pub solar_total: Option<f64>,
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
}

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
        }
    }

    pub fn get_state(&self) -> InverterState {
        self.state.lock().unwrap().clone()
    }

    pub fn connect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut mqttoptions = MqttOptions::new("inverter-dashboard-desktop", &self.host, self.port);
        mqttoptions.set_keep_alive(Duration::from_secs(60));

        let (client, mut connection) = Client::new(mqttoptions, 10);

        // Subscribe to topics
        client.subscribe("inverter/state", QoS::AtMostOnce)?;
        client.subscribe("inverter/console", QoS::AtMostOnce)?;

        self.client = Some(client);

        let state = self.state.clone();

        // Spawn a task to handle incoming messages
        tauri::async_runtime::spawn(async move {
            tokio::task::spawn_blocking(move || {
                for event in connection.iter() {
                    match event {
                        Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish))) => {
                            let topic = publish.topic;
                            let payload = String::from_utf8(publish.payload.to_vec())
                                .unwrap_or_else(|_| String::new());

                            if topic == "inverter/state" {
                                if let Ok(new_state) = serde_json::from_str::<InverterState>(&payload) {
                                    *state.lock().unwrap() = new_state;
                                }
                            } else if topic == "inverter/console" {
                                let mut state = state.lock().unwrap();
                                let console = state.console.get_or_insert_with(Vec::new);
                                console.push(payload);
                                if console.len() > 50 {
                                    console.remove(0);
                                }
                            }
                        }
                        Ok(rumqttc::Event::Incoming(_)) => {}
                        Err(e) => {
                            eprintln!("MQTT error: {:?}", e);
                        }
                        _ => {}
                    }
                }
            });
        });

        Ok(())
    }

    pub fn publish_command(&self, action: &str, payload: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
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
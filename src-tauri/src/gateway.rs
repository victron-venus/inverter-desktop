//! Remote inverter-gateway client (Cloudflare Access + bearer).
//! When `gateway_enabled`, the desktop polls `/v1/snapshot` and maps Cerbo
//! leaf paths into `InverterState` for the same UI events as LAN MQTT.

use crate::mqtt::{
    inverter_state_name, voltage_soc, Battery, InverterState, MpptCharger, PvInverter,
};
use log::{info, warn};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const POLL_INTERVAL_SECS: u64 = 2;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GatewaySnapshot {
    #[serde(default)]
    pub system: HashMap<String, Value>,
    #[serde(default)]
    pub vebus: HashMap<String, Value>,
    #[serde(default)]
    pub battery: HashMap<String, Value>,
    #[serde(default)]
    pub solarcharger: HashMap<String, Value>,
    #[serde(default)]
    pub pvinverter: HashMap<String, Value>,
    #[serde(default)]
    pub tank: HashMap<String, Value>,
    #[serde(default)]
    #[allow(dead_code)]
    pub pump: HashMap<String, Value>,
    #[serde(default)]
    pub ev: HashMap<String, Value>,
    #[serde(default)]
    pub evcharger: HashMap<String, Value>,
    #[serde(default)]
    pub acload: HashMap<String, Value>,
    #[serde(default)]
    #[allow(dead_code)]
    pub settings: HashMap<String, Value>,
}

pub struct GatewayClient {
    state: Arc<Mutex<InverterState>>,
    stop: Arc<AtomicBool>,
    handle: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    base: String,
    access_client_id: String,
    access_client_secret: String,
    api_token: String,
}

impl GatewayClient {
    pub fn get_state(&self) -> InverterState {
        self.state.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut h) = self.handle.lock() {
            if let Some(join) = h.take() {
                join.abort();
            }
        }
    }

    pub fn http_auth(&self) -> GatewayHttpAuth {
        GatewayHttpAuth {
            base: self.base.clone(),
            access_client_id: self.access_client_id.clone(),
            access_client_secret: self.access_client_secret.clone(),
            api_token: self.api_token.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GatewayHttpAuth {
    pub base: String,
    pub access_client_id: String,
    pub access_client_secret: String,
    pub api_token: String,
}

pub async fn acknowledge_all_notifications_http(auth: &GatewayHttpAuth) -> Result<(), String> {
    post_command(
        &auth.base,
        &auth.access_client_id,
        &auth.access_client_secret,
        &auth.api_token,
        "acknowledge_all_notifications",
        json!({}),
    )
    .await
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn path_num(map: &HashMap<String, Value>, path: &str) -> Option<f64> {
    map.get(path).and_then(num)
}

/// Map gateway snapshot leaf maps into dashboard InverterState.
pub fn snapshot_to_state(snap: &GatewaySnapshot) -> InverterState {
    let mut st = InverterState::default();

    let g1 = path_num(&snap.system, "0/Ac/Grid/L1/Power");
    let g2 = path_num(&snap.system, "0/Ac/Grid/L2/Power");
    st.g1 = g1;
    st.g2 = g2;
    st.gt = match (g1, g2) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) | (None, Some(a)) => Some(a),
        _ => None,
    };

    let t1 = path_num(&snap.system, "0/Ac/Consumption/L1/Power");
    let t2 = path_num(&snap.system, "0/Ac/Consumption/L2/Power");
    st.t1 = t1;
    st.t2 = t2;
    st.tt = match (t1, t2) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) | (None, Some(a)) => Some(a),
        _ => None,
    };

    let batt_v = path_num(&snap.system, "0/Dc/Battery/Voltage");
    let batt_i = path_num(&snap.system, "0/Dc/Battery/Current");
    let batt_p = path_num(&snap.system, "0/Dc/Battery/Power");
    st.battery_voltage = batt_v;
    st.battery_current = batt_i;
    st.battery_power = batt_p;
    // Match LAN path: bank % from voltage (shunt SoC is unreliable while charging).
    st.battery_soc = batt_v.map(voltage_soc);

    // VE.Bus: first instance with Hub4 setpoint / State.
    let mut setpoint = None;
    let mut inv_state = None;
    let mut vebus_insts: Vec<u32> = snap
        .vebus
        .keys()
        .filter_map(|k| k.split('/').next()?.parse().ok())
        .collect();
    vebus_insts.sort_unstable();
    vebus_insts.dedup();
    for inst in vebus_insts {
        let sp = path_num(&snap.vebus, &format!("{inst}/Hub4/L1/AcPowerSetpoint"));
        let state_code = path_num(&snap.vebus, &format!("{inst}/State"));
        if setpoint.is_none() {
            setpoint = sp;
        }
        if inv_state.is_none() {
            if let Some(code) = state_code {
                inv_state = Some(inverter_state_name(code as u32));
            }
        }
    }
    st.setpoint = setpoint;
    st.inverter_state = inv_state;

    // MPPT chargers
    let mut charger_insts: Vec<u32> = snap
        .solarcharger
        .keys()
        .filter_map(|k| k.split('/').next()?.parse().ok())
        .collect();
    charger_insts.sort_unstable();
    charger_insts.dedup();
    let mut mppts = Vec::new();
    let mut mppt_powers = Vec::new();
    for inst in charger_insts {
        let power = path_num(&snap.solarcharger, &format!("{inst}/Yield/Power"))
            .or_else(|| path_num(&snap.solarcharger, &format!("{inst}/Dc/0/Power")));
        let current = path_num(&snap.solarcharger, &format!("{inst}/Dc/0/Current"));
        let pv_v = path_num(&snap.solarcharger, &format!("{inst}/Pv/V"));
        let name = snap
            .solarcharger
            .get(&format!("{inst}/CustomName"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                snap.solarcharger
                    .get(&format!("{inst}/ProductName"))
                    .and_then(|v| v.as_str())
            })
            .map(|s| s.to_string());
        if power.is_some() || current.is_some() || name.is_some() {
            if let Some(p) = power {
                mppt_powers.push(p);
            }
            mppts.push(MpptCharger {
                name,
                serial: None,
                instance: Some(inst),
                pv_voltage: pv_v,
                current,
                power,
            });
        }
    }
    let mppt_total: f64 = mppt_powers.iter().sum();
    let has_mppt = !mppts.is_empty();
    if has_mppt {
        st.mppt_chargers = Some(mppts);
        st.mppt_individual = Some(mppt_powers);
        st.mppt_total = Some(mppt_total);
    }

    // AC PV inverters
    let mut pv_insts: Vec<u32> = snap
        .pvinverter
        .keys()
        .filter_map(|k| k.split('/').next()?.parse().ok())
        .collect();
    pv_insts.sort_unstable();
    pv_insts.dedup();
    let mut pvs = Vec::new();
    let mut pv_powers = Vec::new();
    for inst in pv_insts {
        let power = path_num(&snap.pvinverter, &format!("{inst}/Ac/Power"))
            .or_else(|| path_num(&snap.pvinverter, &format!("{inst}/Ac/L1/Power")));
        let name = snap
            .pvinverter
            .get(&format!("{inst}/CustomName"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                snap.pvinverter
                    .get(&format!("{inst}/ProductName"))
                    .and_then(|v| v.as_str())
            })
            .map(|s| s.to_string());
        if let Some(p) = power {
            pv_powers.push(p);
            pvs.push(PvInverter {
                name,
                serial: None,
                instance: Some(inst),
                voltage: path_num(&snap.pvinverter, &format!("{inst}/Ac/L1/Voltage")),
                current: path_num(&snap.pvinverter, &format!("{inst}/Ac/L1/Current")),
                power: Some(p),
            });
        }
    }
    let pv_total: f64 = pv_powers.iter().sum();
    let has_pv = !pvs.is_empty();
    if has_pv {
        st.pv_inverters = Some(pvs);
        st.pv_inverter_individual = Some(pv_powers.clone());
    }

    // Only set when we saw at least one producer — Some(0) would overwrite a
    // richer LAN MQTT value if both sources briefly race.
    if has_mppt || has_pv {
        st.solar_total = Some(mppt_total + pv_total);
    }

    // Battery devices (for multi-battery UI)
    let mut bat_insts: Vec<u32> = snap
        .battery
        .keys()
        .filter_map(|k| k.split('/').next()?.parse().ok())
        .collect();
    bat_insts.sort_unstable();
    bat_insts.dedup();
    let mut bats = Vec::new();
    for inst in bat_insts {
        let voltage = path_num(&snap.battery, &format!("{inst}/Dc/0/Voltage"));
        let current = path_num(&snap.battery, &format!("{inst}/Dc/0/Current"));
        let power = path_num(&snap.battery, &format!("{inst}/Dc/0/Power"));
        let soc = path_num(&snap.battery, &format!("{inst}/Soc"));
        let name = snap
            .battery
            .get(&format!("{inst}/CustomName"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                snap.battery
                    .get(&format!("{inst}/ProductName"))
                    .and_then(|v| v.as_str())
            })
            .map(|s| s.to_string());
        if voltage.is_some() || power.is_some() || name.is_some() {
            let bat_state = current.map(crate::mqtt::MqttClient::state_from_current);
            bats.push(Battery {
                name,
                serial: None,
                instance: Some(inst),
                soc,
                voltage,
                current,
                power,
                state: bat_state,
                time_to_go: None,
                max_cell_voltage: None,
                max_voltage_cell_id: None,
                min_cell_voltage: None,
                min_voltage_cell_id: None,
            });
        }
    }
    if !bats.is_empty() {
        st.batteries = Some(bats);
    }

    // Water tank level (first tank with Level)
    let mut tank_insts: Vec<u32> = snap
        .tank
        .keys()
        .filter_map(|k| k.split('/').next()?.parse().ok())
        .collect();
    tank_insts.sort_unstable();
    tank_insts.dedup();
    for inst in tank_insts {
        if let Some(level) = path_num(&snap.tank, &format!("{inst}/Level")) {
            // Victron Level is often 0..1 fraction
            st.water_level = Some(if level <= 1.0 { level * 100.0 } else { level });
            break;
        }
    }

    // Active loads from acload
    let mut load_insts: Vec<u32> = snap
        .acload
        .keys()
        .filter_map(|k| k.split('/').next()?.parse().ok())
        .collect();
    load_insts.sort_unstable();
    load_insts.dedup();
    let mut loads = HashMap::new();
    let mut load_names = HashMap::new();
    for inst in load_insts {
        let power = path_num(&snap.acload, &format!("{inst}/Ac/Power"))
            .or_else(|| path_num(&snap.acload, &format!("{inst}/Ac/L1/Power")));
        if let Some(p) = power {
            let key = inst.to_string();
            loads.insert(key.clone(), p);
            if let Some(name) = snap
                .acload
                .get(&format!("{inst}/CustomName"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    snap.acload
                        .get(&format!("{inst}/ProductName"))
                        .and_then(|v| v.as_str())
                })
            {
                load_names.insert(key, name.to_string());
            }
        }
    }
    if !loads.is_empty() {
        st.loads = Some(loads);
        if !load_names.is_empty() {
            st.load_names = Some(load_names);
        }
    }

    // EV charger power (first with Ac/Power)
    let mut evc_insts: Vec<u32> = snap
        .evcharger
        .keys()
        .filter_map(|k| k.split('/').next()?.parse().ok())
        .collect();
    evc_insts.sort_unstable();
    evc_insts.dedup();
    for inst in &evc_insts {
        if let Some(p) = path_num(&snap.evcharger, &format!("{inst}/Ac/Power")) {
            st.ev_charging_power = Some(p);
            st.ev_power = Some(p);
            st.evcharger_present = true;
            break;
        }
    }
    if !evc_insts.is_empty() {
        st.evcharger_present = true;
    }
    if !snap.ev.is_empty() {
        st.ev_present = true;
    }

    st
}

async fn fetch_snapshot(
    client: &reqwest::Client,
    base: &str,
    access_id: &str,
    access_secret: &str,
    api_token: &str,
) -> Result<GatewaySnapshot, String> {
    let url = format!("{}/v1/snapshot", base.trim_end_matches('/'));
    let mut req = client
        .get(&url)
        .header("CF-Access-Client-Id", access_id)
        .header("CF-Access-Client-Secret", access_secret)
        .header("User-Agent", "inverter-desktop/gateway");
    if !api_token.is_empty() {
        req = req.header("Authorization", format!("Bearer {api_token}"));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("gateway snapshot request failed: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("gateway snapshot body: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "gateway snapshot HTTP {status}: {}",
            body.chars().take(160).collect::<String>()
        ));
    }
    serde_json::from_str(&body).map_err(|e| format!("gateway snapshot JSON: {e}"))
}

async fn post_command(
    base: &str,
    access_id: &str,
    access_secret: &str,
    api_token: &str,
    name: &str,
    body: Value,
) -> Result<(), String> {
    let url = format!(
        "{}/v1/commands/{}",
        base.trim_end_matches('/'),
        name.trim_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| format!("gateway http client: {e}"))?;
    let mut req = client
        .post(&url)
        .header("CF-Access-Client-Id", access_id)
        .header("CF-Access-Client-Secret", access_secret)
        .header("User-Agent", "inverter-desktop/gateway")
        .header("Content-Type", "application/json")
        .json(&body);
    if !api_token.is_empty() {
        req = req.header("Authorization", format!("Bearer {api_token}"));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("gateway command request failed: {e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("gateway command body: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "gateway command HTTP {status}: {}",
            text.chars().take(160).collect::<String>()
        ));
    }
    Ok(())
}

pub fn start_gateway_client(
    app: AppHandle,
    url: String,
    access_client_id: String,
    access_client_secret: String,
    api_token: String,
) -> Result<GatewayClient, String> {
    let base = url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("Gateway URL is required".into());
    }
    if access_client_id.trim().is_empty() || access_client_secret.trim().is_empty() {
        return Err("Cloudflare Access Client ID and Secret are required".into());
    }

    let state = Arc::new(Mutex::new(InverterState::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let state_c = state.clone();
    let stop_c = stop.clone();
    let access_id = access_client_id.trim().to_string();
    let access_secret = access_client_secret.trim().to_string();
    let token = api_token.trim().to_string();
    let access_id_poll = access_id.clone();
    let access_secret_poll = access_secret.clone();
    let token_poll = token.clone();
    let base_poll = base.clone();

    let handle = tauri::async_runtime::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                warn!("gateway http client: {e}");
                let _ = app.emit("mqtt-connection-status", false);
                return;
            }
        };

        let mut connected_emitted = false;
        let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        while !stop_c.load(Ordering::SeqCst) {
            interval.tick().await;
            if stop_c.load(Ordering::SeqCst) {
                break;
            }
            match fetch_snapshot(
                &client,
                &base_poll,
                &access_id_poll,
                &access_secret_poll,
                &token_poll,
            )
            .await
            {
                Ok(snap) => {
                    let mapped = snapshot_to_state(&snap);
                    if let Ok(mut g) = state_c.lock() {
                        *g = mapped.clone();
                    }
                    if !connected_emitted {
                        connected_emitted = true;
                        info!("gateway remote connected to {base_poll}");
                        let _ = app.emit("mqtt-connection-status", true);
                    }
                    let _ = app.emit("mqtt-state-update", mapped);
                }
                Err(e) => {
                    warn!("gateway poll failed: {e}");
                    if connected_emitted {
                        connected_emitted = false;
                        let _ = app.emit("mqtt-connection-status", false);
                    }
                }
            }
        }
        let _ = app.emit("mqtt-connection-status", false);
        info!("gateway remote poller stopped");
    });

    Ok(GatewayClient {
        state,
        stop,
        handle: Mutex::new(Some(handle)),
        base: base.clone(),
        access_client_id: access_id,
        access_client_secret: access_secret,
        api_token: token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_core_tiles() {
        let mut snap = GatewaySnapshot::default();
        snap.system.insert("0/Ac/Grid/L1/Power".into(), json!(10.0));
        snap.system.insert("0/Ac/Grid/L2/Power".into(), json!(5.0));
        snap.system
            .insert("0/Ac/Consumption/L1/Power".into(), json!(100.0));
        snap.system
            .insert("0/Ac/Consumption/L2/Power".into(), json!(50.0));
        snap.system
            .insert("0/Dc/Battery/Voltage".into(), json!(52.15));
        snap.system
            .insert("0/Dc/Battery/Power".into(), json!(-1800.0));
        snap.vebus
            .insert("290/Hub4/L1/AcPowerSetpoint".into(), json!(-2100.0));
        snap.vebus.insert("290/State".into(), json!(3));

        let st = snapshot_to_state(&snap);
        assert_eq!(st.gt, Some(15.0));
        assert_eq!(st.tt, Some(150.0));
        assert_eq!(st.battery_power, Some(-1800.0));
        assert_eq!(st.setpoint, Some(-2100.0));
        assert_eq!(st.inverter_state.as_deref(), Some("Bulk"));
        assert!(st.battery_soc.is_some());
    }
}

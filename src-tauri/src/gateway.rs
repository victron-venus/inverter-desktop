//! Remote inverter-gateway client (bearer + optional Cloudflare Access).
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

/// Gateway credentials require HTTPS, including when connecting directly to the origin.
pub(crate) fn validate_base_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Gateway URL is required".into());
    }
    let url = reqwest::Url::parse(input).map_err(|_| {
        "Enter the gateway's full HTTPS URL, for example https://gateway.example.com"
    })?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(
            "Remote Gateway requires HTTPS to protect access credentials. Use its HTTPS endpoint (for example a Cloudflare Tunnel hostname), not the HTTP origin."
                .into(),
        );
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(
            "Gateway URL must not contain a username or password; use the credential fields".into(),
        );
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(
            "Gateway URL must be a base HTTPS URL without a query string or fragment".into(),
        );
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn http_client_builder() -> Result<reqwest::ClientBuilder, String> {
    Ok(reqwest::Client::builder()
        .use_preconfigured_tls(crate::tls::client_config()?)
        .https_only(true)
        // Reqwest strips standard Authorization on redirects, but not CF Access headers.
        // Even same-origin redirects should be fixed in the configured base URL.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(25)))
}

pub(crate) fn http_client() -> Result<reqwest::Client, String> {
    http_client_builder()?
        .build()
        .map_err(|e| format!("gateway http client: {e}"))
}

pub(crate) fn validate_access_credentials(
    access_id: &str,
    access_secret: &str,
) -> Result<(), String> {
    if access_id.trim().is_empty() != access_secret.trim().is_empty() {
        return Err(
            "Provide both Cloudflare Access Client ID and Secret, or leave both empty for direct HTTPS"
                .into(),
        );
    }
    Ok(())
}

/// Cloudflare service credentials belong to clients using an Access-protected URL.
/// With the Access fields empty, only the configured IGW bearer token is added.
pub(crate) fn authenticated_request(
    mut request: reqwest::RequestBuilder,
    access_id: &str,
    access_secret: &str,
    api_token: &str,
) -> Result<reqwest::RequestBuilder, String> {
    validate_access_credentials(access_id, access_secret)?;
    if !access_id.trim().is_empty() {
        request = request
            .header("CF-Access-Client-Id", access_id.trim())
            .header("CF-Access-Client-Secret", access_secret.trim());
    }
    if !api_token.trim().is_empty() {
        request = request.bearer_auth(api_token.trim());
    }
    Ok(request)
}

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
    // Present null values explicitly invalidate a phase; absent paths remain
    // partial snapshots and must not invalidate unrelated dashboard fields.
    st.grid_l1_available = snap
        .system
        .get("0/Ac/Grid/L1/Power")
        .filter(|value| value.is_null() || num(value).is_some())
        .map(|_| g1.is_some());
    st.grid_l2_available = snap
        .system
        .get("0/Ac/Grid/L2/Power")
        .filter(|value| value.is_null() || num(value).is_some())
        .map(|_| g2.is_some());
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

    // Bank V/I/P/SoC: prefer SmartShunt (same as LAN `apply_cerbo_to_state`).
    // system/0/Dc/Battery/* is a parallel aggregate and often disagrees with the shunt
    // (e.g. 25.2 A system vs 23.5 A shunt) — that was the IGW vs MQTT mismatch.
    // Filled after the battery device list below.

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
            let serial = snap
                .solarcharger
                .get(&format!("{inst}/Serial"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            mppts.push(MpptCharger {
                name,
                serial,
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
        let serial = snap
            .battery
            .get(&format!("{inst}/Serial"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        if voltage.is_some() || power.is_some() || current.is_some() || name.is_some() {
            let bat_state = current.map(crate::mqtt::MqttClient::state_from_current);
            // Cerbo publishes TimeToGo in seconds (null when idle). Same formatter as LAN MQTT.
            let mut time_to_go = path_num(&snap.battery, &format!("{inst}/TimeToGo"))
                .and_then(crate::mqtt::MqttClient::format_time_to_go);
            // Only meaningful while charging/discharging (mirrors apply_cerbo_to_state).
            if !matches!(bat_state.as_deref(), Some("Charging") | Some("Discharging")) {
                time_to_go = None;
            }
            bats.push(Battery {
                name,
                serial,
                instance: Some(inst),
                soc,
                voltage,
                current,
                power,
                state: bat_state,
                time_to_go,
                max_cell_voltage: path_num(&snap.battery, &format!("{inst}/System/MaxCellVoltage")),
                max_voltage_cell_id: snap
                    .battery
                    .get(&format!("{inst}/System/MaxVoltageCellId"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                min_cell_voltage: path_num(&snap.battery, &format!("{inst}/System/MinCellVoltage")),
                min_voltage_cell_id: snap
                    .battery
                    .get(&format!("{inst}/System/MinVoltageCellId"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            });
        }
    }
    if !bats.is_empty() {
        // Bank totals from SmartShunt only (never sum chains — double-counts).
        if let Some(shunt) = bats.iter().find(|b| {
            b.name
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains("shunt")
        }) {
            st.battery_voltage = shunt.voltage;
            st.battery_current = Some(shunt.current.unwrap_or(0.0));
            st.battery_power = Some(shunt.power.unwrap_or(0.0));
            st.battery_soc = shunt.voltage.map(voltage_soc);
        } else {
            let batt_v = path_num(&snap.system, "0/Dc/Battery/Voltage");
            st.battery_voltage = batt_v;
            st.battery_current = path_num(&snap.system, "0/Dc/Battery/Current");
            st.battery_power = path_num(&snap.system, "0/Dc/Battery/Power");
            st.battery_soc = batt_v.map(voltage_soc);
        }
        st.batteries = Some(bats);
    } else {
        let batt_v = path_num(&snap.system, "0/Dc/Battery/Voltage");
        st.battery_voltage = batt_v;
        st.battery_current = path_num(&snap.system, "0/Dc/Battery/Current");
        st.battery_power = path_num(&snap.system, "0/Dc/Battery/Power");
        st.battery_soc = batt_v.map(voltage_soc);
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
    let base = validate_base_url(base)?;
    let url = format!("{base}/v1/snapshot");
    let req = authenticated_request(
        client
            .get(&url)
            .header("User-Agent", "inverter-desktop/gateway"),
        access_id,
        access_secret,
        api_token,
    )?;
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
    let base = validate_base_url(base)?;
    let url = format!("{}/v1/commands/{}", base, name.trim_matches('/'));
    let client = http_client()?;
    let req = authenticated_request(
        client
            .post(&url)
            .header("User-Agent", "inverter-desktop/gateway")
            .json(&body),
        access_id,
        access_secret,
        api_token,
    )?;
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

/// Keep last time_to_go when this poll has no TimeToGo leaf (Cerbo often nulls it)
/// and the battery is not Idle.
fn preserve_battery_time_to_go(prev: &InverterState, next: &mut InverterState) {
    let Some(next_bats) = next.batteries.as_mut() else {
        return;
    };
    let Some(prev_bats) = prev.batteries.as_ref() else {
        return;
    };
    for bat in next_bats.iter_mut() {
        if bat.time_to_go.is_some() {
            continue;
        }
        if bat.state.as_deref() == Some("Idle") {
            continue;
        }
        let prev_bat = prev_bats.iter().find(|p| {
            (p.serial.is_some() && p.serial == bat.serial)
                || (p.instance.is_some() && p.instance == bat.instance)
                || (p.name.is_some()
                    && !p.name.as_deref().unwrap_or("").is_empty()
                    && p.name == bat.name)
        });
        if let Some(p) = prev_bat {
            if p.time_to_go.is_some() {
                bat.time_to_go = p.time_to_go.clone();
            }
        }
    }
}

pub fn start_gateway_client(
    app: AppHandle,
    url: String,
    access_client_id: String,
    access_client_secret: String,
    api_token: String,
) -> Result<GatewayClient, String> {
    let base = validate_base_url(&url)?;
    validate_access_credentials(&access_client_id, &access_client_secret)?;

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
        let client = match http_client() {
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
                    let mut mapped = snapshot_to_state(&snap);
                    if let Ok(mut g) = state_c.lock() {
                        preserve_battery_time_to_go(&g, &mut mapped);
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
pub(crate) fn idle_test_client() -> GatewayClient {
    GatewayClient {
        state: Arc::new(Mutex::new(InverterState::default())),
        stop: Arc::new(AtomicBool::new(false)),
        handle: Mutex::new(None),
        base: String::new(),
        access_client_id: String::new(),
        access_client_secret: String::new(),
        api_token: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_gateway_requests_omit_cloudflare_headers() {
        let client = http_client().unwrap();
        for (method, path) in [
            (reqwest::Method::GET, "/health"),
            (reqwest::Method::GET, "/v1/snapshot"),
            (
                reqwest::Method::POST,
                "/v1/commands/acknowledge_all_notifications",
            ),
        ] {
            let request = authenticated_request(
                client.request(method.clone(), format!("https://gateway.example.com{path}")),
                " ",
                "\t",
                " read-token ",
            )
            .unwrap()
            .build()
            .unwrap();
            assert_eq!(request.method(), method);
            assert_eq!(request.headers()["Authorization"], "Bearer read-token");
            assert!(!request.headers().contains_key("CF-Access-Client-Id"));
            assert!(!request.headers().contains_key("CF-Access-Client-Secret"));
        }
    }

    #[test]
    fn cloudflare_gateway_requests_preserve_complete_service_credentials() {
        let client = http_client().unwrap();
        let request = authenticated_request(
            client.get("https://gateway.example.com/v1/snapshot"),
            " access-id ",
            " access-secret ",
            " api-token ",
        )
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(request.headers()["CF-Access-Client-Id"], "access-id");
        assert_eq!(
            request.headers()["CF-Access-Client-Secret"],
            "access-secret"
        );
        assert_eq!(request.headers()["Authorization"], "Bearer api-token");
        let without_bearer = authenticated_request(
            client.get("https://gateway.example.com/health"),
            "access-id",
            "access-secret",
            " ",
        )
        .unwrap()
        .build()
        .unwrap();
        assert!(!without_bearer.headers().contains_key("Authorization"));
    }

    #[tokio::test]
    async fn all_gateway_operations_reject_incomplete_cloudflare_credentials() {
        let client = http_client().unwrap();
        let base = "https://127.0.0.1:1";
        for (id, secret) in [("id", " "), ("", "secret")] {
            assert!(fetch_snapshot(&client, base, id, secret, "token")
                .await
                .unwrap_err()
                .contains("Provide both Cloudflare"));
            assert!(post_command(base, id, secret, "token", "test", json!({}))
                .await
                .unwrap_err()
                .contains("Provide both Cloudflare"));
            assert!(crate::test_gateway_connection(
                base.into(),
                id.into(),
                secret.into(),
                Some("token".into())
            )
            .await
            .unwrap_err()
            .contains("Provide both Cloudflare"));
        }
    }

    #[test]
    fn gateway_url_requires_https_and_separate_credentials() {
        assert_eq!(
            validate_base_url("  https://gateway.example.com:8443/inverter///  ").unwrap(),
            "https://gateway.example.com:8443/inverter"
        );
        assert_eq!(
            validate_base_url("https://gateway.example.com").unwrap(),
            "https://gateway.example.com"
        );
        for input in [
            "",
            "gateway.example.com",
            "http://gateway.example.com",
            "http://localhost:8080",
            "file:///tmp/gateway",
            "https://user:secret@gateway.example.com",
            "https://gateway.example.com?token=secret",
            "https://gateway.example.com#fragment",
        ] {
            assert!(
                validate_base_url(input).is_err(),
                "accepted unsafe gateway URL"
            );
        }
    }

    #[tokio::test]
    async fn all_gateway_operations_reject_http_before_sending_credentials() {
        let client = http_client().unwrap();
        let url = "http://127.0.0.1:1";
        assert!(fetch_snapshot(&client, url, "id", "secret", "token")
            .await
            .unwrap_err()
            .contains("requires HTTPS"));
        assert!(
            post_command(url, "id", "secret", "token", "test", json!({}))
                .await
                .unwrap_err()
                .contains("requires HTTPS")
        );
        assert!(crate::test_gateway_connection(
            url.into(),
            "id".into(),
            "secret".into(),
            Some("token".into())
        )
        .await
        .err()
        .unwrap()
        .contains("requires HTTPS"));
        // The client also refuses an HTTP URL if a future caller misses URL validation.
        assert!(client.get(url).send().await.unwrap_err().is_builder());
    }

    #[tokio::test]
    async fn redirects_never_forward_gateway_credentials_or_commands() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        // Only this test disables HTTPS, to inspect real redirect behaviour with local
        // HTTP fixtures. It retains the exact production redirect policy.
        let client = http_client_builder()
            .unwrap()
            .https_only(false)
            .no_proxy()
            .build()
            .unwrap();
        for (method, status) in [
            (reqwest::Method::GET, 302),
            (reqwest::Method::POST, 307),
            (reqwest::Method::POST, 308),
        ] {
            let destination = TcpListener::bind("127.0.0.1:0").unwrap();
            destination.set_nonblocking(true).unwrap();
            let origin = TcpListener::bind("127.0.0.1:0").unwrap();
            origin.set_nonblocking(true).unwrap();
            let origin_url = format!("http://{}/v1/snapshot", origin.local_addr().unwrap());
            let redirect_url = format!("http://{}/credentials", destination.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match origin.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "gateway test request timed out"
                            );
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("gateway test accept failed: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buf = [0; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let size = stream.read(&mut buf).unwrap();
                    assert_ne!(size, 0);
                    request.extend_from_slice(&buf[..size]);
                }
                write!(stream, "HTTP/1.1 {status} Redirect\r\nLocation: {redirect_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                String::from_utf8(request).unwrap()
            });
            let response = client
                .request(method, &origin_url)
                .header("CF-Access-Client-Id", "test-client")
                .header("CF-Access-Client-Secret", "test-secret")
                .bearer_auth("test-token")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            let request = server.join().unwrap().to_ascii_lowercase();
            assert!(request.contains("cf-access-client-secret: test-secret"));
            assert!(request.contains("authorization: bearer test-token"));
            assert_eq!(
                destination.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
    }

    #[test]
    fn stopping_idle_client_sets_shutdown_and_is_idempotent() {
        let client = idle_test_client();
        client.stop();
        client.stop();
        assert!(client.stop.load(Ordering::SeqCst));
        assert!(client.handle.lock().unwrap().is_none());
    }

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

    #[test]
    fn bank_totals_prefer_smartshunt_over_system() {
        let mut snap = GatewaySnapshot::default();
        // Divergent system aggregate (what Cerbo systemcalc publishes).
        snap.system
            .insert("0/Dc/Battery/Voltage".into(), json!(53.56));
        snap.system
            .insert("0/Dc/Battery/Current".into(), json!(25.2));
        snap.system
            .insert("0/Dc/Battery/Power".into(), json!(1349.7));
        // Per-device batteries — bank must follow the shunt, not the sum of chains.
        snap.battery
            .insert("289/ProductName".into(), json!("SmartShunt 500A/50mV"));
        snap.battery.insert("289/Dc/0/Voltage".into(), json!(53.55));
        snap.battery.insert("289/Dc/0/Current".into(), json!(23.5));
        snap.battery.insert("289/Dc/0/Power".into(), json!(1258.4));
        snap.battery
            .insert("512/ProductName".into(), json!("JBD Battery Chain 1"));
        snap.battery.insert("512/Dc/0/Current".into(), json!(7.03));
        snap.battery.insert("512/Dc/0/Power".into(), json!(376.0));

        let st = snapshot_to_state(&snap);
        assert_eq!(st.battery_current, Some(23.5));
        assert_eq!(st.battery_power, Some(1258.4));
        assert_eq!(st.battery_voltage, Some(53.55));
        assert_eq!(st.batteries.as_ref().map(|b| b.len()), Some(2));
    }

    #[test]
    fn maps_battery_time_to_go_while_charging() {
        let mut snap = GatewaySnapshot::default();
        snap.battery
            .insert("289/ProductName".into(), json!("SmartShunt 500A/50mV"));
        snap.battery.insert("289/Dc/0/Voltage".into(), json!(53.55));
        snap.battery.insert("289/Dc/0/Current".into(), json!(23.5));
        snap.battery.insert("289/Dc/0/Power".into(), json!(1258.4));
        // ~40h 48m = 146880 seconds
        snap.battery.insert("289/TimeToGo".into(), json!(146880.0));

        let st = snapshot_to_state(&snap);
        let b = &st.batteries.as_ref().unwrap()[0];
        assert_eq!(b.state.as_deref(), Some("Charging"));
        assert_eq!(b.time_to_go.as_deref(), Some("40h 48m"));
    }

    #[test]
    fn hides_battery_time_to_go_when_idle() {
        let mut snap = GatewaySnapshot::default();
        snap.battery
            .insert("289/ProductName".into(), json!("SmartShunt 500A/50mV"));
        snap.battery.insert("289/Dc/0/Current".into(), json!(0.1));
        snap.battery.insert("289/TimeToGo".into(), json!(146880.0));

        let st = snapshot_to_state(&snap);
        let b = &st.batteries.as_ref().unwrap()[0];
        assert_eq!(b.state.as_deref(), Some("Idle"));
        assert_eq!(b.time_to_go, None);
    }

    #[test]
    fn solar_total_sums_mppt_yield_and_ac_pv() {
        let mut snap = GatewaySnapshot::default();
        snap.solarcharger
            .insert("290/Yield/Power".into(), json!(403.57));
        snap.solarcharger
            .insert("291/Yield/Power".into(), json!(594.33));
        snap.solarcharger
            .insert("292/Yield/Power".into(), json!(455.39));
        snap.pvinverter.insert("369/Ac/Power".into(), json!(235.0));
        snap.pvinverter.insert("9895/Ac/Power".into(), json!(278.0));

        let st = snapshot_to_state(&snap);
        assert_eq!(st.mppt_total, Some(403.57 + 594.33 + 455.39));
        assert_eq!(
            st.solar_total,
            Some(403.57 + 594.33 + 455.39 + 235.0 + 278.0)
        );
        assert_eq!(st.mppt_chargers.as_ref().map(|c| c.len()), Some(3));
        assert_eq!(st.pv_inverters.as_ref().map(|c| c.len()), Some(2));
    }

    /// Tray sparkline reads `solar_total` + `gt` from the same mapped state.
    #[test]
    fn maps_tray_sparkline_metrics_from_live_like_snapshot() {
        let mut snap = GatewaySnapshot::default();
        snap.system
            .insert("0/Ac/Grid/L1/Power".into(), json!(-32.0));
        snap.system.insert("0/Ac/Grid/L2/Power".into(), json!(17.0));
        snap.system
            .insert("0/Ac/Consumption/L1/Power".into(), json!(411.0));
        snap.system
            .insert("0/Ac/Consumption/L2/Power".into(), json!(17.0));
        snap.system
            .insert("0/Dc/Battery/Voltage".into(), json!(53.21));
        snap.system
            .insert("0/Dc/Battery/Current".into(), json!(-9.3));
        snap.system
            .insert("0/Dc/Battery/Power".into(), json!(-494.85));
        snap.vebus
            .insert("290/Hub4/L1/AcPowerSetpoint".into(), json!(-394));
        snap.vebus.insert("290/State".into(), json!(3));
        // Cerbo often publishes null on unused phases — must not break mapping.
        snap.vebus
            .insert("290/Ac/ActiveIn/L2/P".into(), json!(null));
        snap.solarcharger
            .insert("290/Yield/Power".into(), json!(1250.0));
        snap.solarcharger
            .insert("290/ProductName".into(), json!("SmartSolar"));
        snap.battery
            .insert("289/ProductName".into(), json!("SmartShunt 500A/50mV"));
        snap.battery.insert("289/Dc/0/Voltage".into(), json!(53.21));
        snap.battery.insert("289/Dc/0/Current".into(), json!(-9.3));
        snap.battery.insert("289/Dc/0/Power".into(), json!(-494.85));

        let st = snapshot_to_state(&snap);
        assert_eq!(st.gt, Some(-15.0));
        assert_eq!(st.tt, Some(428.0));
        assert_eq!(st.solar_total, Some(1250.0));
        assert_eq!(st.setpoint, Some(-394.0));
        assert_eq!(st.battery_power, Some(-494.85));
        // IPC must include these so the menu-bar painter (and UI) see them.
        let json = serde_json::to_value(&st).unwrap();
        assert_eq!(json.get("gt").and_then(|v| v.as_f64()), Some(-15.0));
        assert_eq!(
            json.get("solar_total").and_then(|v| v.as_f64()),
            Some(1250.0)
        );
    }
}

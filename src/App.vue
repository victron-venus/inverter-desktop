<template>
  <div id="app" class="container-fluid p-2">
    <div class="ws-status" :class="mqttConnected ? 'connected' : 'disconnected'">
      <i class="fas" :class="mqttConnected ? 'fa-link' : 'fa-unlink'"></i>
      {{ mqttConnected ? 'Live' : 'Disconnected' }}
    </div>

    <!-- Header -->
    <div class="card mb-2">
      <div class="card-body py-1 px-2">
        <div class="d-flex flex-wrap gap-1 align-items-center">
          <v-btn
            outlined
            class="custom-3d"
            :color="state.dry_run ? 'success' : 'grey-darken-3'"
            size="small"
            @click="send('dry_run')"
          >
            <i class="fas fa-flask me-1"></i>DRY
          </v-btn>
          <v-btn
            outlined
            class="custom-3d"
            :color="essClass === 'on' ? 'success' : 'grey-darken-3'"
            size="small"
            @click="send('ess_mode')"
          >
            <i class="fas fa-bolt me-1"></i>{{ essText }}
          </v-btn>
          <div class="vr mx-1" style="border-left:1px solid #ccc;height:16px;"></div>
          <v-btn
            v-for="toggle in headerToggles"
            :key="toggle.id"
            outlined
            class="custom-3d"
            :color="state.booleans?.[toggle.id] === true ? 'success' : 'grey-darken-3'"
            size="small"
            @click="send('toggle', {entity: toggle.entity})"
          >
            {{ toggle.label }}
          </v-btn>
          <div class="ms-auto d-flex gap-1">
            <v-btn
              outlined
              class="custom-3d"
              icon
              size="small"
              @click="toggleTheme"
              id="theme-btn"
            >
              <i class="fas" :class="isDark ? 'fa-sun' : 'fa-moon'"></i>
            </v-btn>
          </div>
        </div>
      </div>
    </div>

    <!-- Daily stats -->
    <div class="daily-stats mb-2" v-html="dailyStatsHtml"></div>

    <!-- Main stats -->
    <div class="row g-2 mb-2">
      <div class="col-md-2">
        <div class="card h-100"><div class="card-body text-center">
          <div class="stat-label">Grid</div>
          <div class="stat-value text-grid">{{ formatPower(state.gt) }}</div>
          <div class="stat-sub">{{ formatPower(state.g1) }} | {{ formatPower(state.g2) }}</div>
        </div></div>
      </div>
      <div class="col-md-2">
        <div class="card h-100"><div class="card-body text-center">
          <div class="stat-label">Consumption</div>
          <div class="stat-value text-consumption">{{ formatPower(state.tt) }}</div>
          <div class="stat-sub">{{ formatPower(state.t1) }} | {{ formatPower(state.t2) }}</div>
        </div></div>
      </div>
      <div class="col-md-3">
        <div class="card h-100"><div class="card-body text-center">
          <div class="stat-label">Solar</div>
          <div class="stat-value text-solar">{{ formatPower(state.solar_total) }}</div>
          <div class="stat-sub">{{ formatPower(mpptTotal) }} | {{ formatPower(tasmotaTotal) }}</div>
        </div></div>
      </div>
      <div class="col-md-3">
        <div class="card h-100"><div class="card-body text-center">
          <div class="stat-label">Battery (Shunt)</div>
          <div class="stat-value text-battery">{{ Math.floor(state.battery_soc || 0) }}%</div>
          <div class="stat-sub">{{ formatPower(state.battery_power) }} | {{ (state.battery_voltage || 0).toFixed(2) }}V | {{ (state.battery_current || 0).toFixed(1) }}A</div>
        </div></div>
      </div>
      <div class="col-md-2">
        <div class="card h-100"><div class="card-body text-center">
          <div class="stat-label">Setpoint</div>
          <div class="stat-value text-accent">{{ formatPower(state.setpoint) }}</div>
          <div class="stat-sub">{{ state.inverter_state || '--' }}</div>
        </div></div>
      </div>
    </div>

    <!-- Chart and side panels -->
    <div class="row g-2 mb-2">
      <div class="col-md-8">
        <div class="card"><div class="card-body py-1">
          <v-chart
            ref="chartRef"
            class="chart-wrap"
            :option="chartOption"
            :autoresize="true"
          />
        </div></div>
      </div>
      <div class="col-md-4">
        <!-- EV -->
        <div class="card mb-2" v-if="state.features?.ev !== false">
          <div class="card-header"><i class="fas fa-car me-2"></i>EV</div>
          <div class="card-body py-1">
            <div class="d-flex justify-content-between">
              <div><div class="stat-value text-solar">{{ evCharging }}</div><div class="stat-sub">Charging</div></div>
              <div class="text-center"><div class="stat-value" style="color:#9e9e9e">{{ evPower }}</div><div class="stat-sub">VUE</div></div>
              <div class="text-end"><div class="stat-value text-accent">{{ Math.floor(state.car_soc || 0) }}%</div><div class="stat-sub">SoC</div></div>
            </div>
          </div>
        </div>
        <!-- Water -->
        <div class="card mb-2" v-if="state.features?.water !== false">
          <div class="card-header"><i class="fas fa-faucet me-2"></i>Water</div>
          <div class="card-body py-1">
            <div class="d-flex justify-content-between align-items-center">
              <div class="fw-bold" :style="{color: state.water_valve ? '#f44336' : '#4caf50'}">{{ state.water_level || 0 }} cm</div>
              <div class="d-flex gap-1">
                <v-btn
                  outlined
                  class="custom-3d"
                  :color="state.pump_switch ? 'success' : 'grey-darken-3'"
                  size="small"
                  @click="send('toggle', {entity: pumpSwitchEntity})"
                >
                  PUMP
                </v-btn>
                <v-btn
                  outlined
                  class="custom-3d"
                  :color="state.water_valve ? 'success' : 'grey-darken-3'"
                  size="small"
                  @click="send('toggle', {entity: waterValveEntity})"
                >
                  VALVE
                </v-btn>
              </div>
            </div>
          </div>
        </div>
        <!-- Dishwasher -->
        <div class="card mb-2" v-if="state.features?.dishwasher !== false && state.dishwasher_running">
          <div class="card-header"><i class="fas fa-utensils me-2"></i>Dishwasher</div>
          <div class="card-body py-1">
            <div class="d-flex justify-content-between align-items-center">
              <div class="fw-bold text-success">Running</div>
              <div>{{ formatDuration(state.dishwasher_duration) }}</div>
            </div>
          </div>
        </div>
        <!-- Washer -->
        <div class="card mb-2" v-if="state.features?.washer !== false && (((state.washer_time || 0) > 0) || state.washer_power)">
          <div class="card-header"><i class="fas fa-soap me-2"></i>Washer</div>
          <div class="card-body py-1">
            <div class="d-flex justify-content-between align-items-center">
              <div class="fw-bold">{{ formatDuration(state.washer_time) }}</div>
              <v-btn
                outlined
                class="custom-3d"
                :color="state.washer_power ? 'success' : 'grey-darken-3'"
                size="small"
                disabled
              >
                PWR
              </v-btn>
            </div>
          </div>
        </div>
        <!-- Dryer -->
        <div class="card mb-2" v-if="state.features?.dryer !== false && (((state.dryer_time || 0) > 0) || state.dryer_power)">
          <div class="card-header"><i class="fas fa-wind me-2"></i>Dryer</div>
          <div class="card-body py-1">
            <div class="d-flex justify-content-between align-items-center">
              <div class="fw-bold">{{ formatDuration(state.dryer_time) }}</div>
              <v-btn
                outlined
                class="custom-3d"
                :color="state.dryer_power ? 'success' : 'grey-darken-3'"
                size="small"
                disabled
              >
                PWR
              </v-btn>
            </div>
          </div>
        </div>
        <!-- Home -->
        <div class="card" v-if="state.features?.ha !== false && homeButtons.length > 0">
          <div class="card-header"><i class="fas fa-home me-2"></i>Home</div>
          <div class="card-body py-1">
            <div class="d-flex gap-1 flex-wrap">
              <v-btn
                v-for="btn in homeButtons"
                :key="btn.id"
                outlined
                class="custom-3d"
                :color="buttonStates[btn.id] === 'on' ? 'success' : 'grey-darken-3'"
                size="small"
                @click="send('toggle', {entity: btn.entity})"
              >
                {{ btn.label }}
              </v-btn>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Batteries & Solar Production -->
    <div class="row g-2 mb-2">
      <div class="col-md-6">
        <div class="card h-100">
          <div class="card-header"><i class="fas fa-battery-three-quarters me-2"></i>Batteries</div>
          <div class="card-body py-1" style="font-size:0.75rem">
            <div class="d-flex flex-wrap gap-2">
              <div v-for="bat in batteries" :key="bat.name" class="flex-fill subsection" style="min-width:140px">
                <div class="fw-bold mb-1" style="font-size:0.65rem;color:var(--text-dim)">{{ bat.name }}</div>
                <div class="d-flex justify-content-between subsection-value">
                  <span>{{ bat.voltage.toFixed(2) }}V</span>
                  <span v-if="bat.current !== undefined">{{ bat.current.toFixed(1) }}A</span>
                  <span v-if="bat.power !== undefined">{{ Math.floor(bat.power) }}W</span>
                </div>
                <div class="d-flex justify-content-between mt-1">
                  <span class="fw-bold" :style="{color: bat.soc > 50 ? '#7ed321' : bat.soc > 20 ? '#f5a623' : '#e74c3c'}">{{ bat.soc.toFixed(1) }}%</span>
                  <span style="color:var(--text-dim);text-align:right">{{ bat.state }}{{ bat.time_to_go ? ' · ' + bat.time_to_go : '' }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
      <div class="col-md-6">
        <div class="card h-100">
          <div class="card-header"><i class="fas fa-solar-panel me-2"></i>Solar Production</div>
          <div class="card-body py-1" style="font-size:0.75rem">
            <div class="d-flex flex-wrap gap-2">
              <div v-for="src in solarSources" :key="src.name" class="flex-fill subsection" style="min-width:100px">
                <div class="fw-bold mb-1" style="font-size:0.65rem;color:var(--text-dim)">{{ src.name }}</div>
                <div v-if="src.pv_voltage" class="subsection-value" style="color:var(--solar)">{{ src.pv_voltage.toFixed(2) }}V</div>
                <div v-if="src.current" class="subsection-value">{{ src.current.toFixed(1) }}A</div>
                <div class="fw-bold" style="color:var(--solar)">{{ Math.floor(src.power) }}W</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Loads -->
    <div class="row g-2 mb-2" v-if="state.features?.ha_loads !== false && sortedLoads.length">
      <div class="col-12">
        <div class="card">
          <div class="card-header">Loads</div>
          <div class="card-body py-1" id="loads">
            <div class="loads-table">
              <div class="loads-row" v-for="[name, val] in sortedLoads" :key="name">
                <span class="loads-name">{{ name }}</span>
                <span class="loads-value">{{ Math.floor(val) }}W</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Status -->
    <div class="mt-2 text-center small" style="color:#666">
      <span class="status-dot" :class="state.ha_connected ? 'online' : 'offline'"></span>
      HA
      &nbsp;|&nbsp; Uptime: {{ formatUptime(state.uptime || 0) }}
      &nbsp;|&nbsp; MQTT: {{ mqttConnected ? 'OK' : 'Disconnected' }}
      &nbsp;|&nbsp; Desktop v{{ appVersion }} · Control v{{ appVersion }}
    </div>
  </div>

</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import VChart from 'vue-echarts'
import * as echarts from 'echarts'
VChart.inst = echarts
import { getVersion } from '@tauri-apps/api/app'
import { getAppConfig, defaultConfig } from './config'

interface InverterState {
  gt?: number
  g1?: number
  g2?: number
  tt?: number
  t1?: number
  t2?: number
  solar_total?: number
  battery_soc?: number
  battery_power?: number
  battery_voltage?: number
  battery_current?: number
  setpoint?: number
  inverter_state?: string
  version?: string
  dashboard_version?: string
  uptime?: number
  ha_connected?: boolean
  ha_direct_connected?: boolean
  dry_run?: boolean
  ess_mode?: { mode_name?: string; is_external?: boolean }
  booleans?: Record<string, boolean>
  features?: Record<string, boolean>
  mppt_individual?: number[]
  tasmota_individual?: number[]
  mppt_chargers?: Array<{ name?: string; pv_voltage?: number; current?: number; power?: number }>
  batteries?: Array<{ name?: string; voltage?: number; current?: number; power?: number; soc?: number; state?: string; time_to_go?: string }>
  loads?: Record<string, number>
  ui_config?: {
    loads?: { hidden?: string[]; min_watts?: number }
    home_buttons?: Array<{ id: string; label: string; entity: string; state_key?: string }>
    header_toggles?: Array<{ id: string; label: string; entity: string }>
  }
  daily_stats?: {
    produced_today?: number
    produced_dollars?: number
    grid_kwh?: number
    battery_in?: number
    battery_out?: number
    battery_in_yesterday?: number
    battery_out_yesterday?: number
    tasmota_daily?: number[]
    mppt_daily?: number[]
    pv_total_daily?: number
  }
  ev_charging_kw?: number
  ev_power?: number
  car_soc?: number
  water_level?: number
  water_valve?: boolean
  pump_switch?: boolean
  dishwasher_running?: boolean
  dishwasher_duration?: number
  washer_time?: number
  washer_power?: boolean
  dryer_time?: number
  dryer_power?: boolean
  latest_version?: string
  console?: string[]
}

const state = ref<InverterState>({
  booleans: {},
  features: {},
  loads: {},
  ui_config: {}
})
const mqttConnected = ref(false)
const isDark = ref(localStorage.getItem('theme') !== 'light')
const appConfig = ref<any>(null)
const appVersion = ref('')
const chartOption = ref<any>({})

let pollInterval: number | null = null
let historyData = { timestamps: [] as number[], grid: [] as number[], solar: [] as number[], battery: [] as number[], setpoint: [] as number[] }

function toggleTheme() {
  isDark.value = !isDark.value
  document.body.classList.toggle('light', !isDark.value)
  localStorage.setItem('theme', isDark.value ? 'dark' : 'light')
}

if (!isDark.value) document.body.classList.add('light')

async function connectMqtt() {
  try {
    const config = await getAppConfig()
    appConfig.value = config
    await invoke('connect_mqtt', { host: config.mqtt_host, port: config.mqtt_port })
    mqttConnected.value = true
    startPolling()
  } catch (e) {
    console.error('Failed to connect to MQTT:', e)
    mqttConnected.value = false
  }
}

function startPolling() {
  if (pollInterval) clearInterval(pollInterval)
  pollInterval = window.setInterval(async () => {
    try {
      const newState = await invoke<InverterState>('get_state')
      // Convert boolean strings to actual booleans
      if (newState.booleans) {
        Object.keys(newState.booleans).forEach(key => {
          const val = newState.booleans[key]
          if (typeof val === 'string') {
            newState.booleans[key] = val === 'true' || val === '1'
          }
        })
      }
      // Convert specific boolean fields that might come as strings
      const boolFields = ['pump_switch', 'water_valve', 'washer_power', 'dryer_power', 'dry_run'];
      boolFields.forEach(field => {
        if (typeof newState[field] === 'string') {
          newState[field] = newState[field] === 'true' || newState[field] === '1';
        }
      })
      // Debug: log booleans to see what keys are available
      if (newState.booleans) {
        console.log('Booleans from HA:', Object.keys(newState.booleans))
      }
      state.value = newState

      if (newState.gt !== undefined) {
        const now = Date.now() / 1000
        historyData.timestamps.push(now)
        historyData.grid.push(newState.gt || 0)
        historyData.solar.push(newState.solar_total || 0)
        historyData.battery.push(newState.battery_power || 0)
        historyData.setpoint.push(newState.setpoint || 0)

        if (historyData.timestamps.length > 1800) {
          historyData.timestamps.shift()
          historyData.grid.shift()
          historyData.solar.shift()
          historyData.battery.shift()
          historyData.setpoint.shift()
        }
        updateChartOption()
      }
    } catch (e) {
      console.error('Failed to get state:', e)
      mqttConnected.value = false
    }
  }, 1000)
}

async function send(action: string, payload: any = {}) {
  try {
    await invoke('send_command', { action, payload })
  } catch (e) {
    console.error('Failed to send command:', e)
  }
}

function formatPower(w: number | undefined) {
  const v = Math.abs(Math.floor(w || 0))
  const sign = w && w < 0 ? '-' : ''
  return v >= 1000 ? sign + (v / 1000).toFixed(1) + 'kW' : sign + v + 'W'
}

function formatUptime(s: number) {
  if (s < 60) return s + 's'
  if (s < 3600) return Math.floor(s / 60) + 'm'
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60)
  return h + 'h ' + m + 'm'
}

function formatDuration(s: number | undefined) {
  if (!s || s <= 0) return '0:00'
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = Math.floor(s % 60)
  if (h > 0) return h + ':' + String(m).padStart(2, '0') + ':' + String(sec).padStart(2, '0')
  return m + ':' + String(sec).padStart(2, '0')
}

function formatSemverLabel(ver: string | undefined) {
  if (ver === null || ver === undefined || ver === '') return '?'
  const s = String(ver).trim()
  if (s === '?') return '?'
  if (/^v[0-9]/i.test(s)) return s
  if (/^[0-9]/.test(s)) return 'v' + s
  return s
}

function getButtonState(btn: { id: string; state_key?: string }) {
  const stateKey = btn.state_key || 'home_' + btn.id
  return state.value.booleans?.[stateKey] ? 'on' : 'off'
}

const buttonStates = computed(() => {
  const states: Record<string, string> = {}
  homeButtons.value.forEach(btn => {
    const stateKey = btn.state_key || 'home_' + btn.id
    let val = (state.value as any)[stateKey]
    if (typeof val === 'string') {
      val = val === 'true' || val === '1'
    } else if (typeof val === 'number') {
      val = val !== 0
    }
    states[btn.id] = val ? 'on' : 'off'
  })
  return states
})


const essClass = computed(() => {
  const m = state.value.ess_mode
  if (!m) return 'off'
  if (m.mode_name === 'Off' || m.mode_name === 'Charger only') return 'off'
  return 'on'
})

const essText = computed(() => {
  const m = state.value.ess_mode
  if (!m) return 'ESS'
  if (m.is_external) return 'External'
  return m.mode_name || 'ESS'
})

const mpptTotal = computed(() => (state.value.mppt_individual || []).reduce((a, b) => a + b, 0))
const tasmotaTotal = computed(() => (state.value.tasmota_individual || []).reduce((a, b) => a + b, 0))

const evCharging = computed(() => {
  const kw = parseFloat(String(state.value.ev_charging_kw)) || 0
  return kw > 0 ? kw.toFixed(1) + 'kW' : '0'
})

const evPower = computed(() => formatPower(state.value.ev_power))

const sortedLoads = computed(() => {
  const loads = state.value.loads || {}
  const uiConfig = state.value.ui_config || {}
  const loadsConfig = uiConfig.loads || {}
  const hiddenLoads = loadsConfig.hidden || ['solar_shed']
  const minWatts = loadsConfig.min_watts || 10
  return Object.entries(loads)
    .filter(([name, v]) => v > minWatts && !hiddenLoads.includes(name))
    .sort((a, b) => b[1] - a[1])
})

const homeButtons = computed(() => {
  const uiConfig = state.value.ui_config || {}
  if (uiConfig.home_buttons) {
    return uiConfig.home_buttons
  }

  const config = appConfig.value
  if (config && config.ha_switch_entities) {
    return Object.entries(config.ha_switch_entities).map(([id, data]) => ({
      id,
      label: data.label || id,
      entity: data.entity
    }))
  }

  return []
})

const waterValveEntity = computed(() => {
  const config = appConfig.value
  return config?.ha_water_valve_entity || 'switch.shutoff_valve'
})

const pumpSwitchEntity = computed(() => {
  const config = appConfig.value
  return config?.ha_pump_switch_entity || 'switch.pump_switch'
})

const headerToggles = computed(() => {
  const uiConfig = state.value.ui_config || {}
  if (uiConfig.header_toggles) {
    return uiConfig.header_toggles
  }

  const config = appConfig.value || defaultConfig
  if (config && config.ha_boolean_entities) {
    return Object.entries(config.ha_boolean_entities).map(([id, entity]) => ({
      id,
      label: id.replace(/_/g, ' ').toUpperCase(),
      entity
    }))
  }

  return [
    { id: 'only_charging', label: 'ONLY CHARGING', entity: 'input_boolean.only_charging' },
    { id: 'no_feed', label: 'NO FEED', entity: 'input_boolean.no_feed' },
    { id: 'house_support', label: 'HOUSE SUPPORT', entity: 'input_boolean.house_support' },
    { id: 'charge_battery', label: 'CHARGE BATTERY', entity: 'input_boolean.charge_battery' },
    { id: 'do_not_supply_charger', label: 'DO NOT SUPPLY EV', entity: 'input_boolean.do_not_supply_charger' },
    { id: 'set_limit_to_ev_charger', label: 'LIMIT TO EV', entity: 'input_boolean.set_limit_to_ev_charger' },
    { id: 'minimize_charging', label: 'MINIMIZE CHARGING', entity: 'input_boolean.minimize_charging' }
  ]
})

const batteries = computed(() => {
  return (state.value.batteries || []).map(b => ({
    name: b.name || 'Battery',
    voltage: b.voltage || 0,
    current: b.current,
    power: b.power,
    soc: b.soc || 0,
    state: b.state || 'Unknown',
    time_to_go: b.time_to_go || ''
  }))
})

const solarSources = computed(() => {
  const sources: Array<{ name: string; pv_voltage?: number; current?: number; power: number }> = []
  ;(state.value.mppt_chargers || []).forEach(m => {
    sources.push({ name: m.name || 'MPPT', pv_voltage: m.pv_voltage || 0, current: m.current || 0, power: m.power || 0 })
  })
  ;(state.value.tasmota_individual || []).forEach((power, i) => {
    sources.push({ name: 'PV Inverter ' + (i + 1), power: power || 0 })
  })
  return sources
})

const dailyStatsHtml = computed(() => {
  const ds = state.value.daily_stats || {}
  const prod = (ds.produced_today || 0).toFixed(2)
  const dollars = (ds.produced_dollars || 0).toFixed(2)
  const grid = (ds.grid_kwh || 0).toFixed(2)
  const gridCost = (parseFloat(grid) * 0.31).toFixed(2)
  const batIn = (ds.battery_in || 0).toFixed(2)
  const batOut = (ds.battery_out || 0).toFixed(2)
  const batInY = (ds.battery_in_yesterday || 0).toFixed(1)
  const batOutY = (ds.battery_out_yesterday || 0).toFixed(1)
  const batDelta = (parseFloat(batIn) - parseFloat(batOut)).toFixed(2)
  const batDeltaY = (parseFloat(batInY) - parseFloat(batOutY)).toFixed(1)
  const tasmotaDaily = ds.tasmota_daily || []
  const mpptDaily = ds.mppt_daily || []
  const pvTotalDaily = ds.pv_total_daily || 0
  let solarParts: string[] = []
  tasmotaDaily.forEach(v => { if (v > 0) solarParts.push(v.toFixed(2)) })
  solarParts.push(pvTotalDaily.toFixed(2) + '(' + mpptDaily.map(v => v.toFixed(2)).join('+') + ')')
  let result = `<span class="highlight">☀️ ${prod}kWh</span> <span class="detail">${solarParts.join('+')}</span> `
  result += `<span class="money">($${dollars})</span> | Grid: ${grid}kWh <span class="money">($${gridCost})</span> | `
  result += `🔋 I: ${batIn}kWh <span class="dim">(${batInY})</span>, O: ${batOut}kWh <span class="dim">(${batOutY})</span>; Δ: ${batDelta}kWh <span class="dim">(${batDeltaY})</span>`
  return result
})

function updateChartOption() {
  const { timestamps, grid, solar, battery, setpoint } = historyData
  const dark = isDark.value
  const textColor = dark ? '#e0e0e0' : '#333'
  const gridColor = dark ? '#444' : '#e0e0e0'

  const timeData = timestamps.map(ts => ts * 1000)

  chartOption.value = {
    animation: false,
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'cross', label: { backgroundColor: '#6a7985' } },
      formatter: function(params) {
        const date = new Date(params[0].value[0])
        const timeStr = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
        let result = `${timeStr}<br/>`
        params.forEach(p => {
          if (p.seriesName === 'Setpoint') return
          const val = Math.floor(p.value[1])
          result += `<span style="display:inline-block;margin-right:5px;border-radius:10px;width:10px;height:10px;background-color:${p.color};"></span>`
          result += `${p.seriesName}: ${val >= 1000 ? (val/1000).toFixed(1) + 'kW' : val + 'W'}<br/>`
        })
        return result
      }
    },
    legend: {
      data: ['Grid', 'Solar', 'Battery', 'Setpoint'],
      top: 0,
      textStyle: { color: textColor }
    },
    grid: {
      top: 30,
      bottom: 50,
      left: 50,
      right: 20,
      containLabel: false
    },
    xAxis: {
      type: 'time',
      axisLine: { lineStyle: { color: gridColor } },
      axisLabel: { color: textColor, formatter: '{HH}:{mm}' },
      splitLine: { show: false }
    },
    yAxis: {
      type: 'value',
      splitLine: { lineStyle: { color: gridColor, type: 'dashed' } },
      axisLabel: { color: textColor, formatter: v => v >= 1000 ? v/1000 + 'kW' : v + 'W' }
    },
    series: [
      {
        name: 'Grid',
        type: 'line',
        smooth: true,
        showSymbol: false,
        data: timeData.map((t, i) => [t, grid[i] || 0]),
        lineStyle: { color: '#4a90d9', width: 2 },
        areaStyle: { color: 'rgba(74,144,217,0.15)' }
      },
      {
        name: 'Solar',
        type: 'line',
        smooth: true,
        showSymbol: false,
        data: timeData.map((t, i) => [t, solar[i] || 0]),
        lineStyle: { color: '#f5a623', width: 2 },
        areaStyle: { color: 'rgba(245,166,35,0.15)' }
      },
      {
        name: 'Battery',
        type: 'line',
        smooth: true,
        showSymbol: false,
        data: timeData.map((t, i) => [t, battery[i] || 0]),
        lineStyle: { color: '#7ed321', width: 2 },
        areaStyle: { color: 'rgba(126,211,33,0.15)' }
      },
      {
        name: 'Setpoint',
        type: 'line',
        smooth: true,
        showSymbol: false,
        data: timeData.map((t, i) => [t, setpoint[i] || 0]),
        lineStyle: { color: '#00d4aa', width: 2, type: 'dashed' },
        areaStyle: { opacity: 0 }
      }
    ]
  }
}

onMounted(async () => {
  try {
    appVersion.value = await getVersion()
  } catch (e) {
    console.error('Failed to get app version:', e)
    appVersion.value = 'unknown'
  }
  connectMqtt()
  nextTick(() => initChart())
  window.addEventListener('resize', () => {
    if (chart && chartEl.value) chart.setSize({ width: chartEl.value.clientWidth, height: 250 })
  })
})

onUnmounted(() => {
  if (pollInterval) clearInterval(pollInterval)
})
</script>
<template>
  <div id="app" class="container-fluid p-2" @contextmenu.prevent="onContextMenu">
    <div class="ws-status">
      <div :class="mqttConnected ? 'connected' : 'disconnected'" style="padding:2px 8px;border-radius:4px">
        <i class="fas" :class="mqttConnected ? 'fa-link' : 'fa-unlink'"></i>
        MQTT {{ mqttConnected ? 'Live' : 'Disconnected' }}
      </div>
    </div>

    <AppHeader
      :dryRun="!!state.dry_run"
      :essClass="essClass"
      :essText="essText"
      :headerToggles="headerToggles"
      :booleans="state.booleans"
      :isDark="isDark"
      @send="send"
      @toggle-theme="toggleTheme"
    />

    <DailyStats/>

    <StatCards
      :gt="state.gt"
      :g1="state.g1"
      :g2="state.g2"
      :tt="state.tt"
      :t1="state.t1"
      :t2="state.t2"
      :solarTotal="state.solar_total"
      :mpptTotal="mpptTotal"
      :tasmotaTotal="tasmotaTotal"
      :batterySoc="state.battery_soc"
      :batteryPower="state.battery_power"
      :batteryVoltage="state.battery_voltage"
      :batteryCurrent="state.battery_current"
      :setpoint="state.setpoint"
      :inverterState="state.inverter_state"
    />

    <div class="row g-2 mb-2">
      <div class="col-md-8">
        <ChartPanel :chartOption="chartOption" />
      </div>
      <SidePanel
        :features="state.features"
        :evCharging="evCharging"
        :evPower="evPower"
        :carSoc="state.car_soc"
        :waterLevel="state.water_level"
        :waterValve="state.water_valve"
        :pumpSwitch="state.pump_switch"
        :pumpSwitchEntity="pumpSwitchEntity"
        :waterValveEntity="waterValveEntity"
        :dishwasherRunning="state.dishwasher_running"
        :dishwasherDuration="state.dishwasher_duration"
        :washerTime="state.washer_time"
        :washerPower="state.washer_power"
        :dryerTime="state.dryer_time"
        :dryerPower="state.dryer_power"
        :homeButtons="homeButtons"
        :buttonStates="buttonStates"
        @send="send"
      />
    </div>

    <BatterySolarPanel
      :batteries="batteries"
      :solarSources="solarSources"
    />

    <LoadsTable
      v-if="state.features?.ha_loads !== false"
      :sortedLoads="sortedLoads"
    />

    <StatusBar
      :haEnabled="haEnabled"
      :haConnected="haConnected"
      :mqttConnected="mqttConnected"
      :uptime="state.uptime"
      :appVersion="appVersion"
      :stateVersion="state.version"
    />

    <ContextMenu
      :show="contextMenu.show"
      :x="contextMenu.x"
      :y="contextMenu.y"
      @open-config="openConfig"
    />
  </div>

<button v-if="isDev" @click='debugMode = !debugMode' style='position:fixed;top:10px;right:10px;z-index:99999;font-size:12px;padding:2px 6px;background:#f0f0f0;border:1px solid #ccc;border-radius:4px;cursor:pointer;'>Debug</button>
<div v-if='isDev && debugMode' style='position:fixed;bottom:10px;left:10px;background:rgba(0,0,0,0.85);color:#0f0;padding:8px;font-size:11px;z-index:99998;max-height:300px;overflow:auto;font-family:monospace;border-radius:4px;'>
  <pre style='margin:0;'>{{ JSON.stringify({ homeButtons, buttonStates, booleans: state.booleans }, null, 2) }}</pre>
</div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { invoke } from '@tauri-apps/api/core'
import { formatPower } from './utils'
import { logger } from './logger'
import { listen } from '@tauri-apps/api/event'
import { useConnection } from './composables/useConnection'
import { useHA } from './composables/useHA'
import { useTheme } from './composables/useTheme'
import { useChart } from './composables/useChart'
import AppHeader from './components/AppHeader.vue'
import StatCards from './components/StatCards.vue'
import ChartPanel from './components/ChartPanel.vue'
import SidePanel from './components/SidePanel.vue'
import BatterySolarPanel from './components/BatterySolarPanel.vue'
import LoadsTable from './components/LoadsTable.vue'
import StatusBar from './components/StatusBar.vue'
import ContextMenu from './components/ContextMenu.vue'
import DailyStats from './components/DailyStats.vue'

const { state, mqttConnected, connectMqtt, ensureNotificationPermission, cleanup: cleanupConnection } = useConnection()
const {
  haEnabled, haConnected, homeButtons, buttonStates,
  headerToggles,
  waterValveEntity, pumpSwitchEntity,
  startHaPolling, stopHaPolling, sendHaOrMqtt, cleanupHa,
} = useHA()
const { isDark, toggleTheme } = useTheme()
const { chartOption, addHistoryPoint } = useChart(isDark)

const debugMode = ref(false)
const isDev = import.meta.env.DEV
const appVersion = ref('')
const contextMenu = ref({ show: false, x: 0, y: 0 })
let unlistenConfig: (() => void) | null = null

function onContextMenu(e: MouseEvent) {
  contextMenu.value = { show: true, x: e.clientX, y: e.clientY }
}

function closeContextMenu() {
  contextMenu.value.show = false
}

async function openConfig() {
  contextMenu.value.show = false
  try {
    await invoke('open_config_window')
  } catch (e) {
    logger.error('Failed to open config window:', e)
  }
}

// Re-export send for template as a thin wrapper that uses HA routing
async function send(action: string, payload: Record<string, unknown> = {}) {
  return sendHaOrMqtt(action, payload)
}

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

const batteries = computed(() => {
  return (state.value.batteries || []).map(b => ({
    name: b.name || 'Battery',
    voltage: b.voltage || 0,
    current: b.current,
    power: b.power,
    soc: b.soc || 0,
    state: b.state || 'Unknown',
    timeToGo: b.time_to_go || ''
  }))
})

const solarSources = computed(() => {
  const sources: Array<{ name: string; pvVoltage?: number; current?: number; power: number }> = []
  ;(state.value.mppt_chargers || []).forEach(m => {
    sources.push({ name: m.name || 'MPPT', pvVoltage: m.pv_voltage || 0, current: m.current || 0, power: m.power || 0 })
  })
  ;(state.value.tasmota_individual || []).forEach((power, i) => {
    sources.push({ name: 'PV Inverter ' + (i + 1), power: power || 0 })
  })
  return sources
})

function onDocumentClick() {
  closeContextMenu()
}

watch(() => state.value, (newState) => {
  if (newState.gt !== undefined) addHistoryPoint(newState)
}, { deep: false })

watch(haEnabled, (newVal) => {
  if (newVal) startHaPolling()
  else stopHaPolling()
})

onMounted(async () => {
  try {
    appVersion.value = await getVersion()
  } catch (e) {
    logger.error('Failed to get app version:', e)
    appVersion.value = 'unknown'
  }
  await ensureNotificationPermission()
  await connectMqtt()
  if (haEnabled.value) startHaPolling()
  document.addEventListener('click', onDocumentClick)

  unlistenConfig = await listen<{color_scheme?: string}>('config-saved', (event) => {
    const scheme = event.payload.color_scheme
    if (scheme) {
      isDark.value = scheme !== 'light'
      document.body.classList.toggle('light', !isDark.value)
      localStorage.setItem('theme', scheme)
    }
  })
})

onUnmounted(() => {
  document.removeEventListener('click', onDocumentClick)
  cleanupConnection()
  cleanupHa()
  if (unlistenConfig) unlistenConfig()
})
</script>
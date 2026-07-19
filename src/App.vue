<template>
  <ErrorBoundary>
    <div
      id="app"
      class="h-screen flex flex-col p-1 select-none overflow-hidden"
      @contextmenu.prevent="onContextMenu"
    >
      <!-- Dashboard Header: Compact buttons and theme switcher -->
      <div class="flex items-center justify-between mb-1">
        <AppHeader
          :dryRun="coerceBool(state.dry_run)"
          :essClass="essClass"
          :essText="essText"
          :headerToggles="headerToggles"
          :toggleStates="headerToggleStates"
          :isDark="isDark"
          :showHeaderToggles="appConfig?.show_header_toggles !== false"
          @send="send"
          @toggle-theme="toggleTheme"
        />
      </div>

      <!-- Dashboard Content: Grid and Panels -->
      <div class="flex-1 overflow-y-auto pr-0.5 flex flex-col gap-1 scrollbar-hide">
        <DailyStats v-if="appConfig?.show_daily_stats !== false" />

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

        <div class="grid grid-cols-1 md:grid-cols-12 gap-1">
          <div class="md:col-span-8 h-[280px]">
            <ChartPanel :chartOption="chartOption" />
          </div>
          <div class="md:col-span-4">
            <SidePanel
              :features="state.features"
              :evCharging="evCharging"
              :evPower="evPower"
              :evPowerWatts="evPowerWatts"
              :evChargingKw="evChargingKw"
              :evLoadPower="evLoadPower"
              :carSoc="state.car_soc"
              :waterLevel="state.water_level"
              :waterValve="waterValveState"
              :pumpSwitch="pumpSwitchState"
              :pumpSwitchEntity="pumpSwitchEntity"
              :waterValveEntity="waterValveEntity"
              :dishwasherRunning="dishwasherRunning"
              :dishwasherDuration="state.dishwasher_duration"
              :washerRunning="washerRunning"
              :washerTime="state.washer_time"
              :washerPower="state.washer_power"
              :dryerRunning="dryerRunning"
              :dryerTime="state.dryer_time"
              :dryerPower="state.dryer_power"
              :homeButtons="homeButtons"
              :buttonStates="buttonStates"
              :haSensors="haSensors"
              :haNumbers="haNumbers"
              :haCovers="haCovers"
              :haMediaPlayers="haMediaPlayers"
              :haScenes="haScenes"
              :haWeather="haWeather"
              :showEv="appConfig?.show_ev !== false"
              :showWasher="appConfig?.show_washer !== false"
              :showDryer="appConfig?.show_dryer !== false"
              :showDishwasher="appConfig?.show_dishwasher !== false"
              :showHomeSection="appConfig?.show_home_section !== false"
              :appConfig="appConfig"
              @send="send"
              @number-set="onNumberSet"
              @cover-position="onCoverPosition"
              @media-control="onMediaControl"
              @scene-activate="onSceneActivate"
            />
          </div>
        </div>

        <BatterySolarPanel
          v-if="appConfig?.show_batteries !== false || appConfig?.show_solar_production !== false"
          :batteries="batteries"
          :solarSources="solarSources"
          :showBatteries="appConfig?.show_batteries !== false"
          :showSolar="appConfig?.show_solar_production !== false"
        />

        <LoadsTable v-if="appConfig?.show_active_loads !== false" :sortedLoads="sortedLoads" />
      </div>

      <!-- Bottom Status Bar: Classic dot layout -->
      <StatusBar
        :haEnabled="haEnabled"
        :haConnected="haConnected"
        :mqttConnected="mqttConnected"
        :haMqttConnected="haMqttConnected"
        :uptime="state.uptime"
        :appVersion="appVersion"
        :stateVersion="state.version"
      />

      <ConsoleLog v-if="appConfig?.show_console !== false" :lines="state.console || []" />

      <ContextMenu
        :show="contextMenu.show"
        :x="contextMenu.x"
        :y="contextMenu.y"
        @open-config="openConfig"
      />

      <!-- Video Popup Overlay -->
      <div
        v-if="videoPopup.show"
        class="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-in fade-in duration-200"
      >
        <div
          class="relative w-full max-w-4xl aspect-video bg-black rounded-lg overflow-hidden shadow-2xl border border-slate-800"
        >
          <!-- Camera Name Header -->
          <div
            class="absolute top-0 left-0 right-0 p-3 bg-gradient-to-b from-black/80 to-transparent z-10 flex justify-between items-center"
          >
            <div class="flex items-center gap-2">
              <div class="w-2 h-2 rounded-full bg-red-500 animate-pulse"></div>
              <span class="text-xs font-bold text-white uppercase tracking-widest"
                >LIVE: {{ videoPopup.cameraName }}</span
              >
            </div>
            <button
              type="button"
              @click="videoPopup.show = false"
              class="p-1.5 rounded-full bg-white/10 text-white hover:bg-red-500 transition-colors"
            >
              <X :size="20" />
            </button>
          </div>

          <video autoplay controls class="w-full h-full" :src="videoPopup.url">
            <track kind="captions" />
            Your browser does not support the video tag.
          </video>
        </div>
      </div>

      <!-- Auth Screen Overlay -->
      <AuthScreen v-if="showAuthScreen" @authenticated="handleAuthenticated" />
    </div>
  </ErrorBoundary>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { invoke } from '@tauri-apps/api/core'
import { formatPower } from './utils'
import { logger } from './logger'
import { listen } from '@tauri-apps/api/event'
import { X } from '@lucide/vue'
import { useConnection, notify } from './composables/useConnection'
import { useHA } from './composables/useHA'
import { useTheme } from './composables/useTheme'
import { useChart, addHistoryPoint } from './composables/useChart'
import AppHeader from './components/AppHeader.vue'
import StatCards from './components/StatCards.vue'
import ChartPanel from './components/ChartPanel.vue'
import SidePanel from './components/SidePanel.vue'
import BatterySolarPanel from './components/BatterySolarPanel.vue'
import LoadsTable from './components/LoadsTable.vue'
import StatusBar from './components/StatusBar.vue'
import ContextMenu from './components/ContextMenu.vue'
import DailyStats from './components/DailyStats.vue'
import ConsoleLog from './components/ConsoleLog.vue'
import AuthScreen from './components/AuthScreen.vue'
import ErrorBoundary from './components/ErrorBoundary.vue'
import { initSystemNotifications } from './composables/useSystemNotifications'

const {
  state,
  mqttConnected,
  haMqttConnected,
  appConfig,
  connectMqtt,
  ensureNotificationPermission,
  cleanup: cleanupConnection,
} = useConnection()
const {
  haEnabled,
  haConnected,
  haEntityStates,
  haEntityAttributes,
  homeButtons,
  buttonStates,
  headerToggles,
  headerToggleStates,
  waterValveEntity,
  pumpSwitchEntity,
  waterValveState,
  pumpSwitchState,
  haSensors,
  haNumbers,
  haCovers,
  haMediaPlayers,
  haScenes,
  haWeather,
  dishwasherRunning,
  washerRunning,
  dryerRunning,
  coerceBool,
  initHa,
  sendHaOrMqtt,
  cleanupHa,
  setWindowHidden,
} = useHA()
const { isDark, toggleTheme } = useTheme()
const { chartOption, forceUpdateChart } = useChart(isDark)

const appVersion = ref('')
const contextMenu = ref({ show: false, x: 0, y: 0 })
const videoPopup = ref({ show: false, url: '', cameraName: '' })
const authToken = ref<string | null>(null)
const showAuthScreen = ref(false)
let unlistenConfig: (() => void) | null = null
let unlistenWindowEvents: (() => void) | null = null

function handleAuthenticated(token: string) {
  authToken.value = token
  showAuthScreen.value = false
  // Store token in session
  sessionStorage.setItem('auth_token', token)
}

function onContextMenu(e: MouseEvent) {
  contextMenu.value = { show: true, x: e.clientX, y: e.clientY }
}

function closeContextMenu() {
  contextMenu.value.show = false
}

function handleShowVideoPopup(e: Event) {
  const customEvent = e as CustomEvent
  if (customEvent.detail) {
    const data = customEvent.detail
    if (data && typeof data === 'object') {
      videoPopup.value = {
        show: true,
        url: data.video_url,
        cameraName: data.agent_name || 'Camera',
      }
    } else {
      videoPopup.value = {
        show: true,
        url: data,
        cameraName: 'Camera',
      }
    }
  }
}

async function openConfig() {
  contextMenu.value.show = false
  try {
    await invoke('open_config_window')
  } catch (e) {
    logger.error('Failed to open config window:', e)
  }
}

async function send(action: string, payload: Record<string, unknown> = {}) {
  return sendHaOrMqtt(action, payload)
}

async function onNumberSet(entityId: string, value: number) {
  await send('number_set', { entity: entityId, value })
}

async function onCoverPosition(entityId: string, position: number) {
  await send('set_cover_position', { entity: entityId, position })
}

async function onMediaControl(entityId: string, action: string) {
  await send('media_player', { entity: entityId, mp_action: action })
}

async function onSceneActivate(entityId: string) {
  await send('scene_activate', { entity: entityId })
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

const mpptTotal = computed(() => state.value.mppt_total || 0)
const tasmotaTotal = computed(() => state.value.tasmota_total || 0)

const evCharging = computed(() => {
  const kw = parseFloat(String(state.value.ev_charging_kw)) || 0
  return kw > 0 ? kw.toFixed(1) + 'kW' : '0'
})

const evPower = computed(() => formatPower(state.value.ev_power))
const evPowerWatts = computed(() => Math.abs(state.value.ev_power || 0))
const evChargingKw = computed(() => Number.parseFloat(String(state.value.ev_charging_kw)) || 0)
const evLoadPower = computed(() => {
  const loads = state.value.loads
  if (!loads) return 0
  for (const [key, val] of Object.entries(loads)) {
    if (key.toLowerCase().includes('ev') || key.toLowerCase().includes('charger')) return val
  }
  return 0
})

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
  return (state.value.batteries || []).map((b) => ({
    name: b.name || 'Battery',
    voltage: b.voltage || 0,
    current: b.current,
    power: b.power,
    soc: b.soc || 0,
    state: b.state || 'Unknown',
    timeToGo: b.time_to_go || '',
  }))
})

const solarSources = computed(() => {
  const sources: Array<{ name: string; pvVoltage?: number; current?: number; power: number }> = []
  ;(state.value.mppt_chargers || []).forEach((m) => {
    sources.push({
      name: m.name || 'MPPT',
      pvVoltage: m.pv_voltage || 0,
      current: m.current || 0,
      power: m.power || 0,
    })
  })
  ;(state.value.tasmota_individual || []).forEach((power, i) => {
    sources.push({ name: 'PV Inverter ' + (i + 1), power: power || 0 })
  })
  return sources
})

function onDocumentClick() {
  closeContextMenu()
}

watch(
  () => isDark.value,
  () => {
    forceUpdateChart()
  }
)

watch(
  () => state.value,
  (newState) => {
    if (newState.gt !== undefined) addHistoryPoint(newState)
  },
  { deep: false }
)

onMounted(async () => {
  try {
    appVersion.value = await getVersion()
  } catch (e) {
    logger.error('Failed to get app version:', e)
    appVersion.value = 'unknown'
  }
  await ensureNotificationPermission()
  notify('Inverter Desktop', 'App started')

  // Check if authentication is enabled
  try {
    const cfg = appConfig.value
    if (cfg?.auth_enabled) {
      // Check for existing session
      const storedToken = sessionStorage.getItem('auth_token')
      if (storedToken) {
        const valid = await invoke<boolean>('auth_check', { token: storedToken })
        if (valid) {
          authToken.value = storedToken
        } else {
          sessionStorage.removeItem('auth_token')
          showAuthScreen.value = true
        }
      } else {
        showAuthScreen.value = true
      }
    }
  } catch (e) {
    logger.warn('Auth check failed:', e)
  }

  await connectMqtt()
  await initHa()
  initSystemNotifications(haEntityStates)
  document.addEventListener('click', onDocumentClick)
  globalThis.addEventListener('show-video-popup', handleShowVideoPopup)

  unlistenConfig = await listen<{ color_scheme?: string }>('config-saved', async (event) => {
    const scheme = event.payload.color_scheme
    if (scheme) {
      isDark.value = scheme !== 'light'
      document.documentElement.classList.toggle('dark', isDark.value)
      localStorage.setItem('theme', scheme)
    }
    await connectMqtt()
    haEntityStates.value = {}
    haEntityAttributes.value = {}
  })

  // Pause HA updates only when window is minimized/closed (not when it loses focus)
  const unlistenHidden = await listen('window-hidden', () => setWindowHidden(true))
  const unlistenShown = await listen('window-shown', () => setWindowHidden(false))

  unlistenWindowEvents = () => {
    unlistenHidden()
    unlistenShown()
  }
})

onUnmounted(() => {
  document.removeEventListener('click', onDocumentClick)
  globalThis.removeEventListener('show-video-popup', handleShowVideoPopup)
  cleanupConnection()
  cleanupHa()
  if (unlistenConfig) unlistenConfig()
  if (unlistenWindowEvents) unlistenWindowEvents()
})
</script>

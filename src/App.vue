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

        <NotificationBanner />

        <StatCards
          :gt="state.gt"
          :g1="state.g1"
          :g2="state.g2"
          :tt="state.tt"
          :t1="state.t1"
          :t2="state.t2"
          :solarTotal="state.solar_total"
          :mpptTotal="mpptTotal"
          :pvInvertersTotal="pvInvertersTotal"
          :batterySoc="state.battery_soc"
          :batteryPower="state.battery_power"
          :batteryVoltage="state.battery_voltage"
          :batteryCurrent="state.battery_current"
          :setpoint="state.setpoint"
          :inverterState="state.inverter_state"
        />

        <div class="grid grid-cols-1 md:grid-cols-12 gap-1 md:auto-rows-fr">
          <div class="md:col-span-8 h-[280px] md:h-auto md:min-h-[280px]">
            <ChartPanel :chartOption="chartOption" />
          </div>
          <div class="md:col-span-4">
            <SidePanel
              :features="state.features"
              :showEv="appConfig?.show_ev !== false"
              :evSectionVisible="evSectionVisible"
              :carSoc="evSoc"
              :carChargingPower="evChargingKw != null ? evChargingKw * 1000 : null"
              :evChargingPower="evPowerWatts"
              :waterVisible="waterSectionVisible"
              :waterValve="waterValveState"
              :pumpSwitch="pumpSwitchState"
              :waterLevel="waterLevel"
              :waterPumpMode="waterPumpMode"
              :waterValveMode="waterValveMode"
              :washerActive="washerActive"
              :washerRemainingTime="washerRemainingTime"
              :dryerActive="dryerActive"
              :dryerRemainingTime="dryerRemainingTime"
              :washerStartEntity="washerStartEntity"
              :washerPauseEntity="washerPauseEntity"
              :dryerStartEntity="dryerStartEntity"
              :dryerPauseEntity="dryerPauseEntity"
              :dishwasherActive="dishwasherActive"
              :dishwasherRemainingTime="dishwasherRemainingTime"
              :homeButtons="homeButtons"
              :buttonStates="buttonStates"
              :haSensors="haSensors"
              :haNumbers="haNumbers"
              :haCovers="haCovers"
              :haMediaPlayers="haMediaPlayers"
              :haScenes="haScenes"
              :haWeather="haWeather"
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

        <LoadsTable v-if="appConfig?.show_active_loads !== false" :loads="acloads" />
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
        @check-updates="checkForUpdates"
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

      <!-- Toast Notification -->
      <div
        v-if="message"
        class="fixed bottom-4 left-1/2 -translate-x-1/2 z-[60] px-4 py-1.5 rounded-full shadow-lg text-[10px] font-bold border animate-in slide-in-from-bottom duration-200 uppercase tracking-wider"
        :class="
          messageType === 'error'
            ? 'bg-red-500 border-red-600 text-white'
            : 'bg-green-500 border-green-600 text-white'
        "
      >
        {{ message }}
      </div>
    </div>
  </ErrorBoundary>
</template>

<script setup lang="ts">
import { X } from '@lucide/vue'
import { getVersion } from '@tauri-apps/api/app'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import AppHeader from './components/AppHeader.vue'
import AuthScreen from './components/AuthScreen.vue'
import BatterySolarPanel from './components/BatterySolarPanel.vue'
import ChartPanel from './components/ChartPanel.vue'
import ConsoleLog from './components/ConsoleLog.vue'
import ContextMenu from './components/ContextMenu.vue'
import DailyStats from './components/DailyStats.vue'
import ErrorBoundary from './components/ErrorBoundary.vue'
import LoadsTable from './components/LoadsTable.vue'
import NotificationBanner from './components/NotificationBanner.vue'
import SidePanel from './components/SidePanel.vue'
import StatCards from './components/StatCards.vue'
import StatusBar from './components/StatusBar.vue'
import { checkForUpdates, checkForUpdatesSilent } from './composables/useAutoUpdate'
import { addHistoryPoint, useChart } from './composables/useChart'
import { notify, useConnection } from './composables/useConnection'
import { useHA } from './composables/useHA'
import { useMQTTState } from './composables/useMQTTState'
import { initSystemNotifications } from './composables/useSystemNotifications'
import { useTheme } from './composables/useTheme'
import { getAppConfig } from './config'
import { logger } from './logger'

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
  haSensors,
  haNumbers,
  haCovers,
  haMediaPlayers,
  haScenes,
  haWeather,
  washerActive,
  washerRemainingTime,
  dryerActive,
  dryerRemainingTime,
  washerStartEntity,
  washerPauseEntity,
  dryerStartEntity,
  dryerPauseEntity,
  dishwasherActive,
  dishwasherRemainingTime,
  coerceBool,
  initHa,
  sendHaOrMqtt,
  cleanupHa,
  setWindowHidden,
} = useHA()
const {
  waterLevel,
  pumpSwitchState,
  waterValveState,
  waterPumpMode,
  waterValveMode,
  waterSectionVisible,
  evSoc,
  evChargingKw,
  evPowerWatts,
  evSectionVisible,
  acloads,
} = useMQTTState()
const { isDark, toggleTheme } = useTheme()
const { chartOption, forceUpdateChart, setChartPaused } = useChart(isDark)
const isWindowHidden = ref(false)

const appVersion = ref('')
const contextMenu = ref({ show: false, x: 0, y: 0 })
const videoPopup = ref({ show: false, url: '', cameraName: '' })
const authToken = ref<string | null>(null)
const showAuthScreen = ref(false)
const message = ref('')
const messageType = ref<'success' | 'error'>('success')
let unlistenConfig: (() => void) | null = null
let unlistenWindowEvents: (() => void) | null = null

function clearMessage() {
  message.value = ''
}

function showError(msg: string) {
  message.value = msg
  messageType.value = 'error'
  setTimeout(clearMessage, 3000)
}
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
    showError(`Failed to open config: ${e?.toString() || e}`)
  }
}

async function send(action: string, payload: Record<string, unknown> = {}) {
  try {
    await sendHaOrMqtt(action, payload)
  } catch (e) {
    logger.error('Action failed:', action, payload, e)
    showError(`Failed: ${e?.toString() || e}`)
  }
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

const isInverterOff = computed(() => {
  const s = state.value.inverter_state
  if (!s) return false
  const normalized = s.trim().toLowerCase()
  return normalized === 'off'
})

const essClass = computed(() => {
  if (isInverterOff.value) return 'off'
  const m = state.value.ess_mode
  if (!m) return 'off'
  if (m.mode_name === 'Off' || m.mode_name === 'Charger only') return 'off'
  return 'on'
})

const essText = computed(() => {
  if (isInverterOff.value) return 'Off'
  const m = state.value.ess_mode
  if (!m) return 'ESS'
  if (m.mode_name === 'Off' || m.mode_name === 'Charger only') return m.mode_name
  if (m.is_external) return 'External'
  return m.mode_name || 'ESS'
})

const mpptTotal = computed(() => state.value.mppt_total || 0)
const pvInvertersTotal = computed(() => {
  const invs = state.value.pv_inverters
  if (invs?.length) {
    return invs.reduce((sum, p) => sum + (p.power || 0), 0)
  }
  // Fallback: legacy daemon publishes per-inverter power aggregates
  return (state.value.pv_inverter_individual || []).reduce((sum, p) => sum + (p || 0), 0)
})

const batteries = computed(() => {
  const tiles: Array<{
    name: string
    serial?: string
    instance?: number
    voltage: number
    current?: number
    power?: number
    soc: number
    state: string
    timeToGo?: string
  }> = []
  for (const b of state.value.batteries || []) {
    tiles.push({
      name: b.name || 'Battery',
      serial: b.serial,
      instance: b.instance,
      voltage: b.voltage || 0,
      current: b.current,
      power: b.power,
      soc: b.soc || 0,
      state: b.state || 'Unknown',
      timeToGo: b.time_to_go || '',
    })
  }
  return tiles
})

const solarSources = computed(() => {
  const sources: Array<{
    name: string
    serial?: string
    instance?: number
    pvVoltage?: number
    current?: number
    power: number
  }> = []
  ;(state.value.mppt_chargers || []).forEach((m) => {
    sources.push({
      name: m.name || 'MPPT',
      serial: m.serial,
      instance: m.instance,
      pvVoltage: m.pv_voltage || 0,
      current: m.current || 0,
      power: m.power || 0,
    })
  })
  const pvInvs = state.value.pv_inverters
  if (pvInvs?.length) {
    pvInvs.forEach((p, i) => {
      sources.push({
        name: p.name || `PV Inverter ${i + 1}`,
        serial: p.serial,
        instance: p.instance,
        pvVoltage: p.voltage,
        current: p.current,
        power: p.power || 0,
      })
    })
  } else if (state.value.pv_inverter_individual?.length) {
    // Fallback: daemon publishes per-inverter power aggregates as Vec<f64>
    state.value.pv_inverter_individual.forEach((power, i) => {
      sources.push({
        name: `PV Inverter ${i + 1}`,
        power,
      })
    })
  }
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
    if (isWindowHidden.value) return
    if (newState.gt !== undefined) addHistoryPoint(newState)
  },
  { deep: true }
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

  // Load configuration first to check auth status
  let cfg = appConfig.value
  if (!cfg) {
    try {
      cfg = await getAppConfig()
      appConfig.value = cfg
    } catch (e) {
      logger.warn('Failed to load config for auth check:', e)
    }
  }

  // Check if authentication is enabled
  try {
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
  initSystemNotifications(
    haEntityStates,
    haEntityAttributes,
    evChargingKw,
    waterValveState,
    pumpSwitchState
  )

  // Check for updates on startup (silent check)
  checkForUpdatesSilent().catch((e) => logger.warn('Update check failed:', e))

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

  // Pause updates and charts when window is minimized/closed to tray
  const unlistenHidden = await listen('window-hidden', () => {
    isWindowHidden.value = true
    setChartPaused(true)
    setWindowHidden(true)
  })
  const unlistenShown = await listen('window-shown', () => {
    isWindowHidden.value = false
    setChartPaused(false)
    setWindowHidden(false)
  })

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

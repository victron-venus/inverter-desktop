import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { computed, type Ref, ref, watch, watchEffect } from 'vue'
import type { AppConfig } from '../config'
import { logger } from '../logger'
import type {
  HaCoverDisplay,
  HaFilteredData,
  HaMediaPlayerDisplay,
  HaNumberDisplay,
  HaSceneDisplay,
  HaSensorDisplay,
  HaWeatherDisplay,
} from '../types/ha'
import { formatPower, isInverterControlFlag, resolveHeaderToggleState } from '../utils'
import { appConfig, type InverterState, state } from './useInverterState'

function coerceBool(v: unknown): boolean {
  return v === true || v === 1 || v === 'true' || v === '1' || v === 'on' || v === 'online'
}

function configuredSectionEntities(appConfig: Ref<AppConfig | null>): string[] {
  const cfg = appConfig.value
  const ids: string[] = []
  const singles = [
    cfg?.ha_dryer_entity,
    cfg?.ha_washer_entity,
    cfg?.ha_washer_start_entity,
    cfg?.ha_washer_pause_entity,
    cfg?.ha_dryer_start_entity,
    cfg?.ha_dryer_pause_entity,
    cfg?.ha_dishwasher_running_entity,
    cfg?.ha_dishwasher_duration_entity,
  ]
  const all = [
    ...singles,
    ...(cfg?.ha_consumption_clamps || []),
    ...(cfg?.ha_generation_clamps || []),
  ]
  for (const v of all) {
    const t = (v || '').trim()
    if (t && !ids.includes(t)) ids.push(t)
  }
  return ids
}

function hasNonZeroTime(time: string | null): boolean {
  if (time === null) return false
  return ![...time.replace(/\D/g, '')].every((c) => c === '0')
}

const loadNameCache = new Map<string, string>()
function getFormattedLoadName(key: string): string {
  let cached = loadNameCache.get(key)
  if (!cached) {
    cached = key.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase())
    loadNameCache.set(key, cached)
  }
  return cached
}

export function useHA() {
  const haEntityStates = ref<Record<string, string>>({})
  const haEntityAttributes = ref<Record<string, Record<string, unknown>>>({})
  const haWsConnected = ref(false)
  let unlistenHaUpdate: (() => void) | null = null
  let unlistenHaConn: (() => void) | null = null
  let unlistenHaFiltered: (() => void) | null = null

  // Grace period: retain previous entity states for 15 seconds during
  // transient WebSocket reconnects to prevent UI flicker / entity blinking.
  const HA_GRACE_PERIOD_MS = 15_000
  let haGraceTimer: ReturnType<typeof setTimeout> | null = null

  // Pre-filtered HA entity data from Rust (replaces 6 computed properties)
  const haSensors = ref<HaSensorDisplay[]>([])
  const haNumbers = ref<HaNumberDisplay[]>([])
  const haCovers = ref<HaCoverDisplay[]>([])
  const haMediaPlayers = ref<HaMediaPlayerDisplay[]>([])
  const haScenes = ref<HaSceneDisplay[]>([])
  const haWeather = ref<HaWeatherDisplay | null>(null)

  const haEnabled = computed(() => {
    const cfg = appConfig.value
    return !!(cfg?.ha_use_direct_api && cfg.ha_url && cfg.ha_longlived_token)
  })

  /** Entity IDs tracked for dashboard sections (appliances, water, EV, loads clamps) */
  async function checkHaConnection() {
    const cfg = appConfig.value
    if (!cfg?.ha_url || !cfg?.ha_longlived_token) {
      haWsConnected.value = false
      return
    }
    try {
      await invoke('test_ha_connection', {
        url: cfg.ha_url,
        port: cfg.ha_port || 8123,
        token: cfg.ha_longlived_token,
      })
      haWsConnected.value = true
    } catch {
      haWsConnected.value = false
    }
  }

  const haConnected = computed(() => {
    const cfg = appConfig.value
    if (cfg?.ha_use_direct_api && cfg.ha_url && cfg.ha_longlived_token) {
      return haWsConnected.value
    }
    return !!state.value.ha_connected
  })

  function storeEntityStates(
    states: Array<{ entity_id: string; state: string; attributes?: Record<string, unknown> }>
  ) {
    for (const s of states) {
      haEntityStates.value = { ...haEntityStates.value, [s.entity_id]: s.state }
      if (s.attributes) {
        haEntityAttributes.value = { ...haEntityAttributes.value, [s.entity_id]: s.attributes }
      }
    }
  }

  async function fetchHaStates() {
    const cfg = appConfig.value
    if (!cfg?.ha_url || !cfg?.ha_longlived_token) return
    try {
      const states = await invoke<
        Array<{
          entity_id: string
          state: string
          attributes?: Record<string, unknown>
        }>
      >('get_ha_appliance_states', {
        url: cfg.ha_url,
        port: cfg.ha_port || 8123,
        token: cfg.ha_longlived_token,
      })
      storeEntityStates(states)
    } catch (e) {
      logger.warn('Failed to fetch HA states:', e)
    }
  }

  /** Fetch current state for specific entity IDs (used for buttons/switches) */
  async function fetchHaEntityStates(entityIds: string[]) {
    const cfg = appConfig.value
    if (!cfg?.ha_url || !cfg?.ha_longlived_token || entityIds.length === 0) return
    try {
      const states = await invoke<
        Array<{
          entity_id: string
          state: string
          attributes?: Record<string, unknown>
        }>
      >('get_ha_entity_states', {
        url: cfg.ha_url,
        port: cfg.ha_port || 8123,
        token: cfg.ha_longlived_token,
        entityIds,
      })
      storeEntityStates(states)
    } catch (e) {
      logger.warn('Failed to fetch HA entity states:', e)
    }
  }

  let windowHidden = false

  async function setWindowHidden(hidden: boolean) {
    windowHidden = hidden
    try {
      await invoke('set_window_hidden', { hidden })
      if (!hidden) {
        fetchHaStates()
        const applianceEntities = configuredSectionEntities(appConfig)
        if (applianceEntities.length > 0) fetchHaEntityStates(applianceEntities)
        const initial = await invoke<InverterState>('get_state')
        if (initial) {
          state.value = initial
        }
      }
    } catch (e) {
      logger.error('Failed to sync window state:', e)
    }
  }

  async function initHa() {
    unlistenHaUpdate = await listen<{
      entity_id: string
      state: string
      attributes?: Record<string, unknown>
    }>('ha-state-update', (event) => {
      if (windowHidden) return
      const { entity_id, state: st, attributes } = event.payload
      haEntityStates.value = { ...haEntityStates.value, [entity_id]: st }
      if (attributes) {
        haEntityAttributes.value = { ...haEntityAttributes.value, [entity_id]: attributes }
      }
    })

    // Pre-filtered HA entity data from Rust (replaces 6 frontend computed properties)
    unlistenHaFiltered = await listen<HaFilteredData>('ha-filtered-update', (event) => {
      if (windowHidden) return
      const data = event.payload
      haSensors.value = data.sensors
      haNumbers.value = data.numbers
      haCovers.value = data.covers
      haMediaPlayers.value = data.media_players
      haScenes.value = data.scenes
      haWeather.value = data.weather
    })

    unlistenHaConn = await listen<boolean>('ha-connection-status', (event) => {
      if (event.payload) {
        // Reconnected — cancel any pending grace timer and keep states
        if (haGraceTimer) {
          clearTimeout(haGraceTimer)
          haGraceTimer = null
        }
        haWsConnected.value = true
        // On connect, fetch full state so buttons show correct state
        fetchHaStates()
      } else {
        // Disconnected — start grace period before clearing states
        haWsConnected.value = false
        if (!haGraceTimer) {
          haGraceTimer = setTimeout(() => {
            haGraceTimer = null
            haEntityStates.value = {}
            haEntityAttributes.value = {}
          }, HA_GRACE_PERIOD_MS)
        }
      }
    })

    // Fetch initial state on mount
    await fetchHaStates()

    // Inverter-control flags (only_charging, …) live on Cerbo MQTT
    // inverter/state.booleans — do not fetch them from HA REST.

    // Fetch dashboard section entities (washer/dryer/dishwasher/water/EV/clamps) immediately
    const applianceEntities = configuredSectionEntities(appConfig)
    if (applianceEntities.length > 0) {
      await fetchHaEntityStates(applianceEntities)
    }

    // Check HA connection status via HTTP
    await checkHaConnection()

    // Watch for config/state changes to fetch dynamic entity IDs (home buttons, header toggles)
    watch(
      [appConfig, () => state.value.ui_config],
      () => {
        if (!haEnabled.value) return
        const ids = new Set<string>()
        // Dashboard section entities (washer/dryer/dishwasher/water/EV/clamps)
        for (const entity of configuredSectionEntities(appConfig)) ids.add(entity)
        // Header toggles from config or ui_config
        const toggles =
          appConfig.value?.header_toggles_config || state.value.ui_config?.header_toggles || []
        for (const t of toggles) {
          // Control flags are MQTT-only; skip HA REST fetches for them.
          if (t.entity && !isInverterControlFlag(t.entity) && !isInverterControlFlag(t.id)) {
            ids.add(t.entity)
          }
        }
        // Home buttons from config or ui_config
        const buttons =
          appConfig.value?.ha_entities?.filter((e) => e.enabled).map((e) => e.entity) ||
          state.value.ui_config?.home_buttons?.map((b) => b.entity) ||
          []
        for (const b of buttons) {
          if (b && !isInverterControlFlag(b)) ids.add(b)
        }
        if (ids.size > 0) {
          fetchHaEntityStates([...ids])
        }
      },
      { deep: true, immediate: false }
    )
  }

  // Water comes exclusively from Cerbo GX MQTT (published by dbus-pump);
  // pump/valve control lives in dbus-pump itself - no HA fallback, no toggles.
  const waterValveState = computed(() => state.value.water_valve ?? null)

  const pumpSwitchState = computed(() => state.value.pump_switch ?? null)

  const waterLevel = computed(() => state.value.water_level ?? null)

  const waterPumpMode = computed(() => state.value.water_pump_mode ?? null)

  const waterValveMode = computed(() => state.value.water_valve_mode ?? null)

  const waterSectionVisible = computed(
    () =>
      state.value.water_level != null ||
      state.value.water_valve != null ||
      state.value.pump_switch != null
  )

  // EV metrics now come from the GX via MQTT (dbus-ev / dbus-evcharger),
  // published on N/<portal>/ev/<instance>/Soc, /Ac/Power and
  // N/<portal>/evcharger/<instance>/Ac/Power. No Home Assistant required.
  const evSoc = computed(() => {
    const v = state.value.car_soc
    if (v == null) return null
    return Math.max(0, Math.min(100, v))
  })

  /** EV car charging power in watts (from N/<portal>/ev/<i>/Ac/Power) */
  const evChargingWatts = computed(() => state.value.car_charging_power ?? null)

  const evClampWatts = computed(() => {
    const v = state.value.ev_charging_power
    if (v == null) return null
    return Math.abs(v)
  })

  const evChargingKw = computed(() => {
    const w = evChargingWatts.value
    return w === null ? null : w / 1000
  })

  const evPowerWatts = computed(() => evClampWatts.value)

  const evPower = computed(() => {
    const w = evClampWatts.value
    if (w === null) return ''
    return formatPower(w)
  })

  // Latch: once any ev/evcharger MQTT message is seen, never hide the card.
  // Matches BATTERIES/SOLAR behaviour (chrome-gated by config, not live data).
  // Does NOT unlatch on mqttConnected false.
  const evLatch = ref(false)
  watchEffect(() => {
    if (state.value.ev_present || state.value.evcharger_present) {
      evLatch.value = true
    }
  })

  const evSectionVisible = computed(() => evLatch.value)

  /** Active loads strictly from Cerbo DBus -> MQTT (state.value.loads), zero HA fallback */
  const haLoads = computed(() => {
    const mqttLoads = state.value.loads
    if (!mqttLoads || Object.keys(mqttLoads).length === 0) {
      return []
    }
    const items: Array<{ name: string; value: number; isGeneration: boolean }> = []
    for (const [key, val] of Object.entries(mqttLoads)) {
      const v = typeof val === 'number' ? val : Number(val)
      if (!Number.isNaN(v) && Math.abs(v) > 2) {
        items.push({
          name: getFormattedLoadName(key),
          value: v,
          isGeneration: v < 0,
        })
      }
    }
    items.sort((a, b) => {
      const absDiff = Math.abs(b.value) - Math.abs(a.value)
      if (absDiff !== 0) return absDiff
      // tie-break by name alphabetically (case-insensitive)
      return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
    })
    return items
  })

  const dishwasherActive = computed(() => {
    if (!haEnabled.value) return false
    const entity = (appConfig.value?.ha_dishwasher_running_entity || '').trim()
    if (!entity) return false
    const stateVal = haEntityStates.value[entity]
    if (stateVal === undefined || stateVal === null) return false
    const lower = String(stateVal).trim().toLowerCase()
    if (!lower || lower === 'unavailable' || lower === 'unknown') return false
    return lower === 'on' || lower === 'running'
  })

  const dishwasherRemainingTime = computed(() => {
    if (!haEnabled.value) return null
    const entity = (appConfig.value?.ha_dishwasher_duration_entity || '').trim()
    if (!entity) return null
    const stateVal = haEntityStates.value[entity]
    if (!stateVal) return null
    const val = String(stateVal).trim()
    const lower = val.toLowerCase()
    if (
      !lower ||
      lower === 'unavailable' ||
      lower === 'unknown' ||
      lower === 'off' ||
      lower === 'idle'
    )
      return null
    return val
  })

  const haDryerEntity = computed(() => (appConfig.value?.ha_dryer_entity || '').trim())

  const haWasherEntity = computed(() => (appConfig.value?.ha_washer_entity || '').trim())

  function remainingTimeFromState(entity: string): string | null {
    const stateVal = haEntityStates.value[entity]
    if (!stateVal) return null
    const val = String(stateVal).trim()
    const lower = val.toLowerCase()
    if (
      !lower ||
      lower === 'unavailable' ||
      lower === 'unknown' ||
      lower === 'off' ||
      lower === 'idle'
    )
      return null
    return val
  }

  const dryerRemainingTime = computed(() => {
    if (!haEnabled.value) return null
    if (!haDryerEntity.value) return null
    return remainingTimeFromState(haDryerEntity.value)
  })

  const dryerActive = computed(() => hasNonZeroTime(dryerRemainingTime.value))

  const washerRemainingTime = computed(() => {
    if (!haEnabled.value) return null
    if (!haWasherEntity.value) return null
    return remainingTimeFromState(haWasherEntity.value)
  })

  const washerActive = computed(() => hasNonZeroTime(washerRemainingTime.value))

  const haWasherStartEntity = computed(() => (appConfig.value?.ha_washer_start_entity || '').trim())
  const haWasherPauseEntity = computed(() => (appConfig.value?.ha_washer_pause_entity || '').trim())
  const haDryerStartEntity = computed(() => (appConfig.value?.ha_dryer_start_entity || '').trim())
  const haDryerPauseEntity = computed(() => (appConfig.value?.ha_dryer_pause_entity || '').trim())

  const homeButtons = computed(() => {
    const cfg = appConfig.value
    if (cfg?.ha_entities && cfg.ha_entities.length > 0) {
      return cfg.ha_entities
        .filter((e): e is typeof e & { enabled: true } => e.enabled)
        .map((e) => ({
          id: e.id,
          label: e.label,
          entity: e.entity,
          state_key: (e as { state_key?: string }).state_key,
        }))
    }
    const uiConfig = state.value.ui_config || {}
    if (uiConfig.home_buttons) return uiConfig.home_buttons
    return []
  })

  const headerToggles = computed(() => {
    const cfg = appConfig.value
    if (cfg?.header_toggles_config && cfg.header_toggles_config.length > 0) {
      return cfg.header_toggles_config
    }
    const uiConfig = state.value.ui_config || {}
    if (uiConfig.header_toggles) return uiConfig.header_toggles
    return [
      { id: 'only_charging', label: 'ONLY CHARGING', entity: 'input_boolean.only_charging' },
      { id: 'no_feed', label: 'NO FEED', entity: 'input_boolean.no_feed' },
      { id: 'house_support', label: 'HOUSE SUPPORT', entity: 'input_boolean.house_support' },
      { id: 'charge_battery', label: 'CHARGE BATTERY', entity: 'input_boolean.charge_battery' },
      {
        id: 'do_not_supply_charger',
        label: 'DO NOT SUPPLY EV',
        entity: 'input_boolean.do_not_supply_charger',
      },
      {
        id: 'set_limit_to_ev_charger',
        label: 'LIMIT TO EV',
        entity: 'input_boolean.set_limit_to_ev_charger',
      },
      {
        id: 'minimize_charging',
        label: 'MINIMIZE CHARGING',
        entity: 'input_boolean.minimize_charging',
      },
    ]
  })

  const buttonStates = computed(() => {
    const states: Record<string, string> = {}
    homeButtons.value.forEach(
      (btn: { id: string; label: string; entity: string; state_key?: string }) => {
        if (
          !isInverterControlFlag(btn.entity) &&
          !isInverterControlFlag(btn.id) &&
          haEnabled.value &&
          haEntityStates.value[btn.entity] !== undefined
        ) {
          states[btn.id] = haEntityStates.value[btn.entity] === 'on' ? 'on' : 'off'
        } else {
          const stateKey = btn.state_key || `home_${btn.id}`
          let val = state.value.booleans?.[stateKey]
          if (typeof val === 'string') val = val === 'true' || val === '1'
          else if (typeof val === 'number') val = val !== 0
          states[btn.id] = val ? 'on' : 'off'
        }
      }
    )
    return states
  })

  const headerToggleStates = computed(() => {
    const states: Record<string, string> = {}
    const mqttBooleans = (state.value.booleans || {}) as Record<string, unknown>
    headerToggles.value.forEach((toggle: { id: string; label: string; entity: string }) => {
      // The 7 inverter-control flags always read Cerbo MQTT booleans, even
      // when ha_use_direct_api is on. Other header toggles may still use HA.
      states[toggle.id] = resolveHeaderToggleState(
        toggle,
        haEntityStates.value,
        haEnabled.value,
        mqttBooleans
      )
    })
    return states
  })

  async function sendHaOrMqtt(action: string, payload: Record<string, unknown> = {}) {
    try {
      await invoke('perform_action', { action, payload })
    } catch (e) {
      logger.error('Action failed:', action, payload, e)
      // Re-throw so UI can show error
      throw e
    }
  }

  function cleanupHa() {
    if (unlistenHaUpdate) unlistenHaUpdate()
    if (unlistenHaConn) unlistenHaConn()
    if (unlistenHaFiltered) unlistenHaFiltered()
    if (haGraceTimer) {
      clearTimeout(haGraceTimer)
      haGraceTimer = null
    }
  }

  const haLoadsForConfig = computed(() => {
    const mqttLoads = state.value.loads
    if (!mqttLoads || Object.keys(mqttLoads).length === 0) {
      return []
    }
    const items: Array<{ key: string; name: string }> = []
    for (const [key, val] of Object.entries(mqttLoads)) {
      const v = typeof val === 'number' ? val : Number(val)
      if (!Number.isNaN(v) && Math.abs(v) > 2) {
        items.push({
          key,
          name: getFormattedLoadName(key),
        })
      }
    }
    // Sort by absolute value descending, then by name alphabetically (case-insensitive)
    items.sort((a, b) => {
      const valA =
        typeof mqttLoads[a.key] === 'number' ? mqttLoads[a.key] : Number(mqttLoads[a.key])
      const valB =
        typeof mqttLoads[b.key] === 'number' ? mqttLoads[b.key] : Number(mqttLoads[b.key])
      const absDiff = Math.abs(valB) - Math.abs(valA)
      if (absDiff !== 0) return absDiff
      // tie-break by name alphabetically (case-insensitive)
      return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
    })
    return items
  })

  return {
    haEnabled,
    haConnected,
    haWsConnected,
    haEntityStates,
    haEntityAttributes,
    homeButtons,
    headerToggles,
    buttonStates,
    headerToggleStates,
    waterValveState,
    pumpSwitchState,
    waterLevel,
    waterSectionVisible,
    waterPumpMode,
    waterValveMode,
    evSoc,
    evChargingKw,
    evClampWatts,
    evPower,
    evPowerWatts,
    evSectionVisible,
    haLoads,
    haSensors,
    haNumbers,
    haCovers,
    haMediaPlayers,
    haScenes,
    haWeather,
    haLoadsForConfig,
    dishwasherActive,
    dishwasherRemainingTime,
    washerActive,
    washerRemainingTime,
    dryerActive,
    dryerRemainingTime,
    washerStartEntity: haWasherStartEntity,
    washerPauseEntity: haWasherPauseEntity,
    dryerStartEntity: haDryerStartEntity,
    dryerPauseEntity: haDryerPauseEntity,
    coerceBool,
    initHa,
    sendHaOrMqtt,
    cleanupHa,
    setWindowHidden,
  }
}

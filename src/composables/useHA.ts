import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { computed, markRaw, type Ref, ref, watch } from 'vue'
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
import { isInverterControlFlag, resolveHeaderToggleState } from '../utils'
import { appConfig, applyInverterState, type InverterState, state } from './useInverterState'

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
        // Merge — never assign get_state raw (serde nulls would wipe tiles).
        const initial = await invoke<InverterState>('get_state')
        if (initial) {
          applyInverterState(initial)
        }
        // Sensors are not live-ticked (WebKit freeze); refresh snapshot on show.
        try {
          const filtered = await invoke<HaFilteredData>('get_ha_filtered_data')
          haSensors.value = markRaw(filtered.sensors)
          haNumbers.value = markRaw(filtered.numbers)
          haCovers.value = markRaw(filtered.covers)
          haMediaPlayers.value = markRaw(filtered.media_players)
          haScenes.value = markRaw(filtered.scenes)
          haWeather.value = filtered.weather ? markRaw(filtered.weather) : null
        } catch (e) {
          logger.warn('Failed to refresh HA filtered snapshot:', e)
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
      // markRaw: opaque display snapshots from Rust — avoid deep proxies.
      // Sensors only refresh on connect/force (refresh_sensors); live ticks
      // omit them so SidePanel does not re-render the whole house inventory.
      if (data.refresh_sensors) {
        haSensors.value = markRaw(data.sensors)
      }
      haNumbers.value = markRaw(data.numbers)
      haCovers.value = markRaw(data.covers)
      haMediaPlayers.value = markRaw(data.media_players)
      haScenes.value = markRaw(data.scenes)
      haWeather.value = data.weather ? markRaw(data.weather) : null
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

  // Water / EV / active loads live in useMQTTState (Cerbo MQTT only).
  // No HA fallback, no toggles for those sections.

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

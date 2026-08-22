import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { computed, type Ref, ref, watch } from 'vue'
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
import { formatPower } from '../utils'
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
    cfg?.ha_pump_switch_entity,
    cfg?.ha_valve_switch_entity,
    cfg?.ha_water_level_entity,
    cfg?.ha_ev_soc_entity,
    cfg?.ha_ev_charging_entity,
    cfg?.ha_ev_clamp_entity,
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

export function useHA() {
  const haEntityStates = ref<Record<string, string>>({})
  const haEntityAttributes = ref<Record<string, Record<string, unknown>>>({})
  const haWsConnected = ref(false)
  let unlistenHaUpdate: (() => void) | null = null
  let unlistenHaConn: (() => void) | null = null
  let unlistenHaFiltered: (() => void) | null = null

  /** Parse an HA state string into a number; excludes unavailable/unknown states */
  function parseNumberState(entity: string): number | null {
    const stateVal = haEntityStates.value[entity]
    if (!stateVal) return null
    const raw = String(stateVal).trim()
    const lower = raw.toLowerCase()
    if (!lower || lower === 'unavailable' || lower === 'unknown') return null
    // Replace comma with dot to handle European decimal format, assuming no thousand separator
    const normalized = raw.replace(/,/g, '.')
    // Extract the first number from the string (handles cases like "10.1 kW", "OFF", etc.)
    const match = /^[-+]?\d*\.?\d+/.exec(normalized)
    if (match) {
      const n = Number.parseFloat(match[0])
      return Number.isNaN(n) ? null : n
    }
    return null
  }

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
      haWsConnected.value = event.payload
      // On connect, fetch full state so buttons show correct state
      if (event.payload) {
        fetchHaStates()
      }
    })

    // Fetch initial state on mount
    await fetchHaStates()

    // Fetch button/switch states so UI shows correct on/off at startup
    const buttonEntityIds = [
      'input_boolean.only_charging',
      'input_boolean.no_feed',
      'input_boolean.house_support',
      'input_boolean.charge_battery',
      'input_boolean.do_not_supply_charger',
      'input_boolean.set_limit_to_ev_charger',
      'input_boolean.minimize_charging',
    ]
    await fetchHaEntityStates(buttonEntityIds)

    // Fetch dashboard section entities (washer/dryer/dishwasher/water/EV/clamps) immediately
    const applianceEntities = configuredSectionEntities(appConfig)
    if (applianceEntities.length > 0) {
      await fetchHaEntityStates(applianceEntities)
    }

    // Check HA connection status via HTTP
    await checkHaConnection()

    // Poll HA connection status periodically in case WS event is missed
    const connInterval = setInterval(() => {
      if (haEnabled.value && !haWsConnected.value) {
        checkHaConnection()
      }
    }, 10000)

    // Store interval for cleanup
    ;(globalThis as unknown as Record<string, unknown>).__haConnInterval = connInterval

    // Poll appliance entities so their state stays fresh even if WS events are missed
    const appliancePoll = setInterval(() => {
      const entities = configuredSectionEntities(appConfig)
      if (haEnabled.value && entities.length > 0) {
        fetchHaEntityStates(entities)
      }
    }, 30000)
    ;(globalThis as unknown as Record<string, unknown>).__haAppliancePoll = appliancePoll

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
          if (t.entity) ids.add(t.entity)
        }
        // Home buttons from config or ui_config
        const buttons =
          appConfig.value?.ha_entities?.filter((e) => e.enabled).map((e) => e.entity) ||
          state.value.ui_config?.home_buttons?.map((b) => b.entity) ||
          []
        for (const b of buttons) {
          if (b) ids.add(b)
        }
        if (ids.size > 0) {
          fetchHaEntityStates([...ids])
        }
      },
      { deep: true, immediate: false }
    )
  }

  const haValveSwitchEntity = computed(() => (appConfig.value?.ha_valve_switch_entity || '').trim())

  const haPumpSwitchEntity = computed(() => (appConfig.value?.ha_pump_switch_entity || '').trim())

  const haWaterLevelEntity = computed(() => (appConfig.value?.ha_water_level_entity || '').trim())

  const waterValveEntity = haValveSwitchEntity

  const pumpSwitchEntity = haPumpSwitchEntity

  function entityStateIsOn(entity: string): boolean | null {
    if (!entity) return null
    const stateVal = haEntityStates.value[entity]
    if (stateVal === undefined || stateVal === null) return null
    const lower = String(stateVal).trim().toLowerCase()
    if (!lower || lower === 'unavailable' || lower === 'unknown') return null
    return lower === 'on' || lower === 'running'
  }

  const waterValveState = computed(() => {
    if (!haEnabled.value) return null
    return entityStateIsOn(haValveSwitchEntity.value)
  })

  const pumpSwitchState = computed(() => {
    if (!haEnabled.value) return null
    return entityStateIsOn(haPumpSwitchEntity.value)
  })

  const waterLevel = computed(() => {
    if (!haEnabled.value) return null
    const entity = haWaterLevelEntity.value
    if (!entity) return null
    const n = parseNumberState(entity)
    return n
  })

  const waterSectionVisible = computed(() => {
    if (!haEnabled.value) return false
    return !!(haPumpSwitchEntity.value || haValveSwitchEntity.value || haWaterLevelEntity.value)
  })

  const haEvSocEntity = computed(() => (appConfig.value?.ha_ev_soc_entity || '').trim())

  const haEvChargingEntity = computed(() => (appConfig.value?.ha_ev_charging_entity || '').trim())

  const haEvClampEntity = computed(() => (appConfig.value?.ha_ev_clamp_entity || '').trim())

  const evSoc = computed(() => {
    if (!haEnabled.value) return null
    const entity = haEvSocEntity.value
    if (!entity) return null
    const n = parseNumberState(entity)
    if (n === null) return null
    return Math.max(0, Math.min(100, n))
  })

  /** EV car charging power in watts (converts from kW if needed) */
  const evChargingWatts = computed(() => {
    if (!haEnabled.value) return null
    const entity = haEvChargingEntity.value
    if (!entity) return null
    const stateNum = parseNumberState(entity)
    if (stateNum === null) return null

    // Check if we have attributes with unit information
    const attrs = haEntityAttributes.value[entity]
    const unit = attrs?.unit_of_measurement

    // Convert to watts: if unit is kW, multiply by 1000; if unit is W or unset, use as-is (assume watts)
    if (unit === 'kW') {
      return stateNum * 1000 // Convert kW to watts
    } else {
      // Assume watts (covers W, empty/null, or any other unit)
      return stateNum // Already in watts
    }
  })

  const evClampWatts = computed(() => {
    if (!haEnabled.value) return null
    const entity = haEvClampEntity.value
    if (!entity) return null
    const stateNum = parseNumberState(entity)
    if (stateNum === null) return null

    // Check if we have attributes with unit information
    const attrs = haEntityAttributes.value[entity]
    const unit = attrs?.unit_of_measurement

    // Convert to watts: if unit is kW, multiply by 1000; if unit is W or unset, use as-is (assume watts)
    if (unit === 'kW') {
      return stateNum * 1000 // Convert kW to watts
    } else {
      // Assume watts (covers W, empty/null, or any other unit)
      return stateNum // Already in watts
    }
  })

  const evChargingKw = computed(() => {
    const w = evChargingWatts.value
    return w === null ? null : w / 1000
  })

  const evPowerWatts = computed(() => {
    const w = evClampWatts.value
    return w === null ? null : Math.abs(w)
  })

  const evPower = computed(() => {
    const w = evClampWatts.value
    if (w === null) return ''
    return formatPower(Math.abs(w))
  })

  const evSectionVisible = computed(() => {
    if (!haEnabled.value) return false
    if (!(haEvSocEntity.value || haEvChargingEntity.value || haEvClampEntity.value)) return false
    return evSoc.value !== null || evChargingWatts.value !== null || evClampWatts.value !== null
  })

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
        const formattedName = key.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase())
        items.push({
          name: formattedName,
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
        if (haEnabled.value && haEntityStates.value[btn.entity] !== undefined) {
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
    headerToggles.value.forEach((toggle: { id: string; label: string; entity: string }) => {
      if (haEnabled.value && haEntityStates.value[toggle.entity] !== undefined) {
        states[toggle.id] = haEntityStates.value[toggle.entity] === 'on' ? 'on' : 'off'
      } else {
        // Fallback: check MQTT booleans (key = toggle.id or entity name without prefix)
        const entityKey = toggle.entity.split('.').pop() || toggle.id
        const rawVal =
          state.value.booleans?.[toggle.id] ??
          state.value.booleans?.[entityKey] ??
          state.value.booleans?.[toggle.entity]
        let val = rawVal
        if (typeof val === 'string') val = val === 'true' || val === '1'
        else if (typeof val === 'number') val = val !== 0
        states[toggle.id] = val ? 'on' : 'off'
      }
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
    const interval = (globalThis as unknown as Record<string, unknown>).__haConnInterval
    if (typeof interval === 'number') {
      clearInterval(interval)
    }
    const appliancePoll = (globalThis as unknown as Record<string, unknown>).__haAppliancePoll
    if (typeof appliancePoll === 'number') {
      clearInterval(appliancePoll)
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
        const formattedName = key.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase())
        items.push({
          key,
          name: formattedName,
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
    waterValveEntity,
    pumpSwitchEntity,
    waterValveState,
    pumpSwitchState,
    waterLevel,
    waterSectionVisible,
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

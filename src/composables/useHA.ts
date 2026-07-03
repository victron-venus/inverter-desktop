import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { computed, ref, watch } from 'vue'
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
import { appConfig, state } from './useInverterState'

export function useHA() {
  const haEntityStates = ref<Record<string, string>>({})
  const haEntityAttributes = ref<Record<string, Record<string, unknown>>>({})
  const haWsConnected = ref(false)
  let unlistenHaUpdate: (() => void) | null = null
  let unlistenHaConn: (() => void) | null = null
  let unlistenHaFiltered: (() => void) | null = null

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
    // If HA is configured, check if we have any data or WS connection
    const cfg = appConfig.value
    if (cfg?.ha_use_direct_api && cfg.ha_url && cfg.ha_longlived_token) {
      // Green if WS connected OR if we have entity states (data is flowing)
      if (haWsConnected.value) return true
      if (Object.keys(haEntityStates.value).length > 0) return true
      return false
    }
    // Fallback to MQTT ha_connected flag
    return !!state.value.ha_connected
  })

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
      for (const s of states) {
        haEntityStates.value = { ...haEntityStates.value, [s.entity_id]: s.state }
        if (s.attributes) {
          haEntityAttributes.value = { ...haEntityAttributes.value, [s.entity_id]: s.attributes }
        }
      }
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
      for (const s of states) {
        haEntityStates.value = { ...haEntityStates.value, [s.entity_id]: s.state }
        if (s.attributes) {
          haEntityAttributes.value = { ...haEntityAttributes.value, [s.entity_id]: s.attributes }
        }
      }
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
        const initial = await invoke<any>('get_state')
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
    recomputeFilteredFromEntityMaps()

    // Fetch button/switch states so UI shows correct on/off at startup
    const buttonEntityIds = [
      'switch.shutoff_valve',
      'switch.pump_switch',
      'input_boolean.only_charging',
      'input_boolean.no_feed',
      'input_boolean.house_support',
      'input_boolean.charge_battery',
      'input_boolean.do_not_supply_charger',
      'input_boolean.set_limit_to_ev_charger',
      'input_boolean.minimize_charging',
    ]
    await fetchHaEntityStates(buttonEntityIds)
    recomputeFilteredFromEntityMaps()

    // Check HA connection status via HTTP
    await checkHaConnection()

    // Poll HA connection status periodically in case WS event is missed
    const connInterval = setInterval(() => {
      if (haEnabled.value && !haWsConnected.value) {
        checkHaConnection()
      }
    }, 10000)

    // Store interval for cleanup
    ;(window as unknown as Record<string, unknown>).__haConnInterval = connInterval

    // Watch for config/state changes to fetch dynamic entity IDs (home buttons, header toggles)
    watch(
      [appConfig, () => state.value.ui_config],
      () => {
        if (!haEnabled.value) return
        const ids = new Set<string>()
        // Water/pump
        ids.add('switch.shutoff_valve')
        ids.add('switch.pump_switch')
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

  function coerceBool(v: unknown): boolean {
    if (v === true || v === 1 || v === 'true' || v === '1' || v === 'on' || v === 'online')
      return true
    return false
  }

  const waterValveEntity = computed(() => 'switch.shutoff_valve')

  const pumpSwitchEntity = computed(() => 'switch.pump_switch')

  const waterValveState = computed(() => {
    if (haEnabled.value) {
      const haVal = haEntityStates.value[waterValveEntity.value]
      if (haVal !== undefined) return haVal === 'on'
    }
    return coerceBool(state.value.water_valve)
  })

  const pumpSwitchState = computed(() => {
    if (haEnabled.value) {
      const haVal = haEntityStates.value[pumpSwitchEntity.value]
      if (haVal !== undefined) return haVal === 'on'
    }
    return coerceBool(state.value.pump_switch)
  })

  const dishwasherRunning = computed(() => {
    if (haEnabled.value) {
      const haVal =
        haEntityStates.value['binary_sensor.dishwasher_running'] ??
        haEntityStates.value['sensor.dishwasher_status'] ??
        haEntityStates.value['switch.dishwasher']
      if (haVal !== undefined) return haVal === 'on' || haVal === 'running'
    }
    const power = state.value.loads?.dishwasher
    return power !== undefined && (power as number) > 10
  })

  const washerRunning = computed(() => {
    if (haEnabled.value) {
      // Check binary_sensor or switch for running state
      const runVal =
        haEntityStates.value['binary_sensor.washer_running'] ??
        haEntityStates.value['switch.washer']
      if (runVal !== undefined) return runVal === 'on'
      // Fallback: check remaining time sensor (time > 0 means running)
      const timeVal = haEntityStates.value['sensor.washer_remaining_time']
      if (timeVal !== undefined) {
        const time = parseFloat(timeVal)
        return !Number.isNaN(time) && time > 0
      }
    }
    const power = state.value.loads?.washer
    return power !== undefined && (power as number) > 10
  })

  const dryerRunning = computed(() => {
    if (haEnabled.value) {
      const runVal =
        haEntityStates.value['binary_sensor.dryer_running'] ?? haEntityStates.value['switch.dryer']
      if (runVal !== undefined) return runVal === 'on'
      const timeVal = haEntityStates.value['sensor.dryer_remaining_time']
      if (timeVal !== undefined) {
        const time = parseFloat(timeVal)
        return !Number.isNaN(time) && time > 0
      }
    }
    const power = state.value.loads?.dryer
    return power !== undefined && (power as number) > 10
  })

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

  async function sendHaOrMqtt(
    action: string,
    payload: Record<string, unknown> = {} as Record<string, unknown>
  ) {
    try {
      await invoke('perform_action', { action, payload })
    } catch (e) {
      logger.error('Action failed:', action, payload, e)
      // Re-throw so UI can show error
      throw e
    }
  }

  /**
   * Fallback: compute filtered display data on frontend from entity state maps.
   * Called once after initial REST fetch (before WS is fully streaming).
   */
  function recomputeFilteredFromEntityMaps() {
    const states = haEntityStates.value
    const attrs = haEntityAttributes.value

    const sensors: HaSensorDisplay[] = []
    const numbers: HaNumberDisplay[] = []
    const covers: HaCoverDisplay[] = []
    const mediaPlayers: HaMediaPlayerDisplay[] = []
    const scenes: HaSceneDisplay[] = []
    let weather: HaWeatherDisplay | null = null

    for (const [entityId, state] of Object.entries(states)) {
      if (state === 'unavailable' || state === 'unknown') continue
      const domain = entityId.split('.')[0]
      const entityAttrs = attrs[entityId] || {}

      if (domain === 'sensor' || domain === 'binary_sensor') {
        const name = (entityAttrs.friendly_name as string) || entityId
        const unit = (entityAttrs.unit_of_measurement as string) || ''
        sensors.push({ entity_id: entityId, name, state, unit })
      } else if (domain === 'number') {
        const name = (entityAttrs.friendly_name as string) || entityId
        const value = parseFloat(state) || 0
        const min = (entityAttrs.min as number) ?? 0
        const max = (entityAttrs.max as number) ?? 100
        const step = (entityAttrs.step as number) ?? 1
        const unit = (entityAttrs.unit_of_measurement as string) || ''
        numbers.push({ entity_id: entityId, name, value, min, max, step, unit })
      } else if (domain === 'cover') {
        const name = (entityAttrs.friendly_name as string) || entityId
        const position = (entityAttrs.current_position as number) ?? 0
        covers.push({ entity_id: entityId, name, position })
      } else if (domain === 'media_player') {
        const name = (entityAttrs.friendly_name as string) || entityId
        mediaPlayers.push({ entity_id: entityId, name, state })
      } else if (domain === 'scene') {
        const name =
          (entityAttrs.friendly_name as string) || entityId.replace('scene.', '').replace(/_/g, ' ')
        scenes.push({ entity_id: entityId, name })
      } else if (domain === 'weather' && !weather) {
        const name = (entityAttrs.friendly_name as string) || 'Weather'
        const temperature = (entityAttrs.temperature as number) ?? null
        const unit = (entityAttrs.temperature_unit as string) || '°C'
        const forecast = (entityAttrs.forecast as Array<Record<string, unknown>>) ?? []
        weather = { entity_id: entityId, name, state, temperature, unit, forecast }
      }
    }

    haSensors.value = sensors
    haNumbers.value = numbers
    haCovers.value = covers
    haMediaPlayers.value = mediaPlayers
    haScenes.value = scenes
    haWeather.value = weather
  }

  function cleanupHa() {
    if (unlistenHaUpdate) unlistenHaUpdate()
    if (unlistenHaConn) unlistenHaConn()
    if (unlistenHaFiltered) unlistenHaFiltered()
    const interval = (window as unknown as Record<string, unknown>).__haConnInterval
    if (typeof interval === 'number') {
      clearInterval(interval)
    }
  }

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
  }
}

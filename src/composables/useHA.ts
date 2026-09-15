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
import { type ControlState, type DashboardControl, isInverterControlFlag } from '../inverterControl'
import { normalizeHaToggleState } from '../utils'
import { appConfig, state } from './useInverterState'

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

export function useHA() {
  const haEntityStates = ref<Record<string, string>>({})
  const haEntityAttributes = ref<Record<string, Record<string, unknown>>>({})
  const haWsConnected = ref(false)
  let session = 0
  let requestEpoch = 0
  let connectionRevision = 0
  let filteredRevision = 0
  let entityRevision = 0
  const entityRevisions = new Map<string, number>()
  let listeners: Array<() => void> = []
  let stopConfigWatch: (() => void) | null = null
  let windowHidden = false
  const HA_GRACE_PERIOD_MS = 15_000
  let haGraceTimer: ReturnType<typeof setTimeout> | null = null

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
  const haConnected = computed(() =>
    haEnabled.value ? haWsConnected.value : !!state.value.ha_connected
  )

  function clearHaState() {
    haEntityStates.value = {}
    haEntityAttributes.value = {}
    entityRevisions.clear()
    haSensors.value = []
    haNumbers.value = []
    haCovers.value = []
    haMediaPlayers.value = []
    haScenes.value = []
    haWeather.value = null
  }

  function cancelGracePeriod() {
    if (haGraceTimer) clearTimeout(haGraceTimer)
    haGraceTimer = null
  }

  function applyConnectionStatus(connected: boolean) {
    haWsConnected.value = connected
    if (connected) {
      cancelGracePeriod()
    } else if (!haGraceTimer) {
      // In-flight REST results from the old connection must not revive stale controls.
      requestEpoch += 1
      haGraceTimer = setTimeout(() => {
        haGraceTimer = null
        clearHaState()
      }, HA_GRACE_PERIOD_MS)
    }
  }

  function applyFilteredData(data: HaFilteredData, snapshot = false) {
    // Rust batches live sensor changes; markRaw avoids proxying the house inventory.
    if (snapshot || data.refresh_sensors) haSensors.value = markRaw(data.sensors)
    haNumbers.value = markRaw(data.numbers)
    haCovers.value = markRaw(data.covers)
    haMediaPlayers.value = markRaw(data.media_players)
    haScenes.value = markRaw(data.scenes)
    haWeather.value = data.weather ? markRaw(data.weather) : null
  }

  function trackedEntityIds() {
    const ids = new Set(configuredSectionEntities(appConfig))
    const configuredToggles = appConfig.value?.header_toggles_config
    const toggles = configuredToggles?.length
      ? configuredToggles
      : (state.value.ui_config?.header_toggles ?? [])
    for (const toggle of toggles) {
      if (toggle.entity && !isInverterControlFlag(toggle.entity)) {
        ids.add(toggle.entity)
      }
    }
    const configuredButtons = appConfig.value?.ha_entities
    const buttons = configuredButtons?.length
      ? configuredButtons.filter((entity) => entity.enabled).map((entity) => entity.entity)
      : (state.value.ui_config?.home_buttons?.map((button) => button.entity) ?? [])
    for (const entity of buttons) {
      if (entity && !isInverterControlFlag(entity)) ids.add(entity)
    }
    return [...ids]
  }

  function storeEntityStates(
    states: Array<{ entity_id: string; state: string; attributes?: Record<string, unknown> }>,
    startedAtRevision: number
  ) {
    const nextStates = { ...haEntityStates.value }
    const nextAttributes = { ...haEntityAttributes.value }
    for (const entry of states) {
      // A late HTTP snapshot must not overwrite a newer WebSocket value.
      if ((entityRevisions.get(entry.entity_id) ?? 0) > startedAtRevision) continue
      nextStates[entry.entity_id] = entry.state
      if (entry.attributes) nextAttributes[entry.entity_id] = entry.attributes
    }
    haEntityStates.value = nextStates
    haEntityAttributes.value = nextAttributes
  }

  async function fetchHaStates(entityIds?: string[]) {
    const cfg = appConfig.value
    if (!haEnabled.value || !cfg || (entityIds && entityIds.length === 0)) return
    const current = session
    const epoch = requestEpoch
    const revision = entityRevision
    try {
      const states = await invoke<
        Array<{ entity_id: string; state: string; attributes?: Record<string, unknown> }>
      >(entityIds ? 'get_ha_entity_states' : 'get_ha_appliance_states', {
        url: cfg.ha_url,
        port: cfg.ha_port || 8123,
        token: cfg.ha_longlived_token,
        ...(entityIds ? { entityIds } : {}),
      })
      if (current === session && epoch === requestEpoch && haEnabled.value) {
        storeEntityStates(states, revision)
      }
    } catch (error) {
      logger.warn('Failed to fetch HA states:', error)
    }
  }

  async function fetchFilteredSnapshot() {
    if (!haEnabled.value || windowHidden) return
    const current = session
    const epoch = requestEpoch
    const revision = filteredRevision
    try {
      const filtered = await invoke<HaFilteredData>('get_ha_filtered_data')
      if (
        current === session &&
        epoch === requestEpoch &&
        revision === filteredRevision &&
        haEnabled.value
      ) {
        applyFilteredData(filtered, true)
      }
    } catch (error) {
      logger.warn('Failed to refresh HA filtered snapshot:', error)
    }
  }

  async function checkHaConnection() {
    if (!haEnabled.value) return
    const current = session
    const epoch = requestEpoch
    const revision = connectionRevision
    try {
      const connected = await invoke<boolean>('get_ha_connection_status')
      if (current === session && epoch === requestEpoch && revision === connectionRevision) {
        applyConnectionStatus(connected)
      }
    } catch (error) {
      if (current === session && epoch === requestEpoch && revision === connectionRevision) {
        applyConnectionStatus(false)
      }
      logger.warn('Failed to get HA WebSocket status:', error)
    }
  }

  async function refreshHa() {
    await Promise.all([fetchHaStates(), fetchHaStates(trackedEntityIds()), fetchFilteredSnapshot()])
  }

  async function setHaWindowHidden(hidden: boolean) {
    windowHidden = hidden
    if (!hidden) await Promise.all([refreshHa(), checkHaConnection()])
  }

  async function initHa() {
    cleanupHa()
    const current = session
    async function subscribe<T>(name: string, handler: (payload: T) => void) {
      if (current !== session) return
      const unlisten = await listen<T>(name, (event) => {
        if (current === session && haEnabled.value) handler(event.payload)
      })
      if (current === session) listeners.push(unlisten)
      else unlisten()
    }
    await subscribe<{ entity_id: string; state: string; attributes?: Record<string, unknown> }>(
      'ha-state-update',
      (entry) => {
        if (windowHidden || !entry.entity_id || typeof entry.state !== 'string') return
        entityRevisions.set(entry.entity_id, ++entityRevision)
        haEntityStates.value = { ...haEntityStates.value, [entry.entity_id]: entry.state }
        if (entry.attributes) {
          haEntityAttributes.value = {
            ...haEntityAttributes.value,
            [entry.entity_id]: entry.attributes,
          }
        }
      }
    )
    await subscribe<HaFilteredData>('ha-filtered-update', (data) => {
      if (windowHidden) return
      filteredRevision += 1
      applyFilteredData(data)
    })
    await subscribe<boolean>('ha-connection-status', (connected) => {
      connectionRevision += 1
      applyConnectionStatus(connected)
      if (connected) void refreshHa()
    })
    if (current !== session) return
    stopConfigWatch = watch(
      () =>
        JSON.stringify([
          appConfig.value?.ha_use_direct_api,
          appConfig.value?.ha_url,
          appConfig.value?.ha_port,
          appConfig.value?.ha_longlived_token,
          trackedEntityIds(),
        ]),
      () => {
        requestEpoch += 1
        cancelGracePeriod()
        clearHaState()
        haWsConnected.value = false
        if (haEnabled.value) {
          const epoch = requestEpoch
          void refreshHa().then(() => {
            if (current === session && epoch === requestEpoch) return checkHaConnection()
          })
        }
      },
      { flush: 'sync' }
    )
    // Subscribe first, then pull the cached snapshot: startup events may precede Vue mounting.
    await refreshHa()
    if (current === session) await checkHaConnection()
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

  const dishwasherRuntime = computed(() => {
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

  /** Adapter for actual HA entities; inverter-control flags stay in the core. */
  function getHaControlState(control: DashboardControl): ControlState | undefined {
    if (!haEnabled.value || isInverterControlFlag(control.entity)) return undefined
    const value = haEntityStates.value[control.entity]
    return value === undefined ? undefined : normalizeHaToggleState(value)
  }

  function cleanupHa() {
    session += 1
    requestEpoch += 1
    for (const unlisten of listeners) unlisten()
    listeners = []
    stopConfigWatch?.()
    stopConfigWatch = null
    cancelGracePeriod()
  }

  return {
    haEnabled,
    haConnected,
    haWsConnected,
    haEntityStates,
    haEntityAttributes,
    haSensors,
    haNumbers,
    haCovers,
    haMediaPlayers,
    haScenes,
    haWeather,
    dishwasherActive,
    dishwasherRuntime,
    washerActive,
    washerRemainingTime,
    dryerActive,
    dryerRemainingTime,
    washerStartEntity: haWasherStartEntity,
    washerPauseEntity: haWasherPauseEntity,
    dryerStartEntity: haDryerStartEntity,
    dryerPauseEntity: haDryerPauseEntity,
    initHa,
    cleanupHa,
    setHaWindowHidden,
    getHaControlState,
  }
}

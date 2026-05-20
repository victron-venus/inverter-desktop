import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { state, appConfig } from './useInverterState'
import { logger } from '../logger'

const HA_DOMAINS = ['switch', 'light', 'input_boolean', 'fan', 'cover', 'lock', 'media_player', 'scene', 'script', 'number', 'sensor', 'binary_sensor']

function isHaEntity(entityId: string): boolean {
  if (!entityId || typeof entityId !== 'string') return false
  const parts = entityId.split('.')
  return parts.length === 2 && HA_DOMAINS.includes(parts[0])
}

export function useHA() {
  const haEntityStates = ref<Record<string, string>>({})
  const haLastPollSuccess = ref(false)
  let haPollInterval: number | null = null

  const haEnabled = computed(() => {
    const cfg = appConfig.value
    return !!(cfg && cfg.ha_use_direct_api && cfg.ha_url && cfg.ha_longlived_token)
  })

  const haConnected = computed(() => {
    if (haEnabled.value) {
      return haLastPollSuccess.value
    }
    return state.value.ha_connected || false
  })

  async function pollHaStates() {
    if (!haEnabled.value) return
    const cfg = appConfig.value
    if (!cfg?.ha_url || !cfg.ha_longlived_token) return
    try {
      const states = await invoke<Array<{ entity_id: string; state: string }>>('get_ha_states', {
        url: cfg.ha_url,
        port: cfg.ha_port || 8123,
        token: cfg.ha_longlived_token
      })
      const map: Record<string, string> = {}
      for (const s of states) {
        if (s.state === 'on' || s.state === 'off') {
          map[s.entity_id] = s.state
        }
      }
      haEntityStates.value = map
      haLastPollSuccess.value = true
    } catch (e) {
      logger.error('HA poll failed:', e)
      haLastPollSuccess.value = false
    }
  }

  function startHaPolling() {
    if (haPollInterval) clearInterval(haPollInterval)
    haPollInterval = window.setInterval(pollHaStates, 3000)
    pollHaStates()
  }

  function stopHaPolling() {
    if (haPollInterval) {
      clearInterval(haPollInterval)
      haPollInterval = null
    }
    haEntityStates.value = {}
    haLastPollSuccess.value = false
  }

  const waterValveEntity = computed(() => {
    const config = appConfig.value
    return config?.ha_water_valve_entity || 'switch.shutoff_valve'
  })

  const pumpSwitchEntity = computed(() => {
    const config = appConfig.value
    return config?.ha_pump_switch_entity || 'switch.pump_switch'
  })

  const homeButtons = computed(() => {
    const uiConfig = state.value.ui_config || {}
    if (uiConfig.home_buttons) return uiConfig.home_buttons
    const cfg = appConfig.value
    if (cfg?.ha_entities && cfg.ha_entities.length > 0) {
      return cfg.ha_entities.filter((e: any) => e.enabled).map((e: any) => ({
        id: e.id,
        label: e.label,
        entity: e.entity,
        state_key: e.state_key
      }))
    }
    if (cfg?.ha_switch_entities) {
      return Object.entries(cfg.ha_switch_entities).map(([id, data]: [string, any]) => ({
        id,
        label: data.label || id,
        entity: data.entity
      }))
    }
    return []
  })

  const headerToggles = computed(() => {
    const uiConfig = state.value.ui_config || {}
    if (uiConfig.header_toggles) return uiConfig.header_toggles
    const cfg = appConfig.value
    if (cfg?.header_toggles_config && cfg.header_toggles_config.length > 0) {
      return cfg.header_toggles_config
    }
    if (cfg?.ha_boolean_entities) {
      return Object.entries(cfg.ha_boolean_entities).map(([id, entity]) => ({
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

  const buttonStates = computed(() => {
    const states: Record<string, string> = {}
    homeButtons.value.forEach((btn: any) => {
      if (haEnabled.value && haEntityStates.value[btn.entity] !== undefined) {
        states[btn.id] = haEntityStates.value[btn.entity] === 'on' ? 'on' : 'off'
      } else {
        const stateKey = btn.state_key || 'home_' + btn.id
        let val = state.value.booleans?.[stateKey]
        if (typeof val === 'string') val = val === 'true' || val === '1'
        else if (typeof val === 'number') val = val !== 0
        states[btn.id] = val ? 'on' : 'off'
      }
    })
    return states
  })

  const headerToggleStates = computed(() => {
    const states: Record<string, string> = {}
    headerToggles.value.forEach((toggle: any) => {
      if (haEnabled.value && haEntityStates.value[toggle.entity] !== undefined) {
        states[toggle.id] = haEntityStates.value[toggle.entity] === 'on' ? 'on' : 'off'
      } else {
        let val = state.value.booleans?.[toggle.id]
        if (typeof val === 'string') val = val === 'true' || val === '1'
        else if (typeof val === 'number') val = val !== 0
        states[toggle.id] = val ? 'on' : 'off'
      }
    })
    return states
  })

  async function sendHaOrMqtt(action: string, payload: any = {}) {
    if (haEnabled.value && payload.entity && isHaEntity(payload.entity)) {
      const cfg = appConfig.value!
      try {
        await invoke('toggle_ha_entity', {
          url: cfg.ha_url || '',
          port: cfg.ha_port || 8123,
          token: cfg.ha_longlived_token || '',
          entity_id: payload.entity
        })
        return
      } catch (e) {
        logger.error('HA command failed:', e)
      }
    }
    try {
      await invoke('send_command', { action, payload })
    } catch (e) {
      logger.error('Failed to send command:', e)
    }
  }

  function cleanupHa() {
    stopHaPolling()
  }

  return {
    haEnabled,
    haConnected,
    haEntityStates,
    haLastPollSuccess,
    homeButtons,
    headerToggles,
    buttonStates,
    headerToggleStates,
    waterValveEntity,
    pumpSwitchEntity,
    pollHaStates,
    startHaPolling,
    stopHaPolling,
    sendHaOrMqtt,
    cleanupHa,
  }
}

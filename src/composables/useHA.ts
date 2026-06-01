import { ref, computed } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { state, appConfig } from './useInverterState'
import { logger } from '../logger'

export function useHA() {
  const haEntityStates = ref<Record<string, string>>({})
  const haLastPollSuccess = ref(false)
  let unlistenHaUpdate: (() => void) | null = null

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

  async function initHa() {
    unlistenHaUpdate = await listen<Record<string, string>>('ha-state-update', (event) => {
      haEntityStates.value = event.payload
      haLastPollSuccess.value = true
    })
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
    if (cfg?.ha_switch_entities) {
      return Object.entries(cfg.ha_switch_entities).map(([id, data]) => ({
        id,
        label: (data.label as string | undefined) || id,
        entity: (data as { entity: string }).entity,
      }))
    }
    return []
  })

  const headerToggles = computed(() => {
    const cfg = appConfig.value
    if (cfg?.header_toggles_config && cfg.header_toggles_config.length > 0) {
      return cfg.header_toggles_config
    }
    const uiConfig = state.value.ui_config || {}
    if (uiConfig.header_toggles) return uiConfig.header_toggles
    if (cfg?.ha_boolean_entities) {
      return Object.entries(cfg.ha_boolean_entities).map(([id, entity]) => ({
        id,
        label: id.replace(/_/g, ' ').toUpperCase(),
        entity,
      }))
    }
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
          const stateKey = btn.state_key || 'home_' + btn.id
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
        let val = state.value.booleans?.[toggle.id]
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
      logger.error('Action failed:', e)
    }
  }

  function cleanupHa() {
    if (unlistenHaUpdate) unlistenHaUpdate()
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
    initHa,
    sendHaOrMqtt,
    cleanupHa,
  }
}

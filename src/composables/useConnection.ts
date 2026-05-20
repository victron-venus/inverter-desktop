import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { sendNotification, requestPermission, isPermissionGranted } from '@tauri-apps/plugin-notification'
import { state, mqttConnected, appConfig, type InverterState } from './useInverterState'
import { getAppConfig } from '../config'
import { logger } from '../logger'

/** Notification thresholds */
const THRESHOLDS = {
  LOAD_W: 300,        // Notify if any individual load exceeds (washing machine, dryer)
  CONSUMPTION_W: 300, // Notify if total house consumption from grid exceeds
  WATER_CM: 23,       // Notify if cistern water level drops below (near empty)
  SOLAR_W: 3000,      // Notify if solar production exceeds (peak alert)
} as const

type BoolField = keyof Pick<InverterState, 'pump_switch' | 'water_valve' | 'washer_power' | 'dryer_power' | 'dry_run'>

/** Map BoolField keys to their string->boolean conversion source */
const BOOL_FIELDS: BoolField[] = ['pump_switch', 'water_valve', 'washer_power', 'dryer_power', 'dry_run']

function coerceBooleans(newState: InverterState) {
  if (newState.booleans) {
    for (const key of Object.keys(newState.booleans)) {
      const val = newState.booleans[key]
      if (typeof val === 'string') {
        newState.booleans[key] = val === 'true' || val === '1'
      }
    }
  }
  for (const field of BOOL_FIELDS) {
    const val = newState[field]
    if (typeof val === 'string') {
      newState[field] = val === 'true' || val === '1'
    }
  }
}

export function useConnection() {
  let notificationPermission = false
  let unlistenStateUpdate: (() => void) | null = null

  async function ensureNotificationPermission() {
    try {
      let granted = await isPermissionGranted()
      if (!granted) {
        const permission = await requestPermission()
        granted = permission === 'granted'
      }
      notificationPermission = granted
    } catch (e) {
      logger.error('Notification permission error:', e)
    }
  }

  function checkThresholds(newState: InverterState) {
    if (!notificationPermission) return
    if (newState.loads) {
      for (const [name, power] of Object.entries(newState.loads)) {
        if (power > THRESHOLDS.LOAD_W) {
          sendNotification({ title: 'High Load', body: `${name}: ${power}W` })
        }
      }
    }
    if (newState.tt && newState.tt > THRESHOLDS.CONSUMPTION_W) {
      sendNotification({ title: 'High Consumption', body: `Consumption: ${newState.tt}W` })
    }
    if (newState.water_level !== undefined && newState.water_level < THRESHOLDS.WATER_CM) {
      sendNotification({ title: 'Low Water', body: `Water level: ${newState.water_level} cm` })
    }
    if (newState.solar_total && newState.solar_total > THRESHOLDS.SOLAR_W) {
      sendNotification({ title: 'High Solar', body: `Solar: ${newState.solar_total}W` })
    }
  }

  function processState(newState: InverterState) {
    coerceBooleans(newState)
    state.value = newState
    checkThresholds(newState)
  }

  async function connectMqtt() {
    try {
      const config = await getAppConfig()
      appConfig.value = config
      if (config.color_scheme) {
        const isDark = config.color_scheme !== 'light'
        document.body.classList.toggle('light', !isDark)
        localStorage.setItem('theme', config.color_scheme)
      }

      // Subscribe to MQTT state updates from Rust (event-driven, no polling)
      unlistenStateUpdate = await listen<InverterState>('mqtt-state-update', (event) => {
        processState(event.payload)
      })

      await invoke('connect_mqtt', { host: config.mqtt_host, port: config.mqtt_port, portalId: config.portal_id || null })
      mqttConnected.value = true

      // Fetch initial state
      try {
        const initial = await invoke<InverterState>('get_state')
        processState(initial)
      } catch (e) {
        logger.error('Failed to get initial state:', e)
      }
    } catch (e) {
      logger.error('Failed to connect to MQTT:', e)
      mqttConnected.value = false
    }
  }

  async function send(action: string, payload: any = {}) {
    try {
      await invoke('send_command', { action, payload })
    } catch (e) {
      logger.error('Failed to send command:', e)
    }
  }

  function cleanup() {
    if (unlistenStateUpdate) {
      unlistenStateUpdate()
      unlistenStateUpdate = null
    }
  }

  return {
    state,
    mqttConnected,
    appConfig,
    connectMqtt,
    send,
    ensureNotificationPermission,
    cleanup,
  }
}

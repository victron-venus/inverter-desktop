import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { sendNotification, requestPermission, isPermissionGranted } from '@tauri-apps/plugin-notification'
import { state, mqttConnected, appConfig, type InverterState } from './useInverterState'
import { getAppConfig } from '../config'
import { logger } from '../logger'

export function useConnection() {
  let notificationPermission = false
  let unlistenStateUpdate: (() => void) | null = null

  const NOTIFY_LOAD_W = 300
  const NOTIFY_CONSUMPTION_W = 300
  const NOTIFY_WATER_CM = 23
  const NOTIFY_SOLAR_W = 3000

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
        if (power > NOTIFY_LOAD_W) {
          sendNotification({ title: 'High Load', body: `${name}: ${power}W` })
        }
      }
    }
    if (newState.tt && newState.tt > NOTIFY_CONSUMPTION_W) {
      sendNotification({ title: 'High Consumption', body: `Consumption: ${newState.tt}W` })
    }
    if (newState.water_level !== undefined && newState.water_level < NOTIFY_WATER_CM) {
      sendNotification({ title: 'Low Water', body: `Water level: ${newState.water_level} cm` })
    }
    if (newState.solar_total && newState.solar_total > NOTIFY_SOLAR_W) {
      sendNotification({ title: 'High Solar', body: `Solar: ${newState.solar_total}W` })
    }
  }

  type BoolField = keyof Pick<InverterState, 'pump_switch' | 'water_valve' | 'washer_power' | 'dryer_power' | 'dry_run'>

  function processState(newState: InverterState) {
    if (newState.booleans) {
      Object.keys(newState.booleans).forEach(key => {
        const val = newState.booleans![key]
        if (typeof val === 'string') {
          newState.booleans![key] = val === 'true' || val === '1'
        }
      })
    }
    const boolFields: BoolField[] = ['pump_switch', 'water_valve', 'washer_power', 'dryer_power', 'dry_run']
    boolFields.forEach(field => {
      const val = newState[field]
      if (typeof val === 'string') {
        (newState as Record<string, unknown>)[field] = val === 'true' || val === '1'
      }
    })
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

      await invoke('connect_mqtt', { host: config.mqtt_host, port: config.mqtt_port })
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

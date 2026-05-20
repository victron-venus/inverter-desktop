import { invoke } from '@tauri-apps/api/core'
import { sendNotification, requestPermission, isPermissionGranted } from '@tauri-apps/plugin-notification'
import { state, mqttConnected, appConfig, type InverterState } from './useInverterState'
import { getAppConfig } from '../config'
import { logger } from '../logger'

export function useConnection() {
  let pollInterval: number | null = null
  let notificationPermission = false

  const NOTIFY_LOAD_W = 300        // Notify if any individual load > 300W (e.g. washing machine, dryer)
  const NOTIFY_CONSUMPTION_W = 300 // Notify if total house consumption > 300W from grid
  const NOTIFY_WATER_CM = 23       // Notify if cistern water level drops below 23cm (near empty)
  const NOTIFY_SOLAR_W = 3000      // Notify if solar production exceeds 3kW (peak alert)

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

    async function connectMqtt() {
    try {
      const config = await getAppConfig()
      appConfig.value = config
      if (config.color_scheme) {
        const isDark = config.color_scheme !== 'light'
        document.body.classList.toggle('light', !isDark)
        localStorage.setItem('theme', config.color_scheme)
      }
      await invoke('connect_mqtt', { host: config.mqtt_host, port: config.mqtt_port })
      mqttConnected.value = true
      startPolling()
    } catch (e) {
      logger.error('Failed to connect to MQTT:', e)
      mqttConnected.value = false
    }
  }

  function startPolling() {
    if (pollInterval) clearInterval(pollInterval)
    pollInterval = window.setInterval(async () => {
      try {
        const newState = await invoke<InverterState>('get_state')
        if (newState.booleans) {
          Object.keys(newState.booleans).forEach(key => {
            const val = newState.booleans![key]
            if (typeof val === 'string') {
              newState.booleans![key] = val === 'true' || val === '1'
            }
          })
        }
        type BoolField = keyof Pick<InverterState, 'pump_switch' | 'water_valve' | 'washer_power' | 'dryer_power' | 'dry_run'>
        const boolFields: BoolField[] = ['pump_switch', 'water_valve', 'washer_power', 'dryer_power', 'dry_run']
        boolFields.forEach(field => {
          const val = newState[field]
          if (typeof val === 'string') {
            (newState as Record<string, unknown>)[field] = val === 'true' || val === '1'
          }
        })
        state.value = newState
        checkThresholds(newState)
      } catch (e) {
        logger.error('Failed to get state:', e)
        mqttConnected.value = false
      }
    }, 1000)
  }

  async function send(action: string, payload: any = {}) {
    try {
      await invoke('send_command', { action, payload })
    } catch (e) {
      logger.error('Failed to send command:', e)
    }
  }

  function cleanup() {
    if (pollInterval) clearInterval(pollInterval)
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

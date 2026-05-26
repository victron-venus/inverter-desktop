import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { requestPermission, isPermissionGranted } from '@tauri-apps/plugin-notification'
import { state, mqttConnected, appConfig, type InverterState } from './useInverterState'
import { getAppConfig } from '../config'
import { logger } from '../logger'

export function useConnection() {
  let unlistenStateUpdate: (() => void) | null = null
  let unlistenConnectionStatus: (() => void) | null = null
  let unlistenCamera: (() => void) | null = null

  async function ensureNotificationPermission() {
    try {
      let granted = await isPermissionGranted()
      if (!granted) {
        await requestPermission()
      }
    } catch (e) {
      logger.error('Notification permission error:', e)
    }
  }

  function processState(newState: InverterState) {
    state.value = newState
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

      // Cleanup existing listeners if any
      cleanup()

      // Subscribe to MQTT state updates from Rust (event-driven, no polling)
      unlistenStateUpdate = await listen<InverterState>('mqtt-state-update', (event) => {
        processState(event.payload)
      })

      // Subscribe to MQTT connection status updates
      unlistenConnectionStatus = await listen<boolean>('mqtt-connection-status', (event) => {
        mqttConnected.value = event.payload
      })
      
      // Subscribe to camera events
      unlistenCamera = await listen<any>('camera-event', (event) => {
        // Emit global event for App.vue to show popup
        globalThis.dispatchEvent(new CustomEvent('show-video-popup', { detail: event.payload }))
      })

      await invoke('connect_mqtt', { 
        host: config.mqtt_host, 
        port: config.mqtt_port, 
        portalId: config.portal_id || null,
        cameraTopic: null // Primary broker doesn't listen to cameras now
      })

      // Connect to HA MQTT broker if configured
      if (config.mqtt_ha_host && config.mqtt_ha_port) {
        try {
          await invoke('connect_ha_mqtt', {
            host: config.mqtt_ha_host,
            port: config.mqtt_ha_port,
            cameraTopic: config.camera_topic || null
          })
          logger.log('Connected to HA MQTT broker for cameras')
        } catch (e) {
          logger.error('Failed to connect to HA MQTT:', e)
        }
      }

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
      await invoke('perform_action', { action, payload })
    } catch (e) {
      logger.error('Failed to send command:', e)
    }
  }

  function cleanup() {
    if (unlistenStateUpdate) {
      unlistenStateUpdate()
      unlistenStateUpdate = null
    }
    if (unlistenConnectionStatus) {
      unlistenConnectionStatus()
      unlistenConnectionStatus = null
    }
    if (unlistenCamera) {
      unlistenCamera()
      unlistenCamera = null
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

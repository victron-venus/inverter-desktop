import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification'
import { markRaw } from 'vue'
import { getAppConfig } from '../config'
import { logger } from '../logger'
import {
  addNotification,
  appConfig,
  haMqttConnected,
  type InverterState,
  mqttConnected,
  state,
} from './useInverterState'
import { notify } from './useSystemNotifications'

export { notify }

async function ensureNotificationPermission() {
  try {
    const granted = await isPermissionGranted()
    if (!granted) {
      await requestPermission()
    }
  } catch (e) {
    logger.error('Notification permission error:', e)
  }
}

async function send(action: string, payload: Record<string, unknown> = {}) {
  try {
    await invoke('perform_action', { action, payload })
  } catch (e) {
    logger.error('Failed to send command:', e)
  }
}

export function useConnection() {
  let unlistenStateUpdate: (() => void) | null = null
  let unlistenConnectionStatus: (() => void) | null = null
  let unlistenCamera: (() => void) | null = null
  let unlistenNotification: (() => void) | null = null
  let unlistenHaMqttStatus: (() => void) | null = null
  let wakeUnlisten: (() => void) | null = null

  function processState(newState: InverterState) {
    state.value = markRaw(newState)
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

      cleanup()

      unlistenStateUpdate = await listen<InverterState>('mqtt-state-update', (event) => {
        processState(event.payload)
      })

      unlistenConnectionStatus = await listen<boolean>('mqtt-connection-status', (event) => {
        mqttConnected.value = event.payload
      })

      unlistenCamera = await listen<{ video_url: string; agent_name?: string }>(
        'camera-event',
        (event) => {
          globalThis.dispatchEvent(new CustomEvent('show-video-popup', { detail: event.payload }))
        }
      )

      unlistenNotification = await listen<{ title: string; body: string }>(
        'notification',
        (event) => {
          addNotification(event.payload.title, event.payload.body)
        }
      )

      await invoke('connect_mqtt', {
        host: config.mqtt_host,
        port: config.mqtt_port,
        username: config.mqtt_login || null,
        password: config.mqtt_password || null,
        portalId: config.portal_id || null,
        cameraTopic: null,
      })
      notify('MQTT', 'Connected to inverter')

      if (config.camera_enabled && config.mqtt_ha_host && config.mqtt_ha_port) {
        try {
          await invoke('connect_ha_mqtt', {
            host: config.mqtt_ha_host,
            port: config.mqtt_ha_port,
            username: config.mqtt_ha_login || null,
            password: config.mqtt_ha_password || null,
            cameraTopic: config.camera_topic || null,
          })
          haMqttConnected.value = true
          logger.log('Connected to HA MQTT broker for cameras')
          notify('Home Assistant', 'Connected to HA MQTT')
        } catch (e) {
          haMqttConnected.value = false
          logger.error('Failed to connect to HA MQTT:', e)
        }
      } else {
        haMqttConnected.value = null // Not configured — hide indicator
      }

      // Listen for HA MQTT connection status changes
      unlistenHaMqttStatus = await listen<boolean>('ha-mqtt-connection-status', (event) => {
        haMqttConnected.value = event.payload
        // Force reconnect on unexpected HA MQTT disconnect
        if (!event.payload && config.camera_enabled && config.mqtt_ha_host) {
          reconnectHaMqttAfterDelay()
        }
      })

      // Auto-reconnect MQTT on wake (network change, IP renewal after sleep)
      wakeUnlisten = await listen('window-focused', () => {
        if (mqttConnected.value) {
          // Already connected — all good
          return
        }
        logger.log('Wake detected, reconnecting MQTT...')
        reconnectAfterDelay()
      })

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

  let mqttReconnectTimer: ReturnType<typeof setTimeout> | null = null
  let haMqttReconnectTimer: ReturnType<typeof setTimeout> | null = null

  function reconnectAfterDelay(delay = 2000) {
    if (mqttReconnectTimer) clearTimeout(mqttReconnectTimer)
    mqttReconnectTimer = setTimeout(() => {
      mqttReconnectTimer = null
      connectMqtt()
    }, delay)
  }

  function reconnectHaMqttAfterDelay(delay = 2000) {
    if (haMqttReconnectTimer) clearTimeout(haMqttReconnectTimer)
    haMqttReconnectTimer = setTimeout(async () => {
      haMqttReconnectTimer = null
      try {
        const config = await getAppConfig()
        if (config.camera_enabled && config.mqtt_ha_host) {
          await invoke('connect_ha_mqtt', {
            host: config.mqtt_ha_host,
            port: config.mqtt_ha_port,
            username: config.mqtt_ha_login || null,
            password: config.mqtt_ha_password || null,
            cameraTopic: config.camera_topic || null,
          })
          haMqttConnected.value = true
          logger.log('HA MQTT reconnected')
        }
      } catch (e) {
        logger.error('HA MQTT reconnect failed:', e)
        haMqttConnected.value = false
      }
    }, delay)
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
    if (unlistenNotification) {
      unlistenNotification()
      unlistenNotification = null
    }
    if (unlistenHaMqttStatus) {
      unlistenHaMqttStatus()
      unlistenHaMqttStatus = null
    }
    if (wakeUnlisten) {
      wakeUnlisten()
      wakeUnlisten = null
    }
    if (mqttReconnectTimer) {
      clearTimeout(mqttReconnectTimer)
      mqttReconnectTimer = null
    }
    if (haMqttReconnectTimer) {
      clearTimeout(haMqttReconnectTimer)
      haMqttReconnectTimer = null
    }
  }

  return {
    state,
    mqttConnected,
    haMqttConnected,
    appConfig,
    connectMqtt,
    send,
    ensureNotificationPermission,
    cleanup,
  }
}

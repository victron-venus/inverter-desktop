import { invoke } from '@tauri-apps/api/core'
import { listen, type Event } from '@tauri-apps/api/event'
import { getAppConfig, type AppConfig } from '../../config'
import { appConfig, haMqttConnected } from '../../composables/useInverterState'
import { notify } from '../../composables/useSystemNotifications'
import { MQTT_OFFLINE_DELAY_MS } from '../../connectionPolicy'
import { logger } from '../../logger'

function cameraConnectArgs(config: AppConfig) {
  return {
    host: config.mqtt_ha_host,
    port: config.mqtt_ha_port,
    username: config.mqtt_ha_login || null,
    password: config.mqtt_ha_password || null,
    cameraTopic: config.camera_topic || null,
    frigateBaseUrl: config.frigate_base_url || null,
    ringSnapshotUrlTemplate: config.ring_snapshot_url_template || null,
  }
}

/** Separate broker lifecycle owned by the desktop camera feature. */
export function createCameraConnection() {
  let generation = 0
  let settingsRevision = 0
  let listeners: Array<() => void> = []
  let offlineTimer: ReturnType<typeof setTimeout> | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null
  let operations: Promise<unknown> = Promise.resolve()

  function transport(command: string, args?: Record<string, unknown>) {
    const current = generation
    const revision = settingsRevision
    const operation = operations
      .catch(() => {})
      .then(() => {
        if (current === generation && revision === settingsRevision) return invoke(command, args)
      })
    operations = operation
    return operation
  }

  function cleanup(disconnect = true) {
    generation += 1
    settingsRevision += 1
    for (const unlisten of listeners) unlisten()
    listeners = []
    if (offlineTimer) clearTimeout(offlineTimer)
    if (reconnectTimer) clearTimeout(reconnectTimer)
    offlineTimer = reconnectTimer = null
    haMqttConnected.value = null
    if (disconnect)
      void transport('disconnect_ha_mqtt').catch((error) =>
        logger.warn('Camera MQTT stop failed:', error)
      )
  }

  async function syncHaMqttFromConfig(config: AppConfig) {
    const current = generation
    const revision = ++settingsRevision
    if (config.camera_enabled && config.mqtt_ha_host?.trim() && config.mqtt_ha_port) {
      try {
        await transport('connect_ha_mqtt', cameraConnectArgs(config))
        if (current !== generation || revision !== settingsRevision) return
        haMqttConnected.value = true
        notify('Cameras', 'Connected to camera MQTT broker')
      } catch (error) {
        if (current === generation && revision === settingsRevision) haMqttConnected.value = false
        logger.error('Failed to connect camera MQTT:', error)
      }
    } else {
      await disconnectForSettings(current, revision)
    }
  }

  async function disconnectForSettings(current: number, revision: number) {
    if (offlineTimer) clearTimeout(offlineTimer)
    if (reconnectTimer) clearTimeout(reconnectTimer)
    offlineTimer = reconnectTimer = null
    try {
      await transport('disconnect_ha_mqtt')
    } catch (error) {
      logger.warn('Camera MQTT disconnect failed:', error)
    }
    if (current === generation && revision === settingsRevision) haMqttConnected.value = null
  }

  async function connect(config: AppConfig) {
    cleanup(false)
    const current = generation
    const desiredRevision = settingsRevision
    async function subscribe<T>(name: string, handler: (event: Event<T>) => void) {
      if (current !== generation) return
      const unlisten = await listen<T>(name, (event) => {
        if (current === generation) handler(event)
      })
      if (current === generation) listeners.push(unlisten)
      else unlisten()
    }
    await subscribe<{ video_url: string; agent_name?: string }>('camera-event', ({ payload }) => {
      if (!appConfig.value?.camera_enabled || !payload?.video_url) return
      void invoke('open_camera_video_window', {
        videoUrl: payload.video_url,
        agentName: payload.agent_name ?? null,
      }).catch((error) => logger.warn('Failed to open camera video window:', error))
    })
    await subscribe<boolean>('ha-mqtt-connection-status', ({ payload }) => {
      if (!appConfig.value?.camera_enabled) return
      if (payload) {
        if (offlineTimer) clearTimeout(offlineTimer)
        offlineTimer = null
        haMqttConnected.value = true
      } else if (!offlineTimer) {
        offlineTimer = setTimeout(() => {
          offlineTimer = null
          haMqttConnected.value = false
          if (!appConfig.value?.camera_enabled || !appConfig.value?.mqtt_ha_host?.trim()) return
          if (reconnectTimer) clearTimeout(reconnectTimer)
          reconnectTimer = setTimeout(async () => {
            reconnectTimer = null
            const revision = settingsRevision
            try {
              const latest = await getAppConfig()
              if (current === generation && revision === settingsRevision)
                await syncHaMqttFromConfig(latest)
            } catch (error) {
              if (current === generation && revision === settingsRevision)
                haMqttConnected.value = false
              logger.error('Camera MQTT reconnect failed:', error)
            }
          }, 2000)
        }, MQTT_OFFLINE_DELAY_MS)
      }
    })
    if (current === generation && desiredRevision === settingsRevision)
      await syncHaMqttFromConfig(config)
  }

  async function toggleCameraMotion() {
    // Invalidate reconnect reads before loading config; a read begun under the old
    // desired setting must not reconnect the broker after this user action.
    const current = generation
    const revision = ++settingsRevision
    if (reconnectTimer) clearTimeout(reconnectTimer)
    if (offlineTimer) clearTimeout(offlineTimer)
    reconnectTimer = offlineTimer = null
    const config = await getAppConfig()
    if (current !== generation || revision !== settingsRevision) return
    config.camera_enabled = !config.camera_enabled
    await invoke('save_config', { config })
    if (current !== generation || revision !== settingsRevision) return
    appConfig.value = { ...config }
    await syncHaMqttFromConfig(config)
    return config.camera_enabled
  }

  return { connect, cleanup, toggleCameraMotion, syncHaMqttFromConfig }
}

export const cameraConnection = createCameraConnection()

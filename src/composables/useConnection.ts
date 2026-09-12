import { invoke } from '@tauri-apps/api/core'
import { listen, type Event } from '@tauri-apps/api/event'
import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification'
import { type AppConfig, getAppConfig } from '../config'
import {
  chooseStartupSource,
  isIgwConfigured,
  isMqttConfigured,
  MQTT_CONNECT_WATCHDOG_MS,
  MQTT_OFFLINE_DELAY_MS,
  MQTT_RECOVERY_PROBE_MS,
  mqttReconnectDelayMs,
  shouldWatchdogFailoverToIgw,
} from '../connectionPolicy'
import { logger } from '../logger'
import {
  addNotification,
  appConfig,
  applyInverterState,
  type BannerNotification,
  clearBanner,
  dataSource,
  haMqttConnected,
  type InverterState,
  mqttConnected,
  state,
  upsertBanner,
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

/** Both MQTT + IGW configured → prefer MQTT; recover to MQTT while on IGW. */
let dualPathPreferMqtt = false
let mqttRecoveryTimer: ReturnType<typeof setInterval> | null = null
let mqttOnlyReconnectAttempt = 0
/** One-shot: dual-path MQTT started but never got status-true → IGW. */
let mqttConnectWatchdogTimer: ReturnType<typeof setTimeout> | null = null

function mqttConnectArgs(config: AppConfig) {
  return {
    host: config.mqtt_host,
    port: config.mqtt_port,
    username: config.mqtt_login || null,
    password: config.mqtt_password || null,
    portalId: config.portal_id || null,
    waterTankInstance: config.water_tank_instance ?? null,
    waterPumpInstance: config.water_pump_instance ?? null,
    waterValveInstance: config.water_valve_instance ?? null,
    evchargerInstance: config.evcharger_instance ?? 40,
    evInstance: config.ev_instance ?? 22,
    cameraTopic: null,
  }
}

function gatewayConnectArgs(config: AppConfig) {
  return {
    url: config.gateway_url,
    accessClientId: config.gateway_access_client_id,
    accessClientSecret: config.gateway_access_client_secret,
    apiToken: config.gateway_api_token || null,
  }
}

async function probeMqttReachable(config: AppConfig): Promise<boolean> {
  try {
    await invoke('test_mqtt_connection', {
      host: config.mqtt_host,
      port: config.mqtt_port,
      username: config.mqtt_login || null,
      password: config.mqtt_password || null,
    })
    return true
  } catch {
    return false
  }
}

export function useConnection() {
  let session = 0
  let inverterEnabled = false
  let listeners: Array<() => void> = []
  let transportOperations: Promise<unknown> = Promise.resolve()

  function invokeTransport(command: string, args?: Record<string, unknown>) {
    const current = session
    const operation = transportOperations
      .catch(() => {})
      .then(() => {
        // Preserve IPC completion order as well as backend lifecycle lock order.
        if (current === session) return invoke(command, args)
      })
    transportOperations = operation
    return operation
  }
  let mqttOfflineTimer: ReturnType<typeof setTimeout> | null = null
  let haMqttOfflineTimer: ReturnType<typeof setTimeout> | null = null

  let processStateCount = 0
  let processStateLastLogMs = 0

  function processState(newState: InverterState) {
    applyInverterState(newState)
    processStateCount += 1
    const now = Date.now()
    if (now - processStateLastLogMs >= 5000) {
      processStateLastLogMs = now
      logger.log(
        `mqtt processState x${processStateCount} gt=${state.value.gt} tt=${state.value.tt} soc=${state.value.battery_soc}`
      )
    }
  }

  function clearMqttConnectWatchdog() {
    if (mqttConnectWatchdogTimer) {
      clearTimeout(mqttConnectWatchdogTimer)
      mqttConnectWatchdogTimer = null
    }
  }

  function startMqttConnectWatchdog() {
    if (!inverterEnabled) return
    clearMqttConnectWatchdog()
    if (!dualPathPreferMqtt) return
    mqttConnectWatchdogTimer = setTimeout(() => {
      mqttConnectWatchdogTimer = null
      if (
        shouldWatchdogFailoverToIgw({
          dualPath: dualPathPreferMqtt,
          dataSource: dataSource.value,
          mqttConnected: mqttConnected.value,
        })
      ) {
        logger.log('MQTT connect watchdog — no ConnAck; failing over to IGW')
        void failoverToIgw()
      }
    }, MQTT_CONNECT_WATCHDOG_MS)
  }

  async function startMqtt(config: AppConfig, note?: { title: string; body: string }) {
    if (!inverterEnabled) return
    const current = session
    await invokeTransport('connect_mqtt', mqttConnectArgs(config))
    if (current !== session || !inverterEnabled) return
    dataSource.value = 'mqtt'
    // Real connection is confirmed by mqtt-connection-status (ConnAck), not invoke OK.
    mqttConnected.value = false
    mqttOnlyReconnectAttempt = 0
    stopMqttRecoveryProbe()
    if (note) notify(note.title, note.body)
  }

  async function startIgw(config: AppConfig, note?: { title: string; body: string }) {
    if (!inverterEnabled) return
    clearMqttConnectWatchdog()
    const current = session
    await invokeTransport('connect_gateway', gatewayConnectArgs(config))
    if (current !== session || !inverterEnabled) return
    dataSource.value = 'igw'
    // Real connection is confirmed by mqtt-connection-status (first good poll).
    mqttConnected.value = false
    if (note) notify(note.title, note.body)
  }

  async function connectMqtt() {
    cleanup()
    const current = session
    async function listenForSession<T>(name: string, handler: (event: Event<T>) => void) {
      if (current !== session) return
      const unlisten = await listen<T>(name, (event) => {
        if (current === session) handler(event)
      })
      if (current === session) listeners.push(unlisten)
      else unlisten()
    }
    try {
      const config = await getAppConfig()
      if (current !== session) return
      inverterEnabled = isMqttConfigured(config) || isIgwConfigured(config)
      appConfig.value = config
      if (config.color_scheme) {
        const isDark = config.color_scheme !== 'light'
        document.body.classList.toggle('light', !isDark)
        localStorage.setItem('theme', config.color_scheme)
      }

      await listenForSession<InverterState>('mqtt-state-update', (event) => {
        if (inverterEnabled) processState(event.payload)
      })

      await listenForSession<boolean>('mqtt-connection-status', (event) => {
        if (!inverterEnabled) return
        if (event.payload) {
          if (mqttOfflineTimer) {
            clearTimeout(mqttOfflineTimer)
            mqttOfflineTimer = null
          }
          clearMqttConnectWatchdog()
          mqttConnected.value = true
          mqttOnlyReconnectAttempt = 0
        } else if (!mqttOfflineTimer) {
          mqttOfflineTimer = setTimeout(() => {
            mqttOfflineTimer = null
            mqttConnected.value = false
            // MQTT lost while dual-path preferred MQTT → exclusive failover to IGW.
            if (dualPathPreferMqtt && dataSource.value === 'mqtt') {
              logger.log('Cerbo MQTT offline — failing over to IGW')
              void failoverToIgw()
            }
            // MQTT-only: Rust client reconnects with calm backoff; no frontend hammer.
          }, MQTT_OFFLINE_DELAY_MS)
        }
      })

      await listenForSession<{ video_url: string; agent_name?: string }>(
        'camera-event',
        (event) => {
          if (!appConfig.value?.camera_enabled) return
          const payload = event.payload
          if (!payload?.video_url) return
          void invoke('open_camera_video_window', {
            videoUrl: payload.video_url,
            agentName: payload.agent_name ?? null,
          }).catch((e) => logger.warn('Failed to open camera video window:', e))
        }
      )

      await listenForSession<{ title: string; body: string }>('notification', (event) => {
        addNotification(event.payload.title, event.payload.body)
      })

      await listenForSession<BannerNotification>('mqtt-notification', (event) => {
        upsertBanner(event.payload)
        addNotification(event.payload.title, event.payload.body)
      })

      await listenForSession<{ id: string }>('mqtt-notification-clear', (event) => {
        clearBanner(event.payload.id)
      })

      if (current !== session) return
      const mqttOk = isMqttConfigured(config)
      const igwOk = isIgwConfigured(config)
      dualPathPreferMqtt = mqttOk && igwOk

      let mqttReachable = false
      if (dualPathPreferMqtt) {
        mqttReachable = await probeMqttReachable(config)
        if (current !== session) return
      }

      const startup = chooseStartupSource({
        mqttConfigured: mqttOk,
        igwConfigured: igwOk,
        mqttReachable: dualPathPreferMqtt ? mqttReachable : mqttOk,
      })

      stopMqttRecoveryProbe()

      if (startup === 'mqtt') {
        try {
          await startMqtt(config, { title: 'MQTT', body: 'Connecting to inverter' })
          if (current !== session) return
          if (dualPathPreferMqtt) {
            startMqttConnectWatchdog()
          }
        } catch (e) {
          if (current !== session) return
          logger.error('MQTT connect failed:', e)
          if (igwOk) {
            logger.log('MQTT connect failed — starting IGW')
            await startIgw(config, { title: 'Gateway', body: 'MQTT unavailable — using IGW' })
            if (current !== session) return
            startMqttRecoveryProbe()
          } else {
            throw e
          }
        }
      } else if (startup === 'igw') {
        await startIgw(config, {
          title: 'Gateway',
          body: dualPathPreferMqtt ? 'MQTT unreachable — using IGW' : 'Connected remotely',
        })
        if (current !== session) return
        if (dualPathPreferMqtt) {
          startMqttRecoveryProbe()
        }
      } else {
        logger.log('Neither Cerbo MQTT nor IGW configured')
        dualPathPreferMqtt = false
        mqttConnected.value = false
        dataSource.value = 'mqtt'
        state.value = { booleans: {}, features: {}, ui_config: {} }
        await invokeTransport('disconnect_inverter')
      }

      if (current !== session) return
      await syncHaMqttFromConfig(config)
      if (current !== session) return

      // Listen for HA MQTT connection status changes
      await listenForSession<boolean>('ha-mqtt-connection-status', (event) => {
        if (event.payload) {
          if (haMqttOfflineTimer) {
            clearTimeout(haMqttOfflineTimer)
            haMqttOfflineTimer = null
          }
          haMqttConnected.value = true
        } else if (!haMqttOfflineTimer) {
          haMqttOfflineTimer = setTimeout(() => {
            haMqttOfflineTimer = null
            haMqttConnected.value = false
            if (appConfig.value?.camera_enabled && appConfig.value?.mqtt_ha_host?.trim()) {
              reconnectHaMqttAfterDelay()
            }
          }, MQTT_OFFLINE_DELAY_MS)
        }
      })

      // Auto-reconnect on wake (network change, IP renewal after sleep)
      await listenForSession('window-focused', () => {
        if (!inverterEnabled || mqttConnected.value) {
          return
        }
        logger.log(
          `Wake detected, reconnecting ${dataSource.value === 'igw' ? 'gateway' : 'MQTT'}...`
        )
        if (dualPathPreferMqtt && dataSource.value === 'igw') {
          void tryRecoverMqtt()
        } else if (!dualPathPreferMqtt && isMqttConfigured(config) && !isIgwConfigured(config)) {
          scheduleMqttOnlyReconnect(0)
        } else {
          reconnectAfterDelay(mqttReconnectDelayMs(0))
        }
      })

      try {
        if (!inverterEnabled) return
        const initial = await invoke<InverterState>('get_state')
        if (current === session && inverterEnabled) processState(initial)
      } catch (e) {
        logger.error('Failed to get initial state:', e)
      }
    } catch (e) {
      logger.error('Failed to connect to MQTT:', e)
      if (current === session) mqttConnected.value = false
    }
  }

  let mqttReconnectTimer: ReturnType<typeof setTimeout> | null = null
  let haMqttReconnectTimer: ReturnType<typeof setTimeout> | null = null

  async function failoverToIgw() {
    const current = session
    if (!inverterEnabled) return
    clearMqttConnectWatchdog()
    try {
      const config = await getAppConfig()
      if (current !== session || !inverterEnabled) return
      if (!isIgwConfigured(config)) {
        scheduleMqttOnlyReconnect()
        return
      }
      // connect_gateway stops MQTT (exclusive).
      await startIgw(config, { title: 'Gateway', body: 'MQTT lost — switched to IGW' })
      if (current === session) startMqttRecoveryProbe()
    } catch (e) {
      logger.error('IGW failover failed:', e)
      if (current === session) scheduleMqttOnlyReconnect()
    }
  }

  function stopMqttRecoveryProbe() {
    if (mqttRecoveryTimer) {
      clearInterval(mqttRecoveryTimer)
      mqttRecoveryTimer = null
    }
  }

  function startMqttRecoveryProbe() {
    stopMqttRecoveryProbe()
    if (!dualPathPreferMqtt) return
    mqttRecoveryTimer = setInterval(() => {
      void tryRecoverMqtt()
    }, MQTT_RECOVERY_PROBE_MS)
  }

  async function tryRecoverMqtt() {
    const current = session
    if (!dualPathPreferMqtt || dataSource.value === 'mqtt') return
    try {
      const config = await getAppConfig()
      if (current !== session || !inverterEnabled) return
      if (!isMqttConfigured(config)) return
      const reachable = await probeMqttReachable(config)
      if (!reachable || current !== session || !inverterEnabled) return
      logger.log('Cerbo MQTT reachable again — switching back (stops IGW)')
      stopMqttRecoveryProbe()
      // connect_mqtt stops gateway (exclusive).
      await startMqtt(config, { title: 'MQTT', body: 'Cerbo MQTT restored' })
      if (current === session) startMqttConnectWatchdog()
    } catch {
      // still down / connect failed — stay on IGW, probe again next minute
    }
  }

  function scheduleMqttOnlyReconnect(forceAttempt?: number) {
    if (!inverterEnabled) return
    if (forceAttempt !== undefined) {
      mqttOnlyReconnectAttempt = forceAttempt
    }
    const delay = mqttReconnectDelayMs(mqttOnlyReconnectAttempt)
    mqttOnlyReconnectAttempt += 1
    reconnectAfterDelay(delay)
  }

  function reconnectAfterDelay(delay = mqttReconnectDelayMs(0)) {
    if (!inverterEnabled) return
    if (mqttReconnectTimer) clearTimeout(mqttReconnectTimer)
    mqttReconnectTimer = setTimeout(() => {
      mqttReconnectTimer = null
      connectMqtt()
    }, delay)
  }

  async function connectHaMqtt(config: AppConfig) {
    await invoke('connect_ha_mqtt', {
      host: config.mqtt_ha_host,
      port: config.mqtt_ha_port,
      username: config.mqtt_ha_login || null,
      password: config.mqtt_ha_password || null,
      cameraTopic: config.camera_topic || null,
      frigateBaseUrl: config.frigate_base_url || null,
      ringSnapshotUrlTemplate: config.ring_snapshot_url_template || null,
    })
    haMqttConnected.value = true
  }

  async function disconnectHaMqtt() {
    if (haMqttReconnectTimer) {
      clearTimeout(haMqttReconnectTimer)
      haMqttReconnectTimer = null
    }
    if (haMqttOfflineTimer) {
      clearTimeout(haMqttOfflineTimer)
      haMqttOfflineTimer = null
    }
    try {
      await invoke('disconnect_ha_mqtt')
    } catch (e) {
      logger.warn('disconnect_ha_mqtt failed:', e)
    }
    haMqttConnected.value = null
  }

  /** Connect or disconnect HA MQTT camera client from current config. */
  async function syncHaMqttFromConfig(config: AppConfig) {
    if (config.camera_enabled && config.mqtt_ha_host?.trim() && config.mqtt_ha_port) {
      try {
        await connectHaMqtt(config)
        logger.log('Connected to HA MQTT broker for cameras')
        notify('Home Assistant', 'Connected to HA MQTT')
      } catch (e) {
        haMqttConnected.value = false
        logger.error('Failed to connect to HA MQTT:', e)
      }
    } else {
      await disconnectHaMqtt()
    }
  }

  async function toggleCameraMotion() {
    const config = await getAppConfig()
    const next = !config.camera_enabled
    config.camera_enabled = next
    await invoke('save_config', { config })
    appConfig.value = { ...config }
    await syncHaMqttFromConfig(config)
    return next
  }

  function reconnectHaMqttAfterDelay(delay = 2000) {
    if (haMqttReconnectTimer) clearTimeout(haMqttReconnectTimer)
    haMqttReconnectTimer = setTimeout(async () => {
      haMqttReconnectTimer = null
      try {
        const config = await getAppConfig()
        if (config.camera_enabled && config.mqtt_ha_host?.trim()) {
          await connectHaMqtt(config)
          logger.log('HA MQTT reconnected')
        }
      } catch (e) {
        logger.error('HA MQTT reconnect failed:', e)
        haMqttConnected.value = false
      }
    }, delay)
  }

  function cleanup() {
    session += 1
    inverterEnabled = false
    stopMqttRecoveryProbe()
    clearMqttConnectWatchdog()
    for (const unlisten of listeners) unlisten()
    listeners = []

    if (mqttReconnectTimer) {
      clearTimeout(mqttReconnectTimer)
      mqttReconnectTimer = null
    }
    if (haMqttReconnectTimer) {
      clearTimeout(haMqttReconnectTimer)
      haMqttReconnectTimer = null
    }
    if (mqttOfflineTimer) {
      clearTimeout(mqttOfflineTimer)
      mqttOfflineTimer = null
    }
    if (haMqttOfflineTimer) {
      clearTimeout(haMqttOfflineTimer)
      haMqttOfflineTimer = null
    }
  }

  return {
    state,
    mqttConnected,
    dataSource,
    haMqttConnected,
    appConfig,
    connectMqtt,
    send,
    ensureNotificationPermission,
    toggleCameraMotion,
    syncHaMqttFromConfig,
    cleanup,
  }
}

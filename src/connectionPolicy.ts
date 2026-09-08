/** Pure helpers for MQTT-first / IGW failover policy (unit-tested). */

export type DataSource = 'mqtt' | 'igw'

export type ConnectionConfigSlice = {
  mqtt_host?: string | null
  mqtt_port?: number | null
  gateway_enabled?: boolean
  gateway_url?: string | null
  gateway_access_client_id?: string | null
  gateway_access_client_secret?: string | null
}

/** Cerbo LAN MQTT looks configured (host present). */
export function isMqttConfigured(config: ConnectionConfigSlice): boolean {
  return Boolean(config.mqtt_host?.trim())
}

/** Remote IGW looks configured (enabled + URL + Access credentials). */
export function isIgwConfigured(config: ConnectionConfigSlice): boolean {
  return Boolean(
    config.gateway_enabled &&
    config.gateway_url?.trim() &&
    config.gateway_access_client_id?.trim() &&
    config.gateway_access_client_secret?.trim()
  )
}

export type StartupSource = 'mqtt' | 'igw' | 'none'

/**
 * Choose the exclusive live path at startup when both transports may exist.
 * MQTT wins when reachable; otherwise IGW; single-transport configs are obvious.
 */
export function chooseStartupSource(opts: {
  mqttConfigured: boolean
  igwConfigured: boolean
  mqttReachable: boolean
}): StartupSource {
  const { mqttConfigured, igwConfigured, mqttReachable } = opts
  if (mqttConfigured && igwConfigured) {
    return mqttReachable ? 'mqtt' : 'igw'
  }
  if (mqttConfigured) return 'mqtt'
  if (igwConfigured) return 'igw'
  return 'none'
}

/** While on IGW (dual-path), probe Cerbo MQTT this often before recovering. */
export const MQTT_RECOVERY_PROBE_MS = 60_000

/**
 * After starting MQTT in dual-path mode, if ConnAck / status-true never arrives
 * within this window, fail over to IGW (Rust may reconnect forever without a
 * reliable offline path).
 */
export const MQTT_CONNECT_WATCHDOG_MS = 15_000

/** Debounce before treating mqtt-connection-status false as offline. */
export const MQTT_OFFLINE_DELAY_MS = 10_000

/**
 * Dual-path watchdog: still on MQTT with no real connection → switch to IGW.
 */
export function shouldWatchdogFailoverToIgw(opts: {
  dualPath: boolean
  dataSource: DataSource | string
  mqttConnected: boolean
}): boolean {
  return opts.dualPath && opts.dataSource === 'mqtt' && !opts.mqttConnected
}

/**
 * Calm MQTT-only reconnect delay (ms): ~5s → 10 → 20 → 40 → 60s cap.
 * Avoids tight 2s loops that hammer WAN / the Cerbo when IGW is unavailable.
 */
export function mqttReconnectDelayMs(attempt: number): number {
  const n = Math.max(0, Math.floor(attempt))
  const base = 5_000
  const delay = base * 2 ** Math.min(n, 4)
  return Math.min(delay, 60_000)
}

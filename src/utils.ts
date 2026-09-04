export function formatPower(w: number | undefined): string {
  const abs = Math.abs(Math.floor(w || 0))
  const sign = w && w < 0 ? '-' : ''
  return abs >= 1000 ? sign + (abs / 1000).toFixed(1) + 'kW' : sign + abs + 'W'
}

export function formatUptime(s: number): string {
  if (s < 60) return s + 's'
  if (s < 3600) return Math.floor(s / 60) + 'm'
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  return h + 'h ' + m + 'm'
}

export function formatInverterState(state: string | undefined): string {
  if (!state) return 'Bulk'
  const normalized = state.trim().toLowerCase()
  if (normalized === 'off') return 'Off'
  return state
}

export function formatDuration(s: number | undefined): string {
  if (!s || s <= 0) return '0:00'
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = Math.floor(s % 60)
  if (h > 0) return h + ':' + String(m).padStart(2, '0') + ':' + String(sec).padStart(2, '0')
  return m + ':' + String(sec).padStart(2, '0')
}

/** Relative age matching Victron GUIv2 style (e.g. "10h 16m ago"). */
export function formatTimestamp(tsString: string | undefined): string {
  if (!tsString) return ''
  const timestamp = new Date(tsString)
  if (isNaN(timestamp.getTime())) return ''
  const diffMs = Date.now() - timestamp.getTime()
  if (diffMs < 0) return 'just now'
  const diffSec = Math.floor(diffMs / 1000)
  if (diffSec < 60) return 'just now'
  const diffMin = Math.floor(diffSec / 60)
  if (diffMin < 60) return `${diffMin}m ago`
  const diffHours = Math.floor(diffMin / 60)
  const remMin = diffMin % 60
  if (diffHours < 24) {
    return remMin === 0 ? `${diffHours}h ago` : `${diffHours}h ${remMin}m ago`
  }
  const diffDays = Math.floor(diffHours / 24)
  if (diffDays < 7) return `${diffDays}d ago`
  return timestamp
    .toLocaleString(undefined, {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    })
    .replace(',', '')
}

/** Inverter-control flags owned by inverter-control on Cerbo MQTT. */
export const INVERTER_CONTROL_FLAGS = [
  'only_charging',
  'no_feed',
  'house_support',
  'charge_battery',
  'do_not_supply_charger',
  'set_limit_to_ev_charger',
  'minimize_charging',
] as const

/** Bare key (`only_charging`) or HA-style id (`input_boolean.only_charging`). */
export function inverterControlFlagKey(entityOrId: string): string | null {
  const raw = (entityOrId || '').trim()
  if (!raw) return null
  const key = raw.includes('.') ? (raw.split('.').pop() as string) : raw
  return (INVERTER_CONTROL_FLAGS as readonly string[]).includes(key) ? key : null
}

export function isInverterControlFlag(entityOrId: string): boolean {
  return inverterControlFlagKey(entityOrId) !== null
}

/** Header-toggle display: the 7 control flags always use MQTT `booleans`. */
export function resolveHeaderToggleState(
  toggle: { id: string; entity: string },
  haEntityStates: Record<string, string>,
  haEnabled: boolean,
  mqttBooleans: Record<string, unknown>
): 'on' | 'off' {
  const controlFlag = isInverterControlFlag(toggle.entity) || isInverterControlFlag(toggle.id)
  if (!controlFlag && haEnabled && haEntityStates[toggle.entity] !== undefined) {
    return haEntityStates[toggle.entity] === 'on' ? 'on' : 'off'
  }
  const entityKey = toggle.entity.split('.').pop() || toggle.id
  const rawVal = mqttBooleans[toggle.id] ?? mqttBooleans[entityKey] ?? mqttBooleans[toggle.entity]
  let val: unknown = rawVal
  if (typeof val === 'string') val = val === 'true' || val === '1'
  else if (typeof val === 'number') val = val !== 0
  return val ? 'on' : 'off'
}

export function formatPower(w: number | undefined): string {
  const abs = Math.abs(Math.floor(w || 0))
  const sign = w && w < 0 ? '-' : ''
  return abs >= 1000 ? sign + (abs / 1000).toFixed(1) + 'kW' : sign + abs + 'W'
}

/** Prefer incoming when defined (including explicit 0); else keep previous.
 *  Used by StatCards to avoid flashing 0W / 0% / 0.00V on brief nullish gaps. */
export function holdNumber(
  incoming: number | null | undefined,
  previous: number | undefined
): number | undefined {
  if (incoming !== null && incoming !== undefined && Number.isFinite(incoming)) {
    return incoming
  }
  return previous
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
  if (Number.isNaN(timestamp.getTime())) return ''
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

/** True when HA reports the entity exists but has no usable state. */
export function isHaUnavailableState(state: string | null | undefined): boolean {
  if (state === undefined || state === null) return false
  const lower = String(state).trim().toLowerCase()
  return !lower || lower === 'unavailable' || lower === 'unknown'
}

/** Map raw HA entity state to on / off / unavailable for toggles & home tiles. */
export function normalizeHaToggleState(raw: string): 'on' | 'off' | 'unavailable' {
  const lower = String(raw).trim().toLowerCase()
  if (!lower || lower === 'unavailable' || lower === 'unknown') return 'unavailable'
  // Covers / locks often use open/closed rather than on/off.
  if (lower === 'on' || lower === 'open' || lower === 'opening' || lower === 'unlocked') return 'on'
  return 'off'
}

/** Normalize boolean-like transport values without interpreting "false" as true. */
export function coerceBoolean(value: unknown): boolean {
  return (
    value === true ||
    value === 1 ||
    value === 'true' ||
    value === '1' ||
    value === 'on' ||
    value === 'online'
  )
}

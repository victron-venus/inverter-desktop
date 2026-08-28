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

export function formatTimestamp(tsString: string | undefined): string {
  if (!tsString) return ''
  const now = new Date()
  const timestamp = new Date(tsString)
  if (isNaN(timestamp.getTime())) return ''
  const diffMs = now.getTime() - timestamp.getTime()
  const diffMin = Math.floor(diffMs / (1000 * 60))
  const diffHours = Math.floor(diffMs / (1000 * 60 * 60))

  if (diffMin < 30) {
    return '30 min ago'
  } else if (diffHours < 24) {
    // Show time in HH:MM format
    return timestamp.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
  } else {
    // Show date and time
    return timestamp
      .toLocaleString(undefined, {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      })
      .replace(',', '') // Remove the comma if present
  }
}

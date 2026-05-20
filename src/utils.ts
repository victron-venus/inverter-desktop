export function formatPower(w: number | undefined) {
  const v = Math.abs(Math.floor(w || 0))
  const sign = w && w < 0 ? '-' : ''
  return v >= 1000 ? sign + (v / 1000).toFixed(1) + 'kW' : sign + v + 'W'
}

export function formatUptime(s: number) {
  if (s < 60) return s + 's'
  if (s < 3600) return Math.floor(s / 60) + 'm'
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60)
  return h + 'h ' + m + 'm'
}

export function formatDuration(s: number | undefined) {
  if (!s || s <= 0) return '0:00'
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = Math.floor(s % 60)
  if (h > 0) return h + ':' + String(m).padStart(2, '0') + ':' + String(sec).padStart(2, '0')
  return m + ':' + String(sec).padStart(2, '0')
}

export function escapeHtml(str: string | number | undefined | null): string {
  return String(str ?? '').replace(/[&<>"']/g, (ch) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[ch] ?? ch
  )
}

export function formatSemverLabel(ver: string | undefined) {
  if (ver === null || ver === undefined || ver === '') return '?'
  const s = String(ver).trim()
  if (s === '?') return '?'
  if (/^v\d/i.test(s)) return s
  if (/^\d/.test(s)) return 'v' + s
  return s
}

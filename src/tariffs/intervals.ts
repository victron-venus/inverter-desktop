import { billingPeriod, currentRate, type TariffPlan } from './model'

export const MAX_INTERVAL_BYTES = 2_000_000
const MINUTE = 60_000
const DAY = 86_400_000
export interface EnergyInterval {
  start: string
  end: string
  importKwh: number
}
export interface IntervalHistory {
  type: 'grid-import-intervals'
  version: 1
  intervals: EnergyInterval[]
}
export interface DateRange {
  start: string
  end: string
}

function timestamp(value: unknown): number {
  if (
    typeof value !== 'string' ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:00(?:Z|[+-]\d{2}:\d{2})$/.test(value)
  )
    throw new Error('Use ISO timestamps with an explicit UTC offset and whole minutes.')
  const time = Date.parse(value)
  const date = value.slice(0, 10)
  if (
    !Number.isFinite(time) ||
    date < '2000-01-01' ||
    date > '2099-12-31' ||
    new Date(`${date}T00:00:00Z`).toISOString().slice(0, 10) !== date ||
    Number(value.slice(11, 13)) > 23
  )
    throw new Error('Invalid interval timestamp; supported years are 2000–2099.')
  return time
}

export function validateIntervals(value: unknown): IntervalHistory {
  const data = value as Partial<IntervalHistory> | null
  if (!data || data.type !== 'grid-import-intervals' || data.version !== 1)
    throw new Error('Expected grid-import-intervals version 1.')
  if (!Array.isArray(data.intervals) || !data.intervals.length || data.intervals.length > 20_000)
    throw new Error('Import between 1 and 20,000 measured intervals.')
  const intervals = data.intervals
    .map((row) => {
      if (!row || typeof row !== 'object') throw new Error('Invalid interval row.')
      const start = timestamp(row.start)
      const end = timestamp(row.end)
      if (end <= start || end - start > 60 * MINUTE)
        throw new Error('Intervals must be positive and no longer than one hour.')
      if (typeof row.importKwh !== 'number' || !Number.isFinite(row.importKwh) || row.importKwh < 0)
        throw new Error('Import kWh must be a finite nonnegative number; blank is not zero.')
      return {
        start: new Date(start).toISOString().replace('.000Z', 'Z'),
        end: new Date(end).toISOString().replace('.000Z', 'Z'),
        importKwh: row.importKwh,
      }
    })
    .sort((a, b) => a.start.localeCompare(b.start))
  for (let i = 1; i < intervals.length; i++) {
    if (intervals[i].start < intervals[i - 1].end)
      throw new Error('Overlapping or duplicate intervals would count energy twice.')
  }
  if (Date.parse(intervals[intervals.length - 1].end) - Date.parse(intervals[0].start) > 366 * DAY)
    throw new Error('Import at most 366 days per file.')
  return { type: 'grid-import-intervals', version: 1, intervals }
}

export function parseIntervals(text: string): IntervalHistory {
  if (new TextEncoder().encode(text).length > MAX_INTERVAL_BYTES)
    throw new Error('Interval files must be no larger than 2 MB.')
  const trimmed = text.replace(/^\uFEFF/, '').trim()
  if (trimmed.startsWith('{')) return validateIntervals(JSON.parse(trimmed))
  const lines = trimmed.split(/\r?\n/)
  // All supported fields are scalar: no embedded commas or newlines are valid.
  const cells = (line: string) =>
    line.split(',').map((cell) => {
      const value = cell.trim()
      return /^"[^"]*"$/.test(value) ? value.slice(1, -1) : value
    })
  if (cells(lines.shift() ?? '').join(',') !== 'start,end,import_kwh')
    throw new Error(
      'CSV header must be start,end,import_kwh. Export and net energy are not import energy.'
    )
  const intervals = lines.map((line) => {
    const row = cells(line)
    if (row.length !== 3 || !row[2] || !/^(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?$/i.test(row[2]))
      throw new Error('Each CSV row needs start, end and nonnegative import kWh.')
    return { start: row[0], end: row[1], importKwh: Number(row[2]) }
  })
  return validateIntervals({
    type: 'grid-import-intervals',
    version: 1,
    intervals,
  })
}

const dateFormatters = new Map<string, Intl.DateTimeFormat>()
export function localDate(at: number, timeZone: string): string {
  let formatter = dateFormatters.get(timeZone)
  if (!formatter) {
    if (dateFormatters.size > 10) dateFormatters.clear()
    formatter = new Intl.DateTimeFormat('en-US', {
      timeZone,
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
    })
    dateFormatters.set(timeZone, formatter)
  }
  const parts = formatter.formatToParts(at)
  const part = (name: string) => parts.find((item) => item.type === name)?.value
  return `${part('year')}-${part('month')}-${part('day')}`
}

// Search instants, not a fixed UTC offset: local days may have 23 or 25 hours.
export function dayStart(date: string, timeZone: string): number {
  const utc = Date.parse(`${date}T00:00:00Z`)
  if (
    !/^\d{4}-\d{2}-\d{2}$/.test(date) ||
    !Number.isFinite(utc) ||
    new Date(utc).toISOString().slice(0, 10) !== date
  )
    throw new Error('Select a valid start and end date.')
  let low = (utc - 36 * 3_600_000) / MINUTE
  let high = (utc + 36 * 3_600_000) / MINUTE
  while (low < high) {
    const mid = Math.floor((low + high) / 2)
    if (localDate(mid * MINUTE, timeZone) < date) low = mid + 1
    else high = mid
  }
  if (localDate(low * MINUTE, timeZone) !== date)
    throw new Error('This calendar date does not exist in the tariff time zone.')
  return low * MINUTE
}
export function defaultRange(
  plan: TariffPlan,
  history: IntervalHistory,
  now = Date.now()
): DateRange {
  const last = Math.min(now, Date.parse(history.intervals[history.intervals.length - 1].end) - 1)
  const day = localDate(last, plan.timeZone)
  return billingPeriod(plan, new Date(last)) ?? { start: day, end: day }
}

export function intervalCost(plan: TariffPlan, history: IntervalHistory, range: DateRange) {
  const start = dayStart(range.start, plan.timeZone)
  dayStart(range.end, plan.timeZone) // Validate before Date.parse can normalize invalid dates.
  const nextDay = new Date(Date.parse(`${range.end}T00:00:00Z`) + DAY).toISOString().slice(0, 10)
  const end = dayStart(nextDay, plan.timeZone)
  if (end <= start || end - start > 367 * DAY) throw new Error('Select a period of 1–366 days.')
  let cost = 0
  let importKwh = 0
  let pricedMinutes = 0
  let recordedMinutes = 0
  let unpricedIntervals = 0
  for (const interval of history.intervals) {
    const a = Date.parse(interval.start)
    const b = Date.parse(interval.end)
    if (b <= start || a >= end) continue
    recordedMinutes += (Math.min(b, end) - Math.max(a, start)) / MINUTE
    if (a < start || b > end) {
      unpricedIntervals++
      continue
    }
    const rate = currentRate(plan, new Date(a))
    // Never divide an interval's kWh across prices: its within-interval profile is unknown.
    let constant = true
    for (let at = a + MINUTE; at < b; at += MINUTE) {
      if (currentRate(plan, new Date(at)) !== rate) {
        constant = false
        break
      }
    }
    if (!constant) {
      unpricedIntervals++
      continue
    }
    cost += interval.importKwh * rate
    importKwh += interval.importKwh
    pricedMinutes += (b - a) / MINUTE
  }
  if (!Number.isFinite(cost) || !Number.isFinite(importKwh))
    throw new Error('Energy or cost exceeds the supported numeric range.')
  const expectedMinutes = (end - start) / MINUTE
  return {
    cost: pricedMinutes ? cost : null,
    importKwh,
    pricedMinutes,
    recordedMinutes,
    expectedMinutes,
    missingMinutes: expectedMinutes - recordedMinutes,
    unpricedIntervals,
    complete: pricedMinutes === expectedMinutes,
  }
}

export function intervalKey(scope: string): string {
  return `victron.grid-import-intervals.v1:${scope}`
}
export function loadIntervals(scope: string): IntervalHistory | null {
  const raw = localStorage.getItem(intervalKey(scope))
  return raw === null ? null : parseIntervals(raw)
}
export function saveIntervals(scope: string, history: IntervalHistory): void {
  const normalized = validateIntervals(history)
  const text = JSON.stringify(normalized)
  if (new TextEncoder().encode(text).length > MAX_INTERVAL_BYTES)
    throw new Error('Normalized history exceeds 2 MB.')
  try {
    localStorage.setItem(intervalKey(scope), text)
  } catch {
    throw new Error('Could not save intervals on this device. The previous history is unchanged.')
  }
}

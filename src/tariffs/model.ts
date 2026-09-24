export const DAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']
export const MONTHS = [
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
]
export const SLOTS = 48
export type RateGrid = (number | null)[][]

export interface TariffSeason {
  name: string
  months: number[]
  rates: number[][]
}
export interface TariffPlan {
  version: 2
  name: string
  currency: string
  timeZone: string
  rates: number[][]
  seasons: TariffSeason[]
  billingDay?: number
  source: 'manual' | 'emporia'
  reference?: string
}
export interface TariffDraft extends Omit<TariffPlan, 'rates' | 'seasons'> {
  rates: RateGrid
  seasons: (Omit<TariffSeason, 'rates'> & { rates: RateGrid })[]
}

export function rateGrid(value: number | null = null): RateGrid {
  return Array.from({ length: SLOTS }, () => new Array(7).fill(value))
}
export function slotLabel(slot: number): string {
  const time = (n: number) => `${String(Math.floor(n / 2)).padStart(2, '0')}:${n % 2 ? '30' : '00'}`
  return `${time(slot)}–${time(slot + 1)}`
}
export function newDraft(): TariffDraft {
  return {
    version: 2,
    name: 'Electricity tariff',
    currency: 'USD',
    timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    rates: rateGrid(),
    seasons: [],
    source: 'manual',
  }
}
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Expected a tariff object.')
  return value as Record<string, unknown>
}
function validateRates(value: unknown): number[][] {
  if (!Array.isArray(value) || value.length !== SLOTS)
    throw new Error('A weekly tariff needs 48 half-hour rows.')
  return value.map((row: unknown, slot: number) => {
    if (!Array.isArray(row) || row.length !== 7)
      throw new Error('A tariff needs seven day columns.')
    return row.map((rate: unknown, day: number) => {
      if (typeof rate !== 'number' || !Number.isFinite(rate))
        throw new Error(
          `Enter a numeric rate for ${DAYS[day]}, ${slotLabel(slot)}. Blank cells are not zero.`
        )
      return rate
    })
  })
}
export function validatePlan(value: unknown): TariffPlan {
  const data = object(value)
  if (data.version !== 1 && data.version !== 2) throw new Error('Unsupported tariff version.')
  // Prevent a mislabeled seasonal file being silently treated as a legacy weekly plan.
  if (data.version === 1 && (data.seasons !== undefined || data.billingDay !== undefined))
    throw new Error('Seasons and billing dates require tariff version 2.')
  if (typeof data.name !== 'string' || !data.name.trim() || data.name.length > 120)
    throw new Error('Enter a tariff name (up to 120 characters).')
  if (typeof data.currency !== 'string' || !/^[A-Z]{3}$/.test(data.currency))
    throw new Error('Enter a three-letter currency, for example USD.')
  if (typeof data.timeZone !== 'string' || !data.timeZone)
    throw new Error('Enter the tariff time zone.')
  try {
    new Intl.DateTimeFormat('en', { timeZone: data.timeZone }).format()
  } catch {
    throw new Error('Unknown time zone. Use a name such as America/Los_Angeles.')
  }
  if (data.source !== 'manual' && data.source !== 'emporia')
    throw new Error('Invalid tariff source.')
  const rates = validateRates(data.rates)
  const rawSeasons = data.version === 1 ? [] : data.seasons
  if (!Array.isArray(rawSeasons) || rawSeasons.length > 12)
    throw new Error('A tariff needs a seasons array with at most 12 seasons.')
  const usedMonths = new Set<number>()
  const seasons = rawSeasons.map((value: unknown) => {
    const season = object(value)
    if (typeof season.name !== 'string' || !season.name.trim() || season.name.length > 80)
      throw new Error('Enter a season name (up to 80 characters).')
    if (!Array.isArray(season.months) || season.months.length === 0)
      throw new Error('Select at least one month for each season.')
    const months = season.months
      .map((month: unknown) => {
        if (typeof month !== 'number' || !Number.isInteger(month) || month < 1 || month > 12)
          throw new Error('Season months must be integers from 1 to 12.')
        if (usedMonths.has(month)) throw new Error('Season months must not overlap or repeat.')
        usedMonths.add(month)
        return month
      })
      .sort((a, b) => a - b)
    try {
      return { name: season.name.trim(), months, rates: validateRates(season.rates) }
    } catch (cause) {
      throw new Error(
        `${season.name}: ${cause instanceof Error ? cause.message : 'Invalid rates.'}`
      )
    }
  })
  if (
    data.billingDay !== undefined &&
    (typeof data.billingDay !== 'number' ||
      !Number.isInteger(data.billingDay) ||
      data.billingDay < 1 ||
      data.billingDay > 31)
  )
    throw new Error('Billing start day must be a whole number from 1 to 31, or blank.')
  if (
    data.reference !== undefined &&
    (typeof data.reference !== 'string' || data.reference.length > 200)
  )
    throw new Error('Invalid tariff reference.')
  return {
    version: 2,
    name: data.name.trim(),
    currency: data.currency,
    timeZone: data.timeZone,
    source: data.source,
    rates,
    seasons,
    ...(data.billingDay !== undefined ? { billingDay: data.billingDay as number } : {}),
    ...(data.reference ? { reference: data.reference as string } : {}),
  }
}

// Import only sanitized tariff data, never credentials or a raw account dump.
export function importTariff(value: unknown): { draft: TariffDraft; message: string } {
  const data = object(value)
  if (data.type !== 'emporia-tariff-reference')
    return {
      draft: validatePlan(data),
      message:
        'Imported tariff, including any seasons and billing start day. Review and save to apply it here.',
    }
  if (data.version !== 1 || typeof data.name !== 'string' || data.name.length > 120)
    throw new Error('Invalid Emporia tariff reference.')
  const reference = typeof data.utilityRateGid === 'string' ? data.utilityRateGid : ''
  const rate = reference ? null : data.flatRate
  if (rate !== null && (typeof rate !== 'number' || !Number.isFinite(rate)))
    throw new Error('Invalid Emporia energy rate.')
  const draft: TariffDraft = {
    version: 2,
    name: data.name,
    currency: String(data.currency),
    timeZone: String(data.timeZone),
    source: 'emporia',
    reference,
    rates: rateGrid(rate as number | null),
    seasons: [],
  }
  validatePlan({ ...draft, rates: rateGrid(0) })
  let message =
    'Imported the Emporia flat energy rate (cents converted to currency/kWh). Review it before saving.'
  if (reference) {
    message = `Emporia utility plan ${reference}: its time-of-use schedule is not included in the available device properties. Copy the rates from the Emporia app before saving.`
  } else if (rate === null) {
    message = 'Emporia did not supply an energy rate. Enter the rates before saving.'
  }
  return { draft, message }
}

export function flatRate(plan: TariffPlan): number | null {
  const first = plan.rates[0][0]
  return [plan.rates, ...plan.seasons.map((season) => season.rates)].every((grid) =>
    grid.every((row) => row.every((value) => value === first))
  )
    ? first
    : null
}
function localParts(timeZone: string, at: Date) {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone,
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    weekday: 'long',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(at)
  return (name: string) => parts.find((part) => part.type === name)?.value ?? ''
}
export function activeSeasonIndex(plan: TariffDraft, at = new Date()): number {
  const month = Number(localParts(plan.timeZone, at)('month'))
  return plan.seasons.findIndex((season) => season.months.includes(month))
}
export function currentRate(plan: TariffPlan, at = new Date()): number {
  const part = localParts(plan.timeZone, at)
  const month = Number(part('month'))
  const rates = plan.seasons.find((season) => season.months.includes(month))?.rates ?? plan.rates
  return rates[Number(part('hour')) * 2 + (Number(part('minute')) >= 30 ? 1 : 0)][
    DAYS.indexOf(part('weekday'))
  ]
}

// Calendar dates, not instants: DST does not change a billing boundary's local date.
export function billingPeriod(
  plan: TariffPlan,
  at = new Date()
): { start: string; end: string } | null {
  if (plan.billingDay === undefined) return null
  const part = localParts(plan.timeZone, at)
  const year = Number(part('year'))
  const month = Number(part('month')) - 1
  const today = Date.UTC(year, month, Number(part('day')))
  const boundary = (offset: number) =>
    Date.UTC(
      year,
      month + offset,
      Math.min(
        plan.billingDay as number,
        new Date(Date.UTC(year, month + offset + 1, 0)).getUTCDate()
      )
    )
  const offset = today >= boundary(0) ? 0 : -1
  const date = (value: number) => new Date(value).toISOString().slice(0, 10)
  return { start: date(boundary(offset)), end: date(boundary(offset + 1) - 86_400_000) }
}
export function estimateDailyCost(plan: TariffPlan | null, kwh: unknown): number | null {
  if (!plan || typeof kwh !== 'number' || !Number.isFinite(kwh) || kwh < 0) return null
  const rate = flatRate(plan)
  // Daily totals cannot be assigned to individual time-of-use periods retroactively.
  return rate === null ? null : kwh * rate
}

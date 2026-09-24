export const DAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']
export const SLOTS = 48
export type RateGrid = (number | null)[][]

export interface TariffPlan {
  version: 1
  name: string
  currency: string
  timeZone: string
  rates: number[][]
  source: 'manual' | 'emporia'
  reference?: string
}

export interface TariffDraft extends Omit<TariffPlan, 'rates'> {
  rates: RateGrid
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
    version: 1,
    name: 'Electricity tariff',
    currency: 'USD',
    timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    rates: rateGrid(),
    source: 'manual',
  }
}

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Expected a tariff object.')
  return value as Record<string, unknown>
}

export function validatePlan(value: unknown): TariffPlan {
  const data = object(value)
  if (data.version !== 1) throw new Error('Unsupported tariff version.')
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
  if (!Array.isArray(data.rates) || data.rates.length !== SLOTS)
    throw new Error('A weekly tariff needs 48 half-hour rows.')
  const rates = data.rates.map((row: unknown, slot: number) => {
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
  if (
    data.reference !== undefined &&
    (typeof data.reference !== 'string' || data.reference.length > 200)
  )
    throw new Error('Invalid tariff reference.')
  return {
    version: 1,
    name: data.name.trim(),
    currency: data.currency,
    timeZone: data.timeZone,
    source: data.source,
    rates,
    ...(data.reference ? { reference: data.reference as string } : {}),
  }
}

// This imports the sanitized companion export, never Emporia credentials or a
// raw account dump. A utility plan ID does not contain its actual YOU schedule.
export function importTariff(value: unknown): { draft: TariffDraft; message: string } {
  const data = object(value)
  if (data.type !== 'emporia-tariff-reference')
    return {
      draft: validatePlan(data),
      message: 'Imported tariff. Review and save to apply it here.',
    }
  if (data.version !== 1 || typeof data.name !== 'string' || data.name.length > 120)
    throw new Error('Invalid Emporia tariff reference.')
  const reference = typeof data.utilityRateGid === 'string' ? data.utilityRateGid : ''
  const rate = reference ? null : data.flatRate
  if (rate !== null && (typeof rate !== 'number' || !Number.isFinite(rate)))
    throw new Error('Invalid Emporia energy rate.')
  const draft: TariffDraft = {
    version: 1,
    name: data.name,
    currency: String(data.currency),
    timeZone: String(data.timeZone),
    source: 'emporia',
    reference,
    rates: rateGrid(rate as number | null),
  }
  // Validate metadata independently of an intentionally incomplete schedule.
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
  return plan.rates.every((row) => row.every((value) => value === first)) ? first : null
}

export function currentRate(plan: TariffPlan, at = new Date()): number {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone: plan.timeZone,
    weekday: 'long',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(at)
  const part = (name: string) => parts.find((p) => p.type === name)?.value ?? ''
  return plan.rates[Number(part('hour')) * 2 + (Number(part('minute')) >= 30 ? 1 : 0)][
    DAYS.indexOf(part('weekday'))
  ]
}

export function estimateDailyCost(plan: TariffPlan | null, kwh: unknown): number | null {
  if (!plan || typeof kwh !== 'number' || !Number.isFinite(kwh) || kwh < 0) return null
  const rate = flatRate(plan)
  // Daily totals cannot be assigned to individual YOU periods retroactively.
  return rate === null ? null : kwh * rate
}

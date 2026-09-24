import { describe, expect, it, vi, afterEach, beforeEach } from 'vitest'
import {
  billingPeriod,
  currentRate,
  estimateDailyCost,
  importTariff,
  newDraft,
  rateGrid,
  validatePlan,
} from './model'
import { loadTariff, saveTariff } from './storage'

const plan = () =>
  validatePlan({ ...newDraft(), timeZone: 'America/Los_Angeles', rates: rateGrid(0.31) })
beforeEach(() => {
  const data = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: vi.fn((key: string, value: string) => {
      data.set(key, value)
    }),
    clear: () => data.clear(),
  })
})
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})
describe('electricity tariff', () => {
  it('calculates only a flat energy estimate from daily kWh without rounding the input', () => {
    expect(estimateDailyCost(plan(), 1.234)).toBeCloseTo(0.38254)
    const weekly = plan()
    weekly.rates[32][0] = 0.6
    expect(estimateDailyCost(weekly, 10)).toBeNull()
    expect(estimateDailyCost(plan(), undefined)).toBeNull()
    expect(estimateDailyCost(plan(), -1)).toBeNull()
  })
  it('uses tariff time zone and half-hour boundaries across DST, including Sunday', () => {
    const weekly = plan()
    weekly.rates[3][6] = 0.6
    weekly.rates[4][6] = 0.8
    expect(currentRate(weekly, new Date('2026-11-01T08:30:00Z'))).toBe(0.6)
    expect(currentRate(weekly, new Date('2026-11-01T09:30:00Z'))).toBe(0.6)
    expect(currentRate(weekly, new Date('2026-11-01T10:00:00Z'))).toBe(0.8)
  })
  it('rejects blanks, numeric strings, infinity and invalid time zones', () => {
    for (const value of [null, '', '0.31', true, Number.NaN, Infinity]) {
      expect(() => validatePlan({ ...plan(), rates: rateGrid(value as number) })).toThrow()
    }
    expect(() => validatePlan({ ...plan(), timeZone: 'Nowhere/Home' })).toThrow()
    expect(estimateDailyCost(validatePlan({ ...plan(), rates: rateGrid(0) }), 10)).toBe(0)
    expect(estimateDailyCost(validatePlan({ ...plan(), rates: rateGrid(-0.1) }), 10)).toBe(-1)
  })
  it('does not substitute a utility plan ID with a flat fallback', () => {
    const imported = importTariff({
      version: 1,
      type: 'emporia-tariff-reference',
      name: 'Emporia',
      currency: 'USD',
      timeZone: 'America/Los_Angeles',
      utilityRateGid: '1234',
      flatRate: 0.31,
    })
    expect(imported.draft.rates[0][0]).toBeNull()
    expect(() => validatePlan(imported.draft)).toThrow()
    expect(imported.message).toContain('Copy the rates')
  })
  it('saves validated tariffs by dashboard scope and rejects corrupt storage', () => {
    saveTariff('site-a', plan())
    expect(loadTariff('site-a').plan).toEqual(plan())
    expect(loadTariff('site-b').plan).toBeNull()
    localStorage.setItem('victron.energy-tariff.v1:bad', '{}')
    expect(loadTariff('bad').error).not.toBe('')
  })
  it('reports storage failure instead of applying an unsaved plan', () => {
    vi.mocked(localStorage.setItem).mockImplementation(() => {
      throw new Error('Quota')
    })
    expect(() => saveTariff('site', plan())).toThrow('could not be saved')
  })
})

describe('seasonal tariffs and billing calendar', () => {
  const seasonal = () =>
    validatePlan({
      ...plan(),
      billingDay: 17,
      seasons: [{ name: 'Summer', months: [6, 7, 8, 9], rates: rateGrid(0.5) }],
    })
  it('changes season at local midnight, independently of the billing cycle', () => {
    const tariff = seasonal()
    expect(currentRate(tariff, new Date('2026-06-01T06:59:59Z'))).toBe(0.31)
    expect(currentRate(tariff, new Date('2026-06-01T07:00:00Z'))).toBe(0.5)
    expect(currentRate(tariff, new Date('2026-10-01T06:59:59Z'))).toBe(0.5)
    expect(currentRate(tariff, new Date('2026-10-01T07:00:00Z'))).toBe(0.31)
    expect(billingPeriod(tariff, new Date('2026-10-01T07:00:00Z'))).toEqual({
      start: '2026-09-17',
      end: '2026-10-16',
    })
    // An aggregate daily total must not be priced as an all-year flat tariff.
    expect(estimateDailyCost(tariff, 10)).toBeNull()
  })
  it('selects the seasonal weekday and half-hour slot', () => {
    const tariff = seasonal()
    tariff.seasons[0].rates[31][6] = 0.7
    expect(currentRate(tariff, new Date('2026-09-27T22:29:59Z'))).toBe(0.5)
    expect(currentRate(tariff, new Date('2026-09-27T22:30:00Z'))).toBe(0.7)
    expect(currentRate(tariff, new Date('2026-09-28T22:30:00Z'))).toBe(0.5)
  })
  it('rejects ambiguous or incomplete season schedules and invalid billing days', () => {
    for (const months of [[], [0], [13], [6.5], ['6'], [6, 6]]) {
      expect(() =>
        validatePlan({ ...plan(), seasons: [{ name: 'Bad', months, rates: rateGrid(1) }] })
      ).toThrow()
    }
    expect(() =>
      validatePlan({ ...seasonal(), seasons: [seasonal().seasons[0], seasonal().seasons[0]] })
    ).toThrow('overlap')
    expect(() =>
      validatePlan({ ...plan(), seasons: [{ name: 'Summer', months: [6], rates: rateGrid() }] })
    ).toThrow('Summer')
    for (const billingDay of [0, 32, 17.5, '17', null, Number.NaN]) {
      expect(() => validatePlan({ ...plan(), billingDay })).toThrow('Billing start day')
    }
  })
  it('preserves all seasons and billing dates through import, storage and JSON export', () => {
    const imported = importTariff(JSON.parse(JSON.stringify(seasonal())))
    saveTariff('seasonal-site', validatePlan(imported.draft))
    expect(loadTariff('seasonal-site').plan).toEqual(seasonal())
  })
  it('migrates version 1 without inventing billing metadata and rejects mislabeled new fields', () => {
    const { seasons: _seasons, ...legacy } = plan()
    const migrated = importTariff({ ...legacy, version: 1 }).draft
    expect(migrated.version).toBe(2)
    expect(migrated.rates).toEqual(legacy.rates)
    expect(migrated.seasons).toEqual([])
    expect(migrated.billingDay).toBeUndefined()
    expect(() => importTariff({ ...seasonal(), version: 1 })).toThrow('version 2')
  })
  it('rolls the billing period at local midnight on the configured day', () => {
    expect(billingPeriod(seasonal(), new Date('2026-09-17T06:59:59Z'))).toEqual({
      start: '2026-08-17',
      end: '2026-09-16',
    })
    expect(billingPeriod(seasonal(), new Date('2026-09-17T07:00:00Z'))).toEqual({
      start: '2026-09-17',
      end: '2026-10-16',
    })
    expect(billingPeriod(seasonal(), new Date('2026-11-17T08:00:00Z'))).toEqual({
      start: '2026-11-17',
      end: '2026-12-16',
    })
    expect(billingPeriod(seasonal(), new Date('2027-01-01T12:00:00Z'))).toEqual({
      start: '2026-12-17',
      end: '2027-01-16',
    })
    expect(billingPeriod(plan())).toBeNull()
  })
  it('clamps days 29–31 in short months without drifting subsequent boundaries', () => {
    const tariff = validatePlan({ ...plan(), billingDay: 31 })
    expect(billingPeriod(tariff, new Date('2026-02-27T20:00:00Z'))).toEqual({
      start: '2026-01-31',
      end: '2026-02-27',
    })
    expect(billingPeriod(tariff, new Date('2026-02-28T20:00:00Z'))).toEqual({
      start: '2026-02-28',
      end: '2026-03-30',
    })
    expect(billingPeriod(tariff, new Date('2028-02-29T20:00:00Z'))).toEqual({
      start: '2028-02-29',
      end: '2028-03-30',
    })
    expect(billingPeriod(tariff, new Date('2026-03-31T20:00:00Z'))).toEqual({
      start: '2026-03-31',
      end: '2026-04-29',
    })
  })
})

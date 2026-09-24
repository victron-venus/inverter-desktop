import { describe, expect, it, vi, afterEach, beforeEach } from 'vitest'
import {
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
    const you = plan()
    you.rates[32][0] = 0.6
    expect(estimateDailyCost(you, 10)).toBeNull()
    expect(estimateDailyCost(plan(), undefined)).toBeNull()
    expect(estimateDailyCost(plan(), -1)).toBeNull()
  })
  it('uses tariff time zone and half-hour boundaries across DST, including Sunday', () => {
    const you = plan()
    you.rates[3][6] = 0.6
    you.rates[4][6] = 0.8
    expect(currentRate(you, new Date('2026-11-01T08:30:00Z'))).toBe(0.6)
    expect(currentRate(you, new Date('2026-11-01T09:30:00Z'))).toBe(0.6)
    expect(currentRate(you, new Date('2026-11-01T10:00:00Z'))).toBe(0.8)
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

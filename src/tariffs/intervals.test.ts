import { describe, expect, it } from 'vitest'
import { billingPeriod, newDraft, rateGrid, validatePlan } from './model'
import {
  dayStart,
  defaultRange,
  intervalCost,
  parseIntervals,
  validateIntervals,
} from './intervals'

const plan = (timeZone = 'America/Los_Angeles') =>
  validatePlan({
    ...newDraft(),
    timeZone,
    rates: rateGrid(0.2),
    billingDay: 17,
  })
const history = (rows: [string, string, number][]) =>
  validateIntervals({
    type: 'grid-import-intervals',
    version: 1,
    intervals: rows.map(([start, end, importKwh]) => ({
      start,
      end,
      importKwh,
    })),
  })
const csv = 'start,end,import_kwh\n2026-09-24T00:00:00-07:00,2026-09-24T00:30:00-07:00,0.25'

describe('measured interval import and costing', () => {
  it('parses real CSV columns, quoted cells, JSON and explicit zero without unit conversion', () => {
    const data = parseIntervals(csv)
    expect(data.intervals[0]).toEqual({
      start: '2026-09-24T07:00:00Z',
      end: '2026-09-24T07:30:00Z',
      importKwh: 0.25,
    })
    expect(parseIntervals(JSON.stringify(data))).toEqual(data)
    expect(parseIntervals(csv.replace('0.25', '"0"')).intervals[0].importKwh).toBe(0)
  })
  it.each([
    csv.replace('import_kwh', 'net_kwh'),
    csv.replace('0.25', ''),
    csv.replace('0.25', '-1'),
    csv.replace('0.25', 'NaN'),
    csv.replace('0.25', '1e999'),
    csv.replace(/-07:00/g, ''),
    csv.replace(/2026-09-24/g, '2026-02-30'),
    csv.replace('00:30:00', '02:30:00'),
    `${csv}\n${csv.split('\n')[1]}`,
  ])('refuses ambiguous, missing, invalid and duplicate data', (input) =>
    expect(() => parseIntervals(input)).toThrow()
  )
  it('adds kWh × each applicable time-of-use price and reports partial coverage', () => {
    const tariff = plan()
    tariff.rates[32] = Array(7).fill(0.5)
    const rows = history([
      ['2026-09-24T15:30:00-07:00', '2026-09-24T16:00:00-07:00', 2],
      ['2026-09-24T16:00:00-07:00', '2026-09-24T16:30:00-07:00', 3],
    ])
    expect(intervalCost(tariff, rows, { start: '2026-09-24', end: '2026-09-24' })).toMatchObject({
      cost: 1.9,
      importKwh: 5,
      pricedMinutes: 60,
      expectedMinutes: 1440,
      missingMinutes: 1380,
      complete: false,
    })
  })
  it('does not allocate a price-crossing or period-crossing aggregate, including an internal change', () => {
    const tariff = plan()
    tariff.rates[32] = Array(7).fill(0.5)
    const rows = history([
      ['2026-09-23T23:30:00-07:00', '2026-09-24T00:30:00-07:00', 5],
      ['2026-09-24T15:45:00-07:00', '2026-09-24T16:45:00-07:00', 2],
    ])
    expect(intervalCost(tariff, rows, { start: '2026-09-24', end: '2026-09-24' })).toMatchObject({
      cost: null,
      importKwh: 0,
      recordedMinutes: 90,
      pricedMinutes: 0,
      unpricedIntervals: 2,
    })
  })
  it('respects seasonal boundaries and negative rates without export netting', () => {
    const tariff = plan()
    tariff.seasons = [{ name: 'September', months: [9], rates: rateGrid(0.5) as number[][] }]
    tariff.rates = rateGrid(-0.1) as number[][]
    const rows = history([
      ['2026-09-30T23:30:00-07:00', '2026-10-01T00:00:00-07:00', 2],
      ['2026-10-01T00:00:00-07:00', '2026-10-01T00:30:00-07:00', 3],
    ])
    expect(intervalCost(tariff, rows, { start: '2026-09-30', end: '2026-10-01' }).cost).toBeCloseTo(
      0.7
    )
  })
  it.each([
    ['2026-03-08', '2026-03-08T08:00:00Z', 23],
    ['2026-11-01', '2026-11-01T07:00:00Z', 25],
  ])(
    'covers all actual hours of DST day %s, retaining repeated-hour intervals',
    (date, instant, hours) => {
      const start = Date.parse(instant)
      const rows: [string, string, number][] = []
      for (let i = 0; i < Number(hours) * 2; i++) {
        const stamp = (n: number) =>
          new Date(start + n * 1_800_000).toISOString().replace('.000Z', 'Z')
        rows.push([stamp(i), stamp(i + 1), 1])
      }
      const result = intervalCost(plan(), history(rows), {
        start: date,
        end: date,
      })
      expect(result).toMatchObject({
        complete: true,
        expectedMinutes: Number(hours) * 60,
        missingMinutes: 0,
      })
      expect(result.cost).toBeCloseTo(Number(hours) * 0.4)
    }
  )
  it('finds billing day 17 from latest reading and handles fractional timezone offsets', () => {
    const data = parseIntervals(csv)
    expect(defaultRange(plan(), data, Date.parse('2026-09-25T00:00:00Z'))).toEqual({
      start: '2026-09-17',
      end: '2026-10-16',
    })
    expect(dayStart('2026-09-24', 'Asia/Kathmandu')).toBe(Date.parse('2026-09-23T18:15:00Z'))
    expect(billingPeriod(plan(), new Date('2026-09-17T06:59:00Z'))?.end).toBe('2026-09-16')
  })
  it('rejects overflow instead of showing an infinite charge', () => {
    const tariff = plan()
    tariff.rates = rateGrid(2) as number[][]
    const data = parseIntervals(csv.replace('0.25', '1e308'))
    expect(() => intervalCost(tariff, data, { start: '2026-09-24', end: '2026-09-24' })).toThrow(
      'numeric range'
    )
  })
})

it('rejects an invalid selected end date instead of normalizing it', () => {
  expect(() =>
    intervalCost(plan(), parseIntervals(csv), {
      start: '2026-02-01',
      end: '2026-02-30',
    })
  ).toThrow('valid start and end')
})

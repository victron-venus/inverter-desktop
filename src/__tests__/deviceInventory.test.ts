import { describe, expect, it } from 'vitest'
import {
  DISCOVERY_INTERVAL_MS,
  lockName,
  mergeDeviceInventory,
  nameRank,
  shouldDiscover,
} from '../deviceInventory'

describe('nameRank', () => {
  it('returns 0 for empty/missing', () => {
    expect(nameRank()).toBe(0)
    expect(nameRank('')).toBe(0)
    expect(nameRank('   ')).toBe(0)
  })
  it('ranks bare MPPT = 1', () => {
    expect(nameRank('MPPT')).toBe(1)
    expect(nameRank('battery')).toBe(1)
  })
  it('ranks numbered fallback MPPT-0 = 2', () => {
    expect(nameRank('MPPT-0')).toBe(2)
    expect(nameRank('Battery 2')).toBe(2)
    expect(nameRank('PV Inverter-3')).toBe(2)
  })
  it('ranks product name = 3', () => {
    expect(nameRank('SmartSolar 100/20')).toBe(3)
    expect(nameRank('BlueSolar 75/15')).toBe(3)
  })
  it('ranks custom name = 4', () => {
    expect(nameRank('Roof Array')).toBe(4)
    expect(nameRank('Garage Battery')).toBe(4)
  })
})

describe('lockName', () => {
  it('returns incoming when current is empty', () => {
    expect(lockName(undefined, 'MPPT-0')).toBe('MPPT-0')
    expect(lockName('', 'Roof Array')).toBe('Roof Array')
  })
  it('returns current when incoming is empty', () => {
    expect(lockName('Roof Array', undefined)).toBe('Roof Array')
    expect(lockName('Roof Array', '')).toBe('Roof Array')
  })
  it('keeps custom name over incoming MPPT fallback', () => {
    expect(lockName('Roof Array', 'MPPT-0')).toBe('Roof Array')
  })
  it('upgrades MPPT fallback to custom', () => {
    expect(lockName('MPPT-0', 'Roof Array')).toBe('Roof Array')
  })
  it('upgrades product name to custom', () => {
    expect(lockName('SmartSolar 100/20', 'Roof Array')).toBe('Roof Array')
  })
  it('downgrades nothing when both are same rank', () => {
    expect(lockName('MPPT-0', 'MPPT-1')).toBe('MPPT-1')
    expect(lockName('Roof Array', 'Yard Array')).toBe('Yard Array')
  })
})

describe('mergeDeviceInventory', () => {
  it('returns incoming when dest is empty', () => {
    const inc = [{ name: 'A', serial: 's1' }]
    expect(mergeDeviceInventory(undefined, inc)).toEqual(inc)
  })
  it('returns empty when both empty', () => {
    expect(mergeDeviceInventory([], [])).toEqual([])
  })
  it('matches by serial and updates metrics', () => {
    const existing = [{ name: 'MPPT-0', serial: 's1', power: 100 }]
    const incoming = [{ name: 'MPPT-0', serial: 's1', power: 200 }]
    const result = mergeDeviceInventory(existing, incoming)
    expect(result).toHaveLength(1)
    expect(result[0].power).toBe(200)
  })
  it('matches by instance when serial missing', () => {
    const existing = [{ name: 'MPPT-0', instance: 0, power: 100 }]
    const incoming = [{ name: 'MPPT-0', instance: 0, power: 250 }]
    const result = mergeDeviceInventory(existing, incoming)
    expect(result).toHaveLength(1)
    expect(result[0].power).toBe(250)
  })
  it('keeps both when no identity match (different serials)', () => {
    const existing = [{ name: 'MPPT-0', serial: 's1' }]
    const incoming = [{ name: 'MPPT-1', serial: 's2' }]
    const result = mergeDeviceInventory(existing, incoming)
    expect(result).toHaveLength(2)
  })
  it('does not add new devices when addNew=false', () => {
    const existing = [{ name: 'MPPT-0', serial: 's1' }]
    const incoming = [
      { name: 'MPPT-0', serial: 's1', power: 5 },
      { name: 'MPPT-1', serial: 's2', power: 10 },
    ]
    const result = mergeDeviceInventory(existing, incoming, { addNew: false })
    expect(result).toHaveLength(1)
    expect(result[0].power).toBe(5)
  })
  it('preserves custom name against incoming MPPT-0', () => {
    const existing = [{ name: 'Roof Array', serial: 's1' }]
    const incoming = [{ name: 'MPPT-0', serial: 's1', power: 50 }]
    const result = mergeDeviceInventory(existing, incoming)
    expect(result[0].name).toBe('Roof Array')
    expect(result[0].power).toBe(50)
  })
  it('upgrades MPPT-0 to incoming custom name on first sight via name match', () => {
    const existing = [{ name: 'MPPT-0', serial: 's1' }]
    const incoming = [{ name: 'Roof Array', serial: 's1' }]
    const result = mergeDeviceInventory(existing, incoming)
    expect(result[0].name).toBe('Roof Array')
  })
  it('fallback name dedupe when neither serial nor instance present', () => {
    const existing = [{ name: 'Battery 1' }]
    const incoming = [{ name: 'Battery 1', power: 200 }]
    const result = mergeDeviceInventory(existing, incoming)
    expect(result).toHaveLength(1)
    expect(result[0].power).toBe(200)
  })
})

describe('shouldDiscover', () => {
  it('first run = true', () => {
    expect(shouldDiscover(null, Date.now())).toBe(true)
  })
  it('within interval = false', () => {
    const t = Date.now()
    expect(shouldDiscover(t, t + 1000)).toBe(false)
  })
  it('past interval = true', () => {
    const t = 1_000_000
    expect(shouldDiscover(t, t + DISCOVERY_INTERVAL_MS + 1)).toBe(true)
  })
})

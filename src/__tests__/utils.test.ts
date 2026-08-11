import { describe, it, expect } from 'vitest'
import { formatPower, formatUptime, formatDuration, formatInverterState } from '../utils'

describe('formatPower', () => {
  it('formats watts below 1000', () => {
    expect(formatPower(500)).toBe('500W')
    expect(formatPower(0)).toBe('0W')
    expect(formatPower(999)).toBe('999W')
  })

  it('formats kilowatts at or above 1000', () => {
    expect(formatPower(1000)).toBe('1.0kW')
    expect(formatPower(1500)).toBe('1.5kW')
    expect(formatPower(12345)).toBe('12.3kW')
  })

  it('handles undefined', () => {
    expect(formatPower(undefined)).toBe('0W')
  })

  it('handles negative values', () => {
    expect(formatPower(-500)).toBe('-500W')
    expect(formatPower(-1500)).toBe('-1.5kW')
  })
})

describe('formatUptime', () => {
  it('formats seconds', () => {
    expect(formatUptime(30)).toBe('30s')
  })

  it('formats minutes', () => {
    expect(formatUptime(120)).toBe('2m')
    expect(formatUptime(3599)).toBe('59m')
  })

  it('formats hours and minutes', () => {
    expect(formatUptime(3600)).toBe('1h 0m')
    expect(formatUptime(3661)).toBe('1h 1m')
    expect(formatUptime(7260)).toBe('2h 1m')
  })
})

describe('formatInverterState', () => {
  it('maps off codes to Off', () => {
    expect(formatInverterState('OF')).toBe('Off')
    expect(formatInverterState('of')).toBe('Off')
    expect(formatInverterState('off')).toBe('Off')
  })

  it('passes through other states', () => {
    expect(formatInverterState('Bulk')).toBe('Bulk')
    expect(formatInverterState('Absorbing')).toBe('Absorbing')
    expect(formatInverterState('Inverting')).toBe('Inverting')
  })

  it('returns Bulk for undefined', () => {
    expect(formatInverterState(undefined)).toBe('Bulk')
  })
})

describe('formatDuration', () => {
  it('returns 0:00 for zero or undefined', () => {
    expect(formatDuration(0)).toBe('0:00')
    expect(formatDuration(undefined)).toBe('0:00')
    expect(formatDuration(-1)).toBe('0:00')
  })

  it('formats minutes and seconds', () => {
    expect(formatDuration(65)).toBe('1:05')
    expect(formatDuration(3661)).toBe('1:01:01')
  })

  it('formats hours', () => {
    expect(formatDuration(7200)).toBe('2:00:00')
  })
})

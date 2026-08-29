import { describe, it, expect } from 'vitest'
import {
  formatPower,
  formatUptime,
  formatDuration,
  formatInverterState,
  isInverterControlFlag,
  resolveHeaderToggleState,
} from '../utils'

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
  it('maps off code to Off', () => {
    expect(formatInverterState('off')).toBe('Off')
  })

  it('passes through other states', () => {
    expect(formatInverterState('OF')).toBe('OF')
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

describe('isInverterControlFlag', () => {
  it('matches the 7 flags as bare keys and input_boolean.*', () => {
    const keys = [
      'only_charging',
      'no_feed',
      'house_support',
      'charge_battery',
      'do_not_supply_charger',
      'set_limit_to_ev_charger',
      'minimize_charging',
    ]
    for (const key of keys) {
      expect(isInverterControlFlag(key)).toBe(true)
      expect(isInverterControlFlag(`input_boolean.${key}`)).toBe(true)
    }
  })

  it('rejects non-flag strings', () => {
    expect(isInverterControlFlag('garage_door')).toBe(false)
    expect(isInverterControlFlag('switch.living_room')).toBe(false)
    expect(isInverterControlFlag('')).toBe(false)
    expect(isInverterControlFlag('minimize_charging_extra')).toBe(false)
  })
})

describe('resolveHeaderToggleState', () => {
  it('uses MQTT for inverter-control flags even when HA is enabled', () => {
    const toggle = { id: 'only_charging', entity: 'input_boolean.only_charging' }
    expect(
      resolveHeaderToggleState(toggle, { 'input_boolean.only_charging': 'on' }, true, {})
    ).toBe('off')
    expect(
      resolveHeaderToggleState(toggle, { 'input_boolean.only_charging': 'off' }, true, {
        only_charging: true,
      })
    ).toBe('on')
  })

  it('uses HA for non-flag entities when HA is enabled', () => {
    const toggle = { id: 'garage', entity: 'switch.garage' }
    expect(resolveHeaderToggleState(toggle, { 'switch.garage': 'on' }, true, {})).toBe('on')
    expect(resolveHeaderToggleState(toggle, { 'switch.garage': 'off' }, true, {})).toBe('off')
  })

  it('falls back to MQTT booleans when HA is disabled', () => {
    const toggle = { id: 'only_charging', entity: 'input_boolean.only_charging' }
    expect(resolveHeaderToggleState(toggle, {}, false, { only_charging: true })).toBe('on')
    expect(resolveHeaderToggleState(toggle, {}, false, { only_charging: false })).toBe('off')
  })

  it('handles string/number MQTT values', () => {
    const toggle = { id: 'no_feed', entity: 'input_boolean.no_feed' }
    expect(resolveHeaderToggleState(toggle, {}, false, { no_feed: 'true' })).toBe('on')
    expect(resolveHeaderToggleState(toggle, {}, false, { no_feed: 1 })).toBe('on')
    expect(resolveHeaderToggleState(toggle, {}, false, { no_feed: 0 })).toBe('off')
  })
})

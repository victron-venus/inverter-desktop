import { describe, it, expect } from 'vitest'

const HA_DOMAINS = new Set(['switch', 'light', 'input_boolean', 'fan', 'cover', 'lock', 'media_player', 'scene', 'script', 'number', 'sensor', 'binary_sensor'])

function isHaEntity(entityId: string): boolean {
  if (!entityId || typeof entityId !== 'string') return false
  const parts = entityId.split('.')
  return parts.length === 2 && HA_DOMAINS.has(parts[0])
}

describe('isHaEntity', () => {
  it('returns true for valid HA entity IDs', () => {
    expect(isHaEntity('switch.shutoff_valve')).toBe(true)
    expect(isHaEntity('light.kitchen')).toBe(true)
    expect(isHaEntity('binary_sensor.motion')).toBe(true)
  })

  it('returns false for invalid entity IDs', () => {
    expect(isHaEntity('')).toBe(false)
    expect(isHaEntity('no_dot')).toBe(false)
    expect(isHaEntity('unknown.foo')).toBe(false)
  })

  it('returns false for non-string input', () => {
    expect(isHaEntity(null as unknown as string)).toBe(false)
    expect(isHaEntity(undefined as unknown as string)).toBe(false)
  })
})

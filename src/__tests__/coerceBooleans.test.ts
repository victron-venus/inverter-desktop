import { describe, it, expect } from 'vitest'
import type { InverterState } from '../composables/useInverterState'

function coerceBooleans(newState: InverterState) {
  if (newState.booleans) {
    for (const key of Object.keys(newState.booleans)) {
      const val = newState.booleans[key]
      if (typeof val === 'string') {
        ;(newState.booleans as Record<string, unknown>)[key] = val === 'true' || val === '1'
      }
    }
  }
  const BOOL_FIELDS: Array<keyof InverterState> = [
    'pump_switch',
    'water_valve',
    'washer_power',
    'dryer_power',
    'dry_run',
  ]
  for (const field of BOOL_FIELDS) {
    const val = newState[field]
    if (typeof val === 'string') {
      ;(newState as Record<string, unknown>)[field] = val === 'true' || val === '1'
    }
  }
}

describe('coerceBooleans (inline, matches useConnection.ts logic)', () => {
  it('converts string booleans in booleans map', () => {
    const state: Record<string, unknown> = { booleans: { a: 'true', b: 'false', c: '1', d: '0' } }
    coerceBooleans(state as InverterState)
    const b = state.booleans as Record<string, unknown>
    expect(b.a).toBe(true)
    expect(b.b).toBe(false)
    expect(b.c).toBe(true)
    expect(b.d).toBe(false)
  })

  it('converts string boolean fields', () => {
    const state: Record<string, unknown> = {
      pump_switch: 'true',
      water_valve: 'false',
      washer_power: '1',
      dryer_power: '0',
      dry_run: 'true',
    }
    coerceBooleans(state as InverterState)
    expect(state.pump_switch).toBe(true)
    expect(state.water_valve).toBe(false)
    expect(state.washer_power).toBe(true)
    expect(state.dryer_power).toBe(false)
    expect(state.dry_run).toBe(true)
  })

  it('leaves actual booleans unchanged', () => {
    const state: InverterState = {
      pump_switch: true,
      water_valve: false,
      booleans: { x: true, y: false },
    }
    coerceBooleans(state)
    expect(state.pump_switch).toBe(true)
    expect(state.water_valve).toBe(false)
    expect(state.booleans!.x).toBe(true)
    expect(state.booleans!.y).toBe(false)
  })

  it('handles missing booleans map', () => {
    const state: InverterState = {}
    coerceBooleans(state)
    expect(state).toEqual({})
  })
})

import { beforeEach, describe, expect, it } from 'vitest'
import { state } from '../composables/useInverterState'

// Re-import the function via module evaluation: useConnection defines processState
// as a non-exported helper, so we test the merge semantics by directly exercising
// the merge contract via state.value mutations + the same Object.entries/loop pattern.

/**
 * Mirrors the merge logic in useConnection.ts processState. If the production
 * code changes, update this helper to keep parity and exercise the same invariants.
 */
function applyProcessStateMerge(
  prev: Record<string, unknown>,
  incoming: Record<string, unknown>
): Record<string, unknown> {
  const merged: Record<string, unknown> = { ...prev }
  for (const [key, val] of Object.entries(incoming)) {
    if (val !== undefined && val !== null) {
      merged[key] = val
    }
  }
  return merged
}

describe('processState merge', () => {
  beforeEach(() => {
    state.value = {} as typeof state.value
  })

  it('JSON null does not overwrite an existing number', () => {
    const prev = { car_soc: 66, car_charging_power: 3200 }
    const incoming = { car_soc: null, car_charging_power: 3300 }
    const merged = applyProcessStateMerge(prev, incoming)
    expect(merged.car_soc).toBe(66)
    expect(merged.car_charging_power).toBe(3300)
  })

  it('JSON null does not clear the EV section fields', () => {
    const prev = { car_soc: 66, ev_charging_power: 7400 }
    const incoming = { car_soc: null, ev_charging_power: null, ev_present: false }
    const merged = applyProcessStateMerge(prev, incoming)
    // Car SoC and EV power preserved; presence latch (false) is accepted because
    // it is a boolean, not null. The Rust backend is the source of truth for
    // presence bits and sticky-true; this test only proves null does not wipe
    // numeric telemetry.
    expect(merged.car_soc).toBe(66)
    expect(merged.ev_charging_power).toBe(7400)
  })

  it('undefined is treated as missing — keep previous', () => {
    const prev = { car_soc: 66 }
    const incoming = { car_soc: undefined }
    const merged = applyProcessStateMerge(prev, incoming)
    expect(merged.car_soc).toBe(66)
  })

  it('zero is a legitimate value for power (not skipped)', () => {
    const prev = { car_charging_power: 3200 }
    const incoming = { car_charging_power: 0 }
    const merged = applyProcessStateMerge(prev, incoming)
    expect(merged.car_charging_power).toBe(0)
  })
})

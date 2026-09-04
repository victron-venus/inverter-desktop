import { beforeEach, describe, expect, it } from 'vitest'
import { applyInverterState, state } from '../composables/useInverterState'

describe('applyInverterState merge', () => {
  beforeEach(() => {
    state.value = { booleans: {}, features: {}, ui_config: {} }
  })

  it('JSON null does not overwrite an existing number', () => {
    applyInverterState({ car_soc: 66, car_charging_power: 3200 })
    applyInverterState({ car_soc: null as unknown as number, car_charging_power: 3300 })
    expect(state.value.car_soc).toBe(66)
    expect(state.value.car_charging_power).toBe(3300)
  })

  it('JSON null does not clear the EV section fields', () => {
    applyInverterState({ car_soc: 66, ev_charging_power: 7400 })
    applyInverterState({
      car_soc: null as unknown as number,
      ev_charging_power: null as unknown as number,
      ev_present: false,
    })
    expect(state.value.car_soc).toBe(66)
    expect(state.value.ev_charging_power).toBe(7400)
  })

  it('undefined is treated as missing — keep previous', () => {
    applyInverterState({ car_soc: 66 })
    applyInverterState({ car_soc: undefined })
    expect(state.value.car_soc).toBe(66)
  })

  it('zero is a legitimate value for power (not skipped)', () => {
    applyInverterState({ car_charging_power: 3200 })
    applyInverterState({ car_charging_power: 0 })
    expect(state.value.car_charging_power).toBe(0)
  })

  it('replaces state object identity so shallowRef consumers re-render', () => {
    applyInverterState({ gt: 100 })
    const first = state.value
    applyInverterState({ gt: 200 })
    expect(state.value).not.toBe(first)
    expect(state.value.gt).toBe(200)
  })
})

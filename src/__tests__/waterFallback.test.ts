import { beforeEach, describe, expect, it } from 'vitest'
import { useHA } from '../composables/useHA'
import { appConfig, state } from '../composables/useInverterState'

type Cfg = NonNullable<typeof appConfig.value>
const setCfg = (partial: Record<string, unknown>) => {
  appConfig.value = partial as Cfg
}

describe('water section Cerbo-MQTT-first fallback chain', () => {
  beforeEach(() => {
    state.value = { ...state.value, water_level: null, water_valve: null, pump_switch: null }
    setCfg({})
  })

  it('prefers MQTT values when present, even without HA configured', () => {
    const ha = useHA()
    state.value.water_level = 66
    state.value.water_valve = true
    state.value.pump_switch = false

    expect(ha.waterLevel.value).toBe(66)
    expect(ha.waterValveState.value).toBe(true)
    expect(ha.pumpSwitchState.value).toBe(false)
    expect(ha.waterSectionVisible.value).toBe(true)
  })

  it('falls back to HA entities when MQTT has no data', () => {
    setCfg({
      ha_use_direct_api: true,
      ha_url: 'http://ha:8123',
      ha_longlived_token: 'tok',
      ha_water_level_entity: 'sensor.level',
      ha_valve_switch_entity: 'switch.valve',
      ha_pump_switch_entity: 'switch.pump',
    })
    // simulate Rust-side filtered HA entity states via the public computed path
    const ha = useHA()
    ha.haEntityStates.value['sensor.level'] = '42'
    ha.haEntityStates.value['switch.valve'] = 'on'
    ha.haEntityStates.value['switch.pump'] = 'off'

    expect(ha.waterLevel.value).toBe(42)
    expect(ha.waterValveState.value).toBe(true)
    expect(ha.pumpSwitchState.value).toBe(false)
    expect(ha.waterSectionVisible.value).toBe(true)
  })

  it('hides water section with neither MQTT nor HA', () => {
    const ha = useHA()
    expect(ha.waterSectionVisible.value).toBe(false)
  })
})

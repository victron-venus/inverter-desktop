import { beforeEach, describe, expect, it } from 'vitest'
import { appConfig, state } from '../composables/useInverterState'
import { useMQTTState } from '../composables/useMQTTState'

type Cfg = NonNullable<typeof appConfig.value>
const setCfg = (partial: Record<string, unknown>) => {
  appConfig.value = partial as Cfg
}

describe('water section (Cerbo MQTT only - dbus-pump data)', () => {
  beforeEach(() => {
    state.value = {
      ...state.value,
      water_level: null,
      water_valve: null,
      pump_switch: null,
      discovered_water_ev: null,
    }
    setCfg({})
    const ha = useMQTTState()
    ha.waterLatch.value = false
  })

  it('shows values straight from MQTT state', () => {
    state.value = {
      ...state.value,
      water_level: 66,
      water_valve: true,
      pump_switch: false,
    }

    const ha = useMQTTState()
    expect(ha.waterLevel.value).toBe(66)
    expect(ha.waterValveState.value).toBe(true)
    expect(ha.pumpSwitchState.value).toBe(false)
    expect(ha.waterSectionVisible.value).toBe(true)
  })

  it('stays hidden without MQTT data even when HA is configured', () => {
    setCfg({
      ha_use_direct_api: true,
      ha_url: 'http://ha:8123',
      ha_longlived_token: 'tok',
    })
    const ha = useMQTTState()
    expect(ha.waterSectionVisible.value).toBe(false)
    expect(ha.waterLevel.value).toBeNull()
    expect(ha.waterValveState.value).toBeNull()
    expect(ha.pumpSwitchState.value).toBeNull()
  })

  it('becomes visible from a partial payload (valve only)', () => {
    state.value = { ...state.value, water_valve: true }

    const ha = useMQTTState()
    expect(ha.waterSectionVisible.value).toBe(true)
    expect(ha.pumpSwitchState.value).toBeNull()
  })

  it('becomes visible when Cerbo discovers tank instances (Config parity)', () => {
    state.value = {
      ...state.value,
      discovered_water_ev: [
        { instance: 21, kind: 'tank', name: 'Fresh water' },
        { instance: 1, kind: 'pump', name: 'Pressure pump' },
      ],
    }

    const ha = useMQTTState()
    expect(ha.waterSectionVisible.value).toBe(true)
    expect(ha.waterLevel.value).toBeNull()
  })
})

describe('water manual override (dbus-pump /Mode)', () => {
  beforeEach(() => {
    state.value = {
      ...state.value,
      water_level: 50,
      water_valve: false,
      pump_switch: false,
      water_pump_mode: null,
      water_valve_mode: null,
      discovered_water_ev: null,
    }
    setCfg({})
    const ha = useMQTTState()
    ha.waterLatch.value = false
  })

  it('exposes mode values from MQTT state', () => {
    state.value = { ...state.value, water_pump_mode: 1, water_valve_mode: 0 }
    const ha = useMQTTState()
    expect(ha.waterPumpMode.value).toBe(1)
    expect(ha.waterValveMode.value).toBe(0)
  })
})

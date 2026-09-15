import { beforeEach, describe, expect, it } from 'vitest'
import {
  applyInverterState,
  mqttConnected,
  refreshTelemetryQuality,
  resetInverterState,
  state,
  telemetry,
  TELEMETRY_STALE_AFTER_MS,
} from '../composables/useInverterState'

beforeEach(() => {
  resetInverterState()
  mqttConnected.value = true
})
describe('telemetry observation quality', () => {
  it('records transport source and distinguishes zero from an absent observation', () => {
    applyInverterState({ car_soc: 60, gt: 1 }, { source: 'mqtt', observedAt: 1000 })
    applyInverterState({ car_soc: null, gt: 0 }, { source: 'igw', observedAt: 2000 })
    expect(state.value.car_soc).toBe(60)
    expect(state.value.gt).toBe(0)
    expect(telemetry.value.fields.car_soc).toEqual({ source: 'mqtt', observed_at: 1000 })
    expect(telemetry.value.fields.gt).toEqual({ source: 'igw', observed_at: 2000 })
  })
  it('marks retained missing fields stale even while other telemetry keeps arriving', () => {
    applyInverterState({ car_soc: 60, gt: 1 }, { observedAt: 1000 })
    applyInverterState({ gt: 2 }, { observedAt: 1000 + TELEMETRY_STALE_AFTER_MS + 1 })
    expect(telemetry.value.quality).toBe('stale')
    expect(telemetry.value.stale_fields).toContain('car_soc')
  })
  it('does not make cached snapshots fresh when reopening the window', () => {
    applyInverterState({ gt: 1 }, { observedAt: 1000 })
    applyInverterState({ gt: 1 }, { snapshot: true, observedAt: 60_000 })
    refreshTelemetryQuality(60_000)
    expect(telemetry.value.observed_at).toBe(1000)
    expect(telemetry.value.quality).toBe('stale')
  })
  it('marks transport loss stale immediately and requires a new live observation after reset', () => {
    applyInverterState({ gt: 1 }, { observedAt: 1000 })
    mqttConnected.value = false
    refreshTelemetryQuality(1001)
    expect(telemetry.value.quality).toBe('stale')
    resetInverterState()
    applyInverterState({ gt: 3 }, { snapshot: true })
    expect(telemetry.value.quality).toBe('unknown')
  })
})

describe('authoritative IGW fields', () => {
  it('clears expired controller and disconnected device values while retaining independent overlays', () => {
    applyInverterState(
      {
        booleans: { no_feed: true },
        dry_run: true,
        ess_mode: { is_external: true },
        pump_switch: true,
        water_valve: false,
        water_pump_mode: 1,
        car_soc: 60,
        car_charging_power: 1500,
        ev_charging_power: 1600,
        ha_direct_connected: true,
        grid_backup: {
          enabled: true,
          available: true,
          service: 'test',
          name: 'Test',
          power: 100,
          device_instance: 2,
          measurement_time: 1000,
          age_seconds: 1,
        },
        grid_backup_observed_at: 1000,
      },
      { source: 'igw', observedAt: 1000 }
    )
    applyInverterState(
      {
        gateway_snapshot: true,
        booleans: {},
        grid_backup: null,
        grid_using_backup: false,
        pump_switch: null,
        water_valve: null,
        car_soc: null,
        car_charging_power: null,
        ev_charging_power: null,
        ev_present: false,
        evcharger_present: false,
      },
      { source: 'igw', observedAt: 2000 }
    )
    expect(state.value.booleans).toEqual({})
    for (const field of [
      'dry_run',
      'ess_mode',
      'pump_switch',
      'water_pump_mode',
      'car_soc',
      'car_charging_power',
      'ev_charging_power',
      'grid_backup_observed_at',
    ]) {
      expect((state.value as Record<string, unknown>)[field]).toBeUndefined()
      expect(telemetry.value.fields[field]).toBeUndefined()
    }
    expect(state.value.grid_backup).toBeUndefined()
    expect(state.value.grid_using_backup).toBe(false)
    expect(state.value.ha_direct_connected).toBe(true)
  })

  it('preserves null holding for ordinary MQTT and non-authoritative snapshots', () => {
    applyInverterState({ pump_switch: true, car_soc: 60 })
    applyInverterState({ pump_switch: null, car_soc: null }, { source: 'mqtt' })
    expect(state.value.pump_switch).toBe(true)
    expect(state.value.car_soc).toBe(60)
  })

  it('accepts authoritative zero and false values including cached IGW snapshots', () => {
    applyInverterState({ car_soc: 60, pump_switch: true })
    applyInverterState(
      { gateway_snapshot: true, car_soc: 0, pump_switch: false },
      { snapshot: true }
    )
    expect(state.value.car_soc).toBe(0)
    expect(state.value.pump_switch).toBe(false)
  })
})

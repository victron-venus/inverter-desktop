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

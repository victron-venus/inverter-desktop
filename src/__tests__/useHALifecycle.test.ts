import { flushPromises } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defaultConfig } from '../config'
import { useHA } from '../composables/useHA'
import { appConfig, resetInverterState } from '../composables/useInverterState'
import type { HaFilteredData } from '../types/ha'

const boundary = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: boundary.listen }))

type Callback = (event: { payload: unknown }) => void
let events: Map<string, Set<Callback>>
let ha: ReturnType<typeof useHA>
const snapshot = (temperature = '20'): HaFilteredData => ({
  sensors: [
    { entity_id: 'sensor.temperature', name: 'Temperature', state: temperature, unit: '°C' },
  ],
  numbers: [
    { entity_id: 'number.limit', name: 'Limit', value: 10, min: 0, max: 100, step: 1, unit: '%' },
  ],
  covers: [{ entity_id: 'cover.blind', name: 'Blind', position: 50, state: 'open' }],
  media_players: [{ entity_id: 'media_player.tv', name: 'TV', state: 'playing' }],
  scenes: [{ entity_id: 'scene.evening', name: 'Evening' }],
  weather: {
    entity_id: 'weather.home',
    name: 'Home',
    state: 'sunny',
    temperature: 20,
    unit: '°C',
    forecast: [],
  },
  refresh_sensors: true,
})
const configured = () => ({
  ...defaultConfig,
  ha_use_direct_api: true,
  ha_url: 'http://ha.invalid',
  ha_longlived_token: 'test-token',
  ha_entities: [
    { id: 'lamp', label: 'Lamp', entity: 'switch.lamp', domain: 'switch', enabled: true },
  ],
})
function emit(name: string, payload: unknown) {
  for (const callback of events.get(name) ?? []) callback({ payload })
}
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}
function defaults(name: string) {
  if (name === 'get_ha_filtered_data') return snapshot()
  if (name === 'get_ha_connection_status') return true
  if (name === 'get_ha_appliance_states') return [{ entity_id: 'switch.lamp', state: 'on' }]
  if (name === 'get_ha_entity_states') return []
  if (name === 'get_state') return {}
  return undefined
}
beforeEach(() => {
  vi.useFakeTimers()
  resetInverterState()
  appConfig.value = configured()
  events = new Map()
  boundary.invoke.mockReset().mockImplementation(async (name: string) => defaults(name))
  boundary.listen.mockReset().mockImplementation(async (name: string, callback: Callback) => {
    const callbacks = events.get(name) ?? new Set<Callback>()
    events.set(name, callbacks)
    callbacks.add(callback)
    return () => callbacks.delete(callback)
  })
  ha = useHA()
})
afterEach(() => {
  ha.cleanupHa()
  vi.useRealTimers()
})

describe('HA event and snapshot lifecycle', () => {
  it('bootstraps cached display and actual WS status when startup events were missed', async () => {
    await ha.initHa()
    expect(ha.haSensors.value[0].state).toBe('20')
    expect(ha.haNumbers.value).toHaveLength(1)
    expect(ha.haConnected.value).toBe(true)
    expect(ha.buttonStates.value.lamp).toBe('on')
    expect(boundary.invoke).toHaveBeenCalledWith('get_ha_connection_status')
    expect(boundary.invoke).not.toHaveBeenCalledWith('test_ha_connection', expect.anything())
    expect([...events.keys()]).toContain('ha-filtered-update')
  })

  it('uses coalesced live sensor updates without allowing an older snapshot to overwrite them', async () => {
    const pending = deferred<HaFilteredData>()
    boundary.invoke.mockImplementation(async (name: string) =>
      name === 'get_ha_filtered_data' ? pending.promise : defaults(name)
    )
    const init = ha.initHa()
    await flushPromises()
    emit('ha-filtered-update', snapshot('30'))
    pending.resolve(snapshot('20'))
    await init
    expect(ha.haSensors.value[0].state).toBe('30')
    emit('ha-filtered-update', snapshot('35'))
    expect(ha.haSensors.value[0].state).toBe('35')
  })

  it('retains a newer entity event when an older HTTP request completes', async () => {
    const pending = deferred<Array<{ entity_id: string; state: string }>>()
    boundary.invoke.mockImplementation(async (name: string) =>
      name === 'get_ha_appliance_states' ? pending.promise : defaults(name)
    )
    const init = ha.initHa()
    await flushPromises()
    emit('ha-state-update', { entity_id: 'switch.lamp', state: 'off' })
    pending.resolve([{ entity_id: 'switch.lamp', state: 'on' }])
    await init
    expect(ha.buttonStates.value.lamp).toBe('off')
  })

  it('marks disconnected immediately and clears every display group after the grace period', async () => {
    await ha.initHa()
    emit('ha-connection-status', false)
    expect(ha.haConnected.value).toBe(false)
    expect(ha.haSensors.value).toHaveLength(1)
    await vi.advanceTimersByTimeAsync(15_001)
    expect(ha.haEntityStates.value).toEqual({})
    for (const group of [ha.haSensors, ha.haNumbers, ha.haCovers, ha.haMediaPlayers, ha.haScenes]) {
      expect(group.value).toEqual([])
    }
    expect(ha.haWeather.value).toBeNull()
  })

  it('reconnects with a fresh snapshot and cancels the old expiry timer', async () => {
    await ha.initHa()
    emit('ha-connection-status', false)
    await vi.advanceTimersByTimeAsync(10_000)
    boundary.invoke.mockImplementation(async (name: string) =>
      name === 'get_ha_filtered_data' ? snapshot('28') : defaults(name)
    )
    emit('ha-connection-status', true)
    await flushPromises()
    await vi.advanceTimersByTimeAsync(10_000)
    expect(ha.haConnected.value).toBe(true)
    expect(ha.haSensors.value[0].state).toBe('28')
  })

  it('disabling HA clears state and rejects late HTTP results and events', async () => {
    const pending = deferred<HaFilteredData>()
    boundary.invoke.mockImplementation(async (name: string) =>
      name === 'get_ha_filtered_data' ? pending.promise : defaults(name)
    )
    const init = ha.initHa()
    await flushPromises()
    appConfig.value = { ...configured(), ha_use_direct_api: false }
    pending.resolve(snapshot())
    await init
    emit('ha-filtered-update', snapshot('99'))
    expect(ha.haSensors.value).toEqual([])
    expect(ha.haWsConnected.value).toBe(false)
  })

  it('reinitialization and cleanup leave no listeners, watchers or in-flight snapshot mutations', async () => {
    await ha.initHa()
    await ha.initHa()
    for (const callbacks of events.values()) expect(callbacks.size).toBe(1)
    const pending = deferred<HaFilteredData>()
    boundary.invoke.mockImplementation(async (name: string) =>
      name === 'get_ha_filtered_data' ? pending.promise : defaults(name)
    )
    emit('ha-connection-status', true)
    await flushPromises()
    ha.cleanupHa()
    pending.resolve(snapshot('99'))
    await flushPromises()
    expect(ha.haSensors.value[0].state).toBe('20')
    for (const callbacks of events.values()) expect(callbacks.size).toBe(0)
    boundary.invoke.mockClear()
    appConfig.value = { ...configured(), ha_url: 'http://another.invalid' }
    await flushPromises()
    expect(boundary.invoke).not.toHaveBeenCalled()
  })
})

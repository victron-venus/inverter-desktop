import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defaultConfig } from '../config'
import { useConnection } from './useConnection'
import { dataSource, mqttConnected, state } from './useInverterState'

const boundary = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), getConfig: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: boundary.listen }))
vi.mock('../config', async (original) => ({
  ...(await original<object>()),
  getAppConfig: boundary.getConfig,
}))
vi.mock('./useSystemNotifications', () => ({ notify: vi.fn() }))
vi.mock('@tauri-apps/plugin-notification', () => ({
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
}))

type Callback = (event: { payload: unknown }) => void
let events: Map<string, Set<Callback>>
let connection: ReturnType<typeof useConnection>
const configured = () => ({
  ...defaultConfig,
  mqtt_host: 'cerbo',
  gateway_enabled: false,
  camera_enabled: false,
})
const disabled = () => ({ ...configured(), mqtt_host: '' })
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

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('localStorage', { setItem: vi.fn() })
  boundary.invoke
    .mockReset()
    .mockImplementation(async (name: string) => (name === 'get_state' ? { gt: 123 } : undefined))
  boundary.getConfig.mockReset().mockResolvedValue(configured())
  events = new Map()
  boundary.listen.mockReset().mockImplementation(async (name: string, callback: Callback) => {
    if (!events.has(name)) events.set(name, new Set())
    events.get(name)!.add(callback)
    return () => events.get(name)!.delete(callback)
  })
  state.value = { booleans: {}, features: {}, ui_config: {} }
  mqttConnected.value = false
  connection = useConnection()
})
afterEach(() => {
  connection.cleanup()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

async function connectAndDisable() {
  await connection.connectMqtt()
  emit('mqtt-connection-status', true)
  expect(mqttConnected.value).toBe(true)
  const oldStateCallbacks = [...(events.get('mqtt-state-update') ?? [])]
  boundary.getConfig.mockResolvedValue(disabled())
  await connection.connectMqtt()
  return oldStateCallbacks
}

describe('inverter transport configuration lifecycle', () => {
  it('disconnects both inverter transports, clears telemetry and rejects late events', async () => {
    await connectAndDisable()
    expect(boundary.invoke).toHaveBeenCalledWith('disconnect_inverter', undefined)
    expect(mqttConnected.value).toBe(false)
    expect(dataSource.value).toBe('mqtt')
    expect(state.value.gt).toBeUndefined()
    emit('mqtt-state-update', { gt: 999 })
    emit('mqtt-connection-status', true)
    emit('mqtt-connection-status', false)
    emit('window-focused', undefined)
    await vi.advanceTimersByTimeAsync(120000)
    expect(state.value.gt).toBeUndefined()
    expect(mqttConnected.value).toBe(false)
    expect(boundary.invoke.mock.calls.filter(([name]) => name === 'connect_mqtt')).toHaveLength(1)
  })

  it('cancels an already scheduled reconnect when configuration is removed', async () => {
    await connection.connectMqtt()
    emit('window-focused', undefined)
    boundary.getConfig.mockResolvedValue(disabled())
    await connection.connectMqtt()
    await vi.advanceTimersByTimeAsync(120000)
    expect(boundary.invoke.mock.calls.filter(([name]) => name === 'connect_mqtt')).toHaveLength(1)
  })

  it('ignores an older configuration load that resolves after disabling', async () => {
    const old = deferred<ReturnType<typeof configured>>()
    boundary.getConfig.mockReturnValueOnce(old.promise)
    const pending = connection.connectMqtt()
    boundary.getConfig.mockResolvedValue(disabled())
    await connection.connectMqtt()
    old.resolve(configured())
    await pending
    expect(boundary.invoke).not.toHaveBeenCalledWith('connect_mqtt', expect.anything())
    expect(mqttConnected.value).toBe(false)
  })

  it('ignores a pending MQTT reachability result after disabling', async () => {
    const probe = deferred<void>()
    boundary.getConfig.mockResolvedValue({
      ...configured(),
      gateway_enabled: true,
      gateway_url: 'https://gateway.example',
      gateway_access_client_id: 'id',
      gateway_access_client_secret: 'test',
    })
    boundary.invoke.mockImplementation(async (name: string) =>
      name === 'test_mqtt_connection' ? probe.promise : undefined
    )
    const pending = connection.connectMqtt()
    await vi.waitFor(() =>
      expect(boundary.invoke).toHaveBeenCalledWith('test_mqtt_connection', expect.anything())
    )
    boundary.getConfig.mockResolvedValue(disabled())
    await connection.connectMqtt()
    probe.resolve()
    await pending
    expect(boundary.invoke).not.toHaveBeenCalledWith('connect_mqtt', expect.anything())
    expect(boundary.invoke).not.toHaveBeenCalledWith('connect_gateway', expect.anything())
  })

  it('finishes an already dispatched startup before dispatching disconnect', async () => {
    const startup = deferred<void>()
    boundary.invoke.mockImplementation(async (name: string) =>
      name === 'connect_mqtt' ? startup.promise : undefined
    )
    const pendingStart = connection.connectMqtt()
    await vi.waitFor(() =>
      expect(boundary.invoke).toHaveBeenCalledWith('connect_mqtt', expect.anything())
    )
    boundary.getConfig.mockResolvedValue(disabled())
    const pendingStop = connection.connectMqtt()
    await vi.advanceTimersByTimeAsync(0)
    expect(boundary.invoke).not.toHaveBeenCalledWith('disconnect_inverter', undefined)
    startup.resolve()
    await Promise.all([pendingStart, pendingStop])
    expect(boundary.invoke).toHaveBeenCalledWith('disconnect_inverter', undefined)
    expect(mqttConnected.value).toBe(false)
    expect(state.value.gt).toBeUndefined()
  })

  it('permits a new configured session but rejects callbacks queued by its predecessor', async () => {
    const oldStateCallbacks = await connectAndDisable()
    boundary.getConfig.mockResolvedValue(configured())
    await connection.connectMqtt()
    emit('mqtt-connection-status', true)
    emit('mqtt-state-update', { gt: 456 })
    expect(mqttConnected.value).toBe(true)
    for (const callback of oldStateCallbacks) callback({ payload: { gt: 999 } })
    expect(state.value.gt).toBe(456)
  })
})

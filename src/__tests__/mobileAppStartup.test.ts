import { flushPromises, shallowMount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from '../App.vue'
import { defaultConfig, type AppConfig } from '../config'
import { appConfig, mqttConnected, resetInverterState } from '../composables/useInverterState'

const boundary = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  getConfig: vi.fn(),
  permission: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: boundary.listen }))
vi.mock('../config', async (original) => ({
  ...(await original<object>()),
  getAppConfig: boundary.getConfig,
}))
vi.mock('../composables/useChart', () => ({
  useChart: () => ({ chartOption: {}, forceUpdateChart: vi.fn(), setChartPaused: vi.fn() }),
  addHistoryPoint: vi.fn(),
}))
vi.mock('../composables/useSystemNotifications', () => ({
  notify: vi.fn(),
  initSystemNotifications: vi.fn(),
}))
vi.mock('@tauri-apps/plugin-notification', () => ({
  isPermissionGranted: boundary.permission,
  requestPermission: vi.fn(),
}))

type Callback = (event: { payload: unknown }) => void
let events: Map<string, Set<Callback>>
let wrapper: VueWrapper | undefined

function configured(host: string): AppConfig {
  return {
    ...defaultConfig,
    mqtt_host: host,
    gateway_enabled: false,
    camera_enabled: false,
    setup_completed: true,
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function register(name: string, callback: Callback) {
  const callbacks = events.get(name) ?? new Set<Callback>()
  events.set(name, callbacks)
  callbacks.add(callback)
  return () => callbacks.delete(callback)
}

function emitSaved() {
  for (const callback of events.get('config-saved') ?? []) {
    callback({ payload: { color_scheme: 'light' } })
  }
}

function mountApp() {
  wrapper = shallowMount(App, { global: { renderStubDefaultSlot: true } })
  return wrapper
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('localStorage', { getItem: vi.fn(() => null), setItem: vi.fn() })
  events = new Map()
  boundary.listen.mockReset().mockImplementation(async (name: string, callback: Callback) => {
    return register(name, callback)
  })
  boundary.permission.mockReset().mockResolvedValue(true)
  boundary.getConfig.mockReset().mockResolvedValue(configured('old-cerbo'))
  boundary.invoke.mockReset().mockImplementation(async (name: string) => {
    if (name === 'get_release_info') return { version: 'test' }
    if (name === 'get_state') return {}
  })
  resetInverterState()
  mqttConnected.value = false
  appConfig.value = configured('old-cerbo')
})

afterEach(async () => {
  wrapper?.unmount()
  wrapper = undefined
  await flushPromises()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('mobile dashboard startup configuration lifecycle', () => {
  it('applies a save during the initial MQTT probe and ignores that older probe result', async () => {
    const probe = deferred<void>()
    const oldConfig = {
      ...configured('old-cerbo'),
      gateway_enabled: true,
      gateway_url: 'https://old-gateway.example',
      gateway_access_client_id: 'old-id',
      gateway_access_client_secret: 'old-secret',
    }
    appConfig.value = oldConfig
    boundary.getConfig.mockResolvedValue(oldConfig)
    boundary.invoke.mockImplementation(async (name: string) => {
      if (name === 'get_release_info') return { version: 'test' }
      if (name === 'get_state') return {}
      if (name === 'test_mqtt_connection') return probe.promise
    })
    mountApp()
    await flushPromises()
    expect(boundary.invoke).toHaveBeenCalledWith(
      'test_mqtt_connection',
      expect.objectContaining({ host: 'old-cerbo' })
    )

    boundary.getConfig.mockResolvedValue(configured('new-cerbo'))
    emitSaved()
    await flushPromises()
    expect(boundary.invoke).toHaveBeenCalledWith(
      'connect_mqtt',
      expect.objectContaining({ host: 'new-cerbo' })
    )

    probe.resolve()
    await flushPromises()
    const connections = boundary.invoke.mock.calls.filter(([name]) => name === 'connect_mqtt')
    expect(connections).toHaveLength(1)
    expect(boundary.invoke).not.toHaveBeenCalledWith('connect_gateway', expect.anything())
    expect(appConfig.value).toMatchObject({ mqtt_host: 'new-cerbo' })
  })

  it('does not restore an older startup configuration or setup wizard after a save', async () => {
    const oldRead = deferred<AppConfig>()
    appConfig.value = null
    boundary.getConfig.mockReturnValueOnce(oldRead.promise)
    const app = mountApp()
    await flushPromises()
    expect(boundary.getConfig).toHaveBeenCalledTimes(1)

    boundary.getConfig.mockResolvedValue(configured('new-cerbo'))
    emitSaved()
    await flushPromises()
    expect(appConfig.value).toMatchObject({ mqtt_host: 'new-cerbo' })

    oldRead.resolve({ ...configured(''), setup_completed: false })
    await flushPromises()
    expect(appConfig.value).toMatchObject({ mqtt_host: 'new-cerbo' })
    expect(app.find('setup-wizard-stub').exists()).toBe(false)
    expect(boundary.getConfig).toHaveBeenCalledTimes(2)
    expect(boundary.invoke.mock.calls.filter(([name]) => name === 'connect_mqtt')).toHaveLength(1)
  })

  it('releases a subscription that resolves after unmount without restarting the connection', async () => {
    const subscription = deferred<() => void>()
    const unlisten = vi.fn()
    let callback!: Callback
    boundary.listen.mockImplementationOnce((_name: string, handler: Callback) => {
      callback = handler
      return subscription.promise
    })
    mountApp().unmount()
    wrapper = undefined
    callback({ payload: {} })
    subscription.resolve(unlisten)
    await flushPromises()

    expect(unlisten).toHaveBeenCalledTimes(1)
    expect(boundary.permission).not.toHaveBeenCalled()
    expect(boundary.getConfig).not.toHaveBeenCalled()
    expect(boundary.invoke).not.toHaveBeenCalledWith('connect_mqtt', expect.anything())
    expect(boundary.listen).toHaveBeenCalledTimes(1)
  })
})

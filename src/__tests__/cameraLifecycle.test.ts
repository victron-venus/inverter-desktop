import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createCameraConnection } from '../features/desktop/cameraConnection'
import { appConfig, haMqttConnected } from '../composables/useInverterState'
import type { AppConfig } from '../config'

const { invoke, getAppConfig, listeners } = vi.hoisted(() => ({
  invoke: vi.fn(),
  getAppConfig: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(name, handler)
    return () => listeners.delete(name)
  }),
}))
vi.mock('../config', () => ({ getAppConfig }))
const enabled: AppConfig = {
  mqtt_host: 'Cerbo',
  mqtt_port: 1883,
  camera_enabled: true,
  mqtt_ha_host: 'Cameras',
  mqtt_ha_port: 1883,
}
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}
async function flush() {
  for (let index = 0; index < 20; index++) await Promise.resolve()
}
const commands = () =>
  invoke.mock.calls.map(([command]) => command).filter((command) => command !== 'send_notification')
beforeEach(() => {
  vi.useFakeTimers()
  invoke.mockReset().mockResolvedValue(undefined)
  getAppConfig.mockReset().mockResolvedValue({ ...enabled })
  listeners.clear()
  appConfig.value = { ...enabled }
})
afterEach(() => vi.useRealTimers())

describe('desktop camera lifecycle ownership', () => {
  it('does not reconnect from an old config read after camera monitoring is disabled', async () => {
    const camera = createCameraConnection()
    await camera.connect(enabled)
    const oldConfig = deferred<AppConfig>()
    getAppConfig.mockReturnValueOnce(oldConfig.promise)
    listeners.get('ha-mqtt-connection-status')?.({ payload: false })
    vi.advanceTimersByTime(12_000)
    await flush()
    expect(getAppConfig).toHaveBeenCalledOnce()
    const disabled = { ...enabled, camera_enabled: false }
    appConfig.value = disabled
    await camera.syncHaMqttFromConfig(disabled)
    oldConfig.resolve({ ...enabled })
    await flush()
    expect(commands()).toEqual(['connect_ha_mqtt', 'disconnect_ha_mqtt'])
    expect(haMqttConnected.value).toBeNull()
    camera.cleanup()
    await flush()
  })

  it('stops the native camera broker after an already-started connect completes during cleanup', async () => {
    const camera = createCameraConnection()
    const pending = deferred<void>()
    invoke.mockImplementation(async (command: string) =>
      command === 'connect_ha_mqtt' ? pending.promise : undefined
    )
    const connecting = camera.connect(enabled)
    await flush()
    expect(commands()).toEqual(['connect_ha_mqtt'])
    camera.cleanup()
    pending.resolve()
    await connecting
    await flush()
    expect(commands()).toEqual(['connect_ha_mqtt', 'disconnect_ha_mqtt'])
    expect(listeners.size).toBe(0)
    expect(haMqttConnected.value).toBeNull()
    expect(vi.getTimerCount()).toBe(0)
  })
  it('cancels a toggle when cleanup happens during its config read', async () => {
    const camera = createCameraConnection()
    const read = deferred<AppConfig>()
    getAppConfig.mockReturnValueOnce(read.promise)
    appConfig.value = { ...enabled, camera_enabled: false }
    const toggling = camera.toggleCameraMotion()
    camera.cleanup()
    read.resolve({ ...enabled, camera_enabled: false })
    await toggling
    await flush()
    expect(commands()).toEqual(['disconnect_ha_mqtt'])
    expect(appConfig.value.camera_enabled).toBe(false)
    expect(haMqttConnected.value).toBeNull()
  })

  it('does not restart the camera after a toggle save completes following cleanup', async () => {
    const camera = createCameraConnection()
    const saving = deferred<void>()
    getAppConfig.mockResolvedValue({ ...enabled, camera_enabled: false })
    invoke.mockImplementation(async (command: string) =>
      command === 'save_config' ? saving.promise : undefined
    )
    appConfig.value = { ...enabled, camera_enabled: false }
    const toggling = camera.toggleCameraMotion()
    await flush()
    expect(commands()).toEqual(['save_config'])
    camera.cleanup()
    saving.resolve()
    await toggling
    await flush()
    expect(commands()).toEqual(['save_config', 'disconnect_ha_mqtt'])
    expect(appConfig.value.camera_enabled).toBe(false)
    expect(haMqttConnected.value).toBeNull()
  })
})

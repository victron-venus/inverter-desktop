import { flushPromises } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { DesktopPluginConfig } from '../../../config'
import type { PluginSnapshot } from './types'
import { createPluginDashboard } from './usePluginDashboard'

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }))

const frigate: DesktopPluginConfig = {
  plugin_id: 'inverter-desktop.frigate',
  version: '1.0.0',
  artifacts: {},
}
const kerberos: DesktopPluginConfig = {
  ...frigate,
  plugin_id: 'inverter-desktop.kerberos',
}
const worker: PluginSnapshot = {
  plugin_id: frigate.plugin_id,
  instance_id: 'running-worker',
  state: 'running',
  generation: 1,
  restart_count: 0,
  last_error: null,
  contributions: [],
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, resolve, reject }
}

const callbacks = new Map<string, (event: { payload: unknown }) => void>()
const dashboards: Array<ReturnType<typeof createPluginDashboard>> = []
let unlocked: boolean
let configuration: unknown
let readConfiguration: () => Promise<unknown>

function emit(name: string, payload?: unknown) {
  const callback = callbacks.get(name)
  if (!callback) throw new Error(`Missing listener: ${name}`)
  callback({ payload })
}

function dashboard() {
  const value = createPluginDashboard()
  dashboards.push(value)
  return value
}

function configurationReads() {
  return native.invoke.mock.calls.filter(([command]) => command === 'get_config').length
}

beforeEach(() => {
  unlocked = true
  configuration = { desktop_plugins: [frigate, { ...kerberos, enabled: false }] }
  readConfiguration = async () => structuredClone(configuration)
  native.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'auth_status') return { unlocked }
    if (command === 'get_plugin_snapshot') return [structuredClone(worker)]
    if (command === 'get_config') return readConfiguration()
    throw new Error(`Unexpected command: ${command}`)
  })
  native.listen.mockReset().mockImplementation(async (name, callback) => {
    callbacks.set(name, callback)
    return () => {
      if (callbacks.get(name) === callback) callbacks.delete(name)
    }
  })
})

afterEach(() => {
  for (const value of dashboards) value.stop()
  dashboards.length = 0
  callbacks.clear()
})

describe('dashboard configured plugin intent', () => {
  it('reads intent once per session independently of repeated worker telemetry', async () => {
    const value = dashboard()
    expect(value.configuredPlugins.value).toBeNull()
    expect(value.configurationUnavailable.value).toBe(true)
    await Promise.all([value.start(), value.start()])
    expect(value.configuredPlugins.value).toEqual([frigate, { ...kerberos, enabled: false }])
    expect(value.configurationUnavailable.value).toBe(false)
    for (let count = 0; count < 25; count += 1) emit('plugin-host-update')
    await value.refresh()
    expect(configurationReads()).toBe(1)
    expect(value.canAct(value.plugins.value[0])).toBe(true)
    expect(
      native.invoke.mock.calls.every(([command]) =>
        ['auth_status', 'get_plugin_snapshot', 'get_config'].includes(command)
      )
    ).toBe(true)
  })

  it('treats a valid configuration without desktop_plugins as an empty list', async () => {
    configuration = { mqtt_host: 'configured-host', mqtt_port: 1883 }
    const value = dashboard()
    await value.start()
    expect(value.configuredPlugins.value).toEqual([])
    expect(value.configurationUnavailable.value).toBe(false)
  })

  it.each([undefined, null, [], { desktop_plugins: null }, { desktop_plugins: [{}] }])(
    'keeps invalid initial configuration unknown without disabling worker actions: %j',
    async (invalid) => {
      configuration = invalid
      const value = dashboard()
      await value.start()
      expect(value.configuredPlugins.value).toBeNull()
      expect(value.configurationUnavailable.value).toBe(true)
      expect(value.canAct(value.plugins.value[0])).toBe(true)
    }
  )

  it('uses enable, disable and uninstall event payloads without configuration IPC', async () => {
    const value = dashboard()
    await value.start()
    emit('plugin-configuration-changed', { desktop_plugins: [frigate, kerberos] })
    expect(value.configuredPlugins.value).toEqual([frigate, kerberos])
    const disabled = { ...frigate, enabled: false }
    const declarations = [disabled]
    emit('plugin-configuration-changed', { desktop_plugins: declarations })
    disabled.enabled = true
    expect(value.configuredPlugins.value).toEqual([{ ...frigate, enabled: false }])
    emit('plugin-configuration-changed', { desktop_plugins: [] })
    expect(value.configuredPlugins.value).toEqual([])
    expect(value.configurationUnavailable.value).toBe(false)
    expect(configurationReads()).toBe(1)
  })

  it.each(['resolve', 'reject'] as const)(
    'does not let an old initial configuration %s replace a newer event',
    async (outcome) => {
      const pending = deferred<unknown>()
      readConfiguration = () => pending.promise
      const value = dashboard()
      const starting = value.start()
      await flushPromises()
      emit('plugin-configuration-changed', { desktop_plugins: [kerberos] })
      expect(value.configuredPlugins.value).toEqual([kerberos])
      expect(value.configurationUnavailable.value).toBe(false)
      if (outcome === 'resolve') pending.resolve({ desktop_plugins: [frigate] })
      else pending.reject(new Error('old read failed'))
      await starting
      expect(value.configuredPlugins.value).toEqual([kerberos])
      expect(value.configurationUnavailable.value).toBe(false)
    }
  )

  it('does not interpret a malformed change event as an empty configuration', async () => {
    const value = dashboard()
    await value.start()
    emit('plugin-configuration-changed', {})
    expect(value.configuredPlugins.value).toEqual([frigate, { ...kerberos, enabled: false }])
    expect(value.configurationUnavailable.value).toBe(true)
    emit('plugin-configuration-changed', { desktop_plugins: [] })
    expect(value.configuredPlugins.value).toEqual([])
    expect(value.configurationUnavailable.value).toBe(false)
    expect(configurationReads()).toBe(1)
  })

  it('retains known intent while a saved-config refresh fails, then recovers on another save', async () => {
    const value = dashboard()
    await value.start()
    const previous = structuredClone(configuration)
    const pending = deferred<unknown>()
    readConfiguration = () => pending.promise
    emit('config-saved', { color_scheme: 'dark' })
    expect(value.configurationUnavailable.value).toBe(true)
    pending.reject(new Error('native config unavailable'))
    await flushPromises()
    expect(value.configuredPlugins.value).toEqual(
      (previous as { desktop_plugins: unknown }).desktop_plugins
    )
    expect(value.configurationUnavailable.value).toBe(true)
    expect(value.canAct(value.plugins.value[0])).toBe(true)
    readConfiguration = async () => ({ desktop_plugins: [kerberos] })
    emit('config-saved')
    await flushPromises()
    expect(value.configuredPlugins.value).toEqual([kerberos])
    expect(value.configurationUnavailable.value).toBe(false)
    expect(configurationReads()).toBe(3)
  })

  it('discards superseded saved-config reads instead of restoring an earlier selection', async () => {
    const value = dashboard()
    await value.start()
    const first = deferred<unknown>()
    const latest = deferred<unknown>()
    readConfiguration = () => first.promise
    emit('config-saved')
    readConfiguration = () => latest.promise
    emit('config-saved')
    latest.resolve({ desktop_plugins: [kerberos] })
    await flushPromises()
    first.resolve({ desktop_plugins: [frigate] })
    await flushPromises()
    expect(value.configuredPlugins.value).toEqual([kerberos])
    expect(value.configurationUnavailable.value).toBe(false)
  })

  it.each(['resolve', 'reject'] as const)(
    'clears intent on logout and ignores the previous session configuration %s',
    async (outcome) => {
      const value = dashboard()
      await value.start()
      const pending = deferred<unknown>()
      readConfiguration = () => pending.promise
      emit('config-saved')
      unlocked = false
      emit('auth-state-changed')
      expect(value.configuredPlugins.value).toBeNull()
      expect(value.configurationUnavailable.value).toBe(true)
      await flushPromises()
      emit('plugin-configuration-changed', { desktop_plugins: [frigate] })
      expect(value.configuredPlugins.value).toBeNull()
      expect(configurationReads()).toBe(2)
      readConfiguration = async () => ({ desktop_plugins: [kerberos] })
      unlocked = true
      emit('auth-state-changed')
      await flushPromises()
      if (outcome === 'resolve') pending.resolve({ desktop_plugins: [frigate] })
      else pending.reject(new Error('old session failed'))
      await flushPromises()
      expect(value.configuredPlugins.value).toEqual([kerberos])
      expect(value.configurationUnavailable.value).toBe(false)
      expect(configurationReads()).toBe(3)
    }
  )

  it('clears intent on stop and ignores pending reads and detached listeners after restart', async () => {
    const value = dashboard()
    await value.start()
    const pending = deferred<unknown>()
    readConfiguration = () => pending.promise
    emit('config-saved')
    const staleEvent = callbacks.get('plugin-configuration-changed')!
    value.stop()
    expect(value.configuredPlugins.value).toBeNull()
    expect(value.configurationUnavailable.value).toBe(true)
    readConfiguration = async () => ({ desktop_plugins: [kerberos] })
    await value.start()
    pending.resolve({ desktop_plugins: [frigate] })
    staleEvent({ payload: { desktop_plugins: [frigate] } })
    await flushPromises()
    expect(value.configuredPlugins.value).toEqual([kerberos])
    expect(value.configurationUnavailable.value).toBe(false)
  })

  it('keeps worker actions available but connection intent unavailable if config events cannot be subscribed', async () => {
    native.listen
      .mockImplementationOnce(async (name, callback) => {
        callbacks.set(name, callback)
        return () => callbacks.delete(name)
      })
      .mockImplementationOnce(async (name, callback) => {
        callbacks.set(name, callback)
        return () => callbacks.delete(name)
      })
      .mockRejectedValueOnce(new Error('configuration event registration failed'))
    const value = dashboard()
    await value.start()
    expect(value.configuredPlugins.value).toEqual([frigate, { ...kerberos, enabled: false }])
    expect(value.configurationUnavailable.value).toBe(true)
    expect(value.canAct(value.plugins.value[0])).toBe(true)
  })
})

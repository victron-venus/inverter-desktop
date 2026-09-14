import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import PluginPanels from './PluginPanels.vue'
import { createPluginDashboard } from './usePluginDashboard'
import type { ActionContribution, PluginSnapshot } from './types'

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

const action: ActionContribution = {
  kind: 'action',
  id: 'refresh-card',
  title: 'Refresh readings',
  action_id: 'refresh',
  label: 'Refresh',
  params: { source: 'dashboard' },
}
const snapshot: PluginSnapshot = {
  plugin_id: 'example.monitor',
  state: 'running',
  generation: 1,
  restart_count: 0,
  last_error: null,
  contributions: [
    { kind: 'text', id: 'text', title: 'Note', text: '<img src=x onerror=alert(1)>' },
    { kind: 'metric', id: 'temperature', title: 'Temperature', value: 24.5, unit: '°C' },
    { kind: 'status', id: 'status', title: 'Connection', value: 'Ready', tone: 'success' },
    action,
  ],
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

const callbacks = new Map<string, () => void>()
const stops: Array<ReturnType<typeof vi.fn>> = []
const dashboards: Array<ReturnType<typeof createPluginDashboard>> = []
let wrapper: VueWrapper | undefined
let currentSnapshot: PluginSnapshot[]
let unlocked: boolean

function registeredCallback(name: string) {
  const callback = callbacks.get(name)
  if (!callback) throw new Error(`Missing listener: ${name}`)
  return callback
}

function dashboard() {
  const result = createPluginDashboard()
  dashboards.push(result)
  return result
}

beforeEach(() => {
  currentSnapshot = [structuredClone(snapshot)]
  unlocked = true
  native.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'auth_status') return { unlocked }
    if (command === 'get_plugin_snapshot') return structuredClone(currentSnapshot)
    return undefined
  })
  native.listen.mockReset().mockImplementation(async (name: string, callback: () => void) => {
    callbacks.set(name, callback)
    const stop = vi.fn(() => callbacks.delete(name))
    stops.push(stop)
    return stop
  })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  for (const value of dashboards) value.stop()
  dashboards.length = 0
  callbacks.clear()
  stops.length = 0
})

describe('desktop worker dashboard', () => {
  it('renders no panel or install prompt when no workers contribute', async () => {
    currentSnapshot = []
    wrapper = mount(PluginPanels)
    await flushPromises()
    expect(wrapper.text()).toBe('')
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('renders host text as text, metrics and status with only declared action arguments', async () => {
    wrapper = mount(PluginPanels)
    await flushPromises()
    expect(wrapper.text()).toContain('<img src=x onerror=alert(1)>')
    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.find('a').exists()).toBe(false)
    expect(wrapper.text()).toContain('24.5°C')
    expect(wrapper.find('output').text()).toBe('Ready')
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('plugin_action', {
      pluginId: snapshot.plugin_id,
      actionId: action.action_id,
      params: action.params,
    })
  })

  it('disables pending actions, rejects duplicate clicks and presents generic retryable errors', async () => {
    const pending = deferred<unknown>()
    native.invoke.mockImplementation(async (command: string) => {
      if (command === 'auth_status') return { unlocked: true }
      if (command === 'get_plugin_snapshot') return [structuredClone(snapshot)]
      if (command === 'plugin_action') return pending.promise
    })
    wrapper = mount(PluginPanels)
    await flushPromises()
    const button = wrapper.find('button')
    await button.trigger('click')
    expect(button.attributes('disabled')).toBeDefined()
    expect(button.attributes('aria-busy')).toBe('true')
    await button.trigger('click')
    expect(
      native.invoke.mock.calls.filter(([command]) => command === 'plugin_action')
    ).toHaveLength(1)
    pending.reject(new Error('private worker path must not reach the view'))
    await flushPromises()
    expect(wrapper.find('[role="alert"]').text()).toBe('plugins.actionFailed')
    expect(wrapper.text()).not.toContain('private worker path')
    expect(button.attributes('disabled')).toBeUndefined()
  })

  it('uses only currently advertised actions and disables them after a failed snapshot refresh', async () => {
    const value = dashboard()
    await value.start()
    await value.runAction(snapshot.plugin_id, { ...action, id: 'unadvertised' })
    await value.runAction(snapshot.plugin_id, { ...action, action_id: 'changed-action' })
    expect(
      native.invoke.mock.calls.filter(([command]) => command === 'plugin_action')
    ).toHaveLength(0)
    native.invoke.mockRejectedValueOnce(new Error('disconnected'))
    await value.refresh()
    expect(value.plugins.value).toHaveLength(1)
    expect(value.canAct(value.plugins.value[0])).toBe(false)
    await value.runAction(snapshot.plugin_id, action)
    expect(
      native.invoke.mock.calls.filter(([command]) => command === 'plugin_action')
    ).toHaveLength(0)
    await value.refresh()
    expect(value.canAct(value.plugins.value[0])).toBe(true)
  })

  it('shows a failed worker without granting its stale actions', async () => {
    currentSnapshot[0].state = 'failed'
    wrapper = mount(PluginPanels)
    await flushPromises()
    expect(wrapper.text()).toContain('plugins.unavailable')
    expect(wrapper.find('button').attributes('disabled')).toBeDefined()
  })

  it('subscribes once, refetches authoritative snapshots, and removes listeners on stop', async () => {
    const value = dashboard()
    await Promise.all([value.start(), value.start()])
    await value.start()
    expect(native.listen).toHaveBeenCalledTimes(2)
    currentSnapshot = []
    registeredCallback('plugin-host-update')()
    await flushPromises()
    expect(value.plugins.value).toEqual([])
    const staleUpdate = registeredCallback('plugin-host-update')
    value.stop()
    expect(stops.every((stop) => stop.mock.calls.length === 1)).toBe(true)
    const previousCalls = native.invoke.mock.calls.length
    staleUpdate()
    expect(native.invoke).toHaveBeenCalledTimes(previousCalls)
  })

  it('ignores a snapshot response overtaken by a newer host update', async () => {
    const value = dashboard()
    await value.start()
    const pending = deferred<PluginSnapshot[]>()
    native.invoke.mockReturnValueOnce(pending.promise)
    const oldRefresh = value.refresh()
    currentSnapshot = []
    registeredCallback('plugin-host-update')()
    await flushPromises()
    pending.resolve([structuredClone(snapshot)])
    await oldRefresh
    expect(value.plugins.value).toEqual([])
  })

  it('clears contributions synchronously on logout and ignores old snapshots and action results', async () => {
    const value = dashboard()
    await value.start()
    const pendingSnapshot = deferred<PluginSnapshot[]>()
    const pendingAction = deferred<unknown>()
    native.invoke.mockReturnValueOnce(pendingAction.promise)
    const actionResult = value.runAction(snapshot.plugin_id, action)
    native.invoke.mockReturnValueOnce(pendingSnapshot.promise)
    const oldRefresh = value.refresh()
    unlocked = false
    registeredCallback('auth-state-changed')()
    expect(value.plugins.value).toEqual([])
    expect(value.pendingActions.value.size).toBe(0)
    pendingSnapshot.resolve([structuredClone(snapshot)])
    pendingAction.reject(new Error('late failure'))
    await Promise.all([actionResult, oldRefresh])
    await flushPromises()
    expect(value.plugins.value).toEqual([])
    expect(value.failedActions.value.size).toBe(0)
    unlocked = true
    registeredCallback('auth-state-changed')()
    await flushPromises()
    expect(value.plugins.value).toHaveLength(1)
    expect(native.listen).toHaveBeenCalledTimes(2)
  })

  it('discards an old authentication result after a later logout', async () => {
    const value = dashboard()
    await value.start()
    const pending = deferred<{ unlocked: boolean }>()
    native.invoke.mockReturnValueOnce(pending.promise)
    registeredCallback('auth-state-changed')()
    unlocked = false
    registeredCallback('auth-state-changed')()
    await flushPromises()
    pending.resolve({ unlocked: true })
    await flushPromises()
    expect(value.plugins.value).toEqual([])
  })

  it('cleans up a late subscription without starting a new listener after unmount', async () => {
    const pending = deferred<() => void>()
    const stop = vi.fn()
    native.listen.mockReturnValueOnce(pending.promise)
    const value = dashboard()
    const startup = value.start()
    value.stop()
    pending.resolve(stop)
    await startup
    expect(stop).toHaveBeenCalledOnce()
    expect(native.listen).toHaveBeenCalledTimes(1)
    expect(native.invoke).not.toHaveBeenCalled()
  })

  it('fails closed when an event subscription is unavailable', async () => {
    native.listen.mockRejectedValueOnce(new Error('event registration failed'))
    const value = dashboard()
    await value.start()
    expect(value.plugins.value).toEqual([])
    expect(native.invoke).not.toHaveBeenCalled()
    await value.start()
    expect(value.plugins.value).toHaveLength(1)
  })

  it('keeps a restarted controller separate from an old pending subscription', async () => {
    const pending = deferred<() => void>()
    const lateStop = vi.fn()
    native.listen.mockReturnValueOnce(pending.promise)
    const value = dashboard()
    const oldStartup = value.start()
    value.stop()
    await value.start()
    pending.resolve(lateStop)
    await oldStartup
    expect(lateStop).toHaveBeenCalledOnce()
    expect(native.listen).toHaveBeenCalledTimes(3)
    expect(value.plugins.value).toHaveLength(1)
    currentSnapshot = []
    registeredCallback('plugin-host-update')()
    await flushPromises()
    expect(value.plugins.value).toEqual([])
  })

  it('does not restore data after a mounted panel is removed during its initial snapshot', async () => {
    const pending = deferred<PluginSnapshot[]>()
    native.invoke.mockImplementation(async (command: string) => {
      if (command === 'auth_status') return { unlocked: true }
      if (command === 'get_plugin_snapshot') return pending.promise
    })
    wrapper = mount(PluginPanels)
    await flushPromises()
    wrapper.unmount()
    wrapper = undefined
    pending.resolve([structuredClone(snapshot)])
    await flushPromises()
    expect(stops.every((stop) => stop.mock.calls.length === 1)).toBe(true)
    expect(callbacks.size).toBe(0)
  })
})

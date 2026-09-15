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
  instance_id: 'worker-instance-1',
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

function actionCalls() {
  return native.invoke.mock.calls.filter(([command]) => command === 'plugin_action')
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
    expect(wrapper.find('button').attributes('aria-label')).toBe('Refresh readings: Refresh')
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('plugin_action', {
      pluginId: snapshot.plugin_id,
      instanceId: snapshot.instance_id,
      actionId: action.action_id,
      params: action.params,
    })
  })

  it('disables pending actions, rejects duplicate clicks and presents generic unconfirmed-outcome errors', async () => {
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
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, {
      ...action,
      id: 'unadvertised',
    })
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, {
      ...action,
      action_id: 'changed-action',
    })
    expect(
      native.invoke.mock.calls.filter(([command]) => command === 'plugin_action')
    ).toHaveLength(0)
    native.invoke.mockRejectedValueOnce(new Error('disconnected'))
    await value.refresh()
    expect(value.plugins.value).toHaveLength(1)
    expect(value.canAct(value.plugins.value[0])).toBe(false)
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
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

  it.each([
    { params: { source: 'replacement-target' } },
    { title: 'Replacement title' },
    { label: 'Replacement label' },
  ])(
    'rejects a stale clicked descriptor after its advertised fields change: %j',
    async (change) => {
      const value = dashboard()
      await value.start()
      const replacement = { ...structuredClone(action), ...change }
      currentSnapshot[0].contributions = [replacement]
      await value.refresh()

      await value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
      expect(actionCalls()).toHaveLength(0)
      await value.runAction(snapshot.plugin_id, snapshot.instance_id, replacement)
      expect(actionCalls()).toEqual([
        [
          'plugin_action',
          {
            pluginId: snapshot.plugin_id,
            instanceId: snapshot.instance_id,
            actionId: replacement.action_id,
            params: replacement.params,
          },
        ],
      ])
    }
  )

  it('compares nested JSON and distinct Unicode keys by content while keeping separate operations independent', async () => {
    const value = dashboard()
    const first = {
      ...action,
      params: {
        target: { entity: 'button.first', area: 'home' },
        values: [1, 2],
        labels: { '\u00e9': 'composed', 'e\u0301': 'decomposed' },
      },
    }
    const alias = {
      ...first,
      id: 'alias',
      title: 'Same operation',
      params: {
        labels: { 'e\u0301': 'decomposed', '\u00e9': 'composed' },
        values: [1, 2],
        target: { area: 'home', entity: 'button.first' },
      },
    }
    const second = {
      ...first,
      id: 'second',
      params: { target: { entity: 'button.second', area: 'home' }, values: [1, 2] },
    }
    currentSnapshot[0].contributions = [first, alias, second]
    await value.start()
    const pendingFirst = deferred<unknown>()
    const pendingSecond = deferred<unknown>()
    native.invoke.mockReturnValueOnce(pendingFirst.promise)
    const firstResult = value.runAction(snapshot.plugin_id, snapshot.instance_id, {
      ...first,
      params: alias.params,
    })
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, alias)
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, {
      ...first,
      params: { ...first.params, values: [2, 1] },
    })
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, {
      ...first,
      params: { ...first.params, labels: { '\u00e9': 'decomposed', 'e\u0301': 'composed' } },
    })
    expect(actionCalls()).toHaveLength(1)
    native.invoke.mockReturnValueOnce(pendingSecond.promise)
    const secondResult = value.runAction(snapshot.plugin_id, snapshot.instance_id, second)
    expect(actionCalls()).toHaveLength(2)
    expect(value.pendingActions.value.size).toBe(2)
    pendingFirst.resolve(undefined)
    pendingSecond.resolve(undefined)
    await Promise.all([firstResult, secondResult])
    expect(value.pendingActions.value.size).toBe(0)
  })

  it('rejects null and replaced worker instances even when reinstall reuses generation counters', async () => {
    const value = dashboard()
    currentSnapshot[0].instance_id = null
    await value.start()
    expect(value.canAct(value.plugins.value[0])).toBe(false)
    await value.runAction(snapshot.plugin_id, null, action)
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
    currentSnapshot[0].instance_id = 'reinstalled-worker'
    await value.refresh()
    expect(value.plugins.value[0].generation).toBe(snapshot.generation)
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
    expect(actionCalls()).toHaveLength(0)
    await value.runAction(snapshot.plugin_id, 'reinstalled-worker', action)
    expect(actionCalls()[0][1]).toMatchObject({ instanceId: 'reinstalled-worker' })
  })

  it.each(['resolve', 'reject'] as const)(
    'ignores an old instance action %s after replacement',
    async (outcome) => {
      const value = dashboard()
      await value.start()
      const oldAction = deferred<unknown>()
      native.invoke.mockReturnValueOnce(oldAction.promise)
      const oldResult = value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
      currentSnapshot[0].instance_id = 'reinstalled-worker'
      await value.refresh()
      expect(value.pendingActions.value.size).toBe(0)
      const newAction = deferred<unknown>()
      native.invoke.mockReturnValueOnce(newAction.promise)
      const newResult = value.runAction(snapshot.plugin_id, 'reinstalled-worker', action)
      const before = native.invoke.mock.calls.length
      oldAction[outcome](new Error('old instance result'))
      await oldResult
      expect(native.invoke.mock.calls).toHaveLength(before)
      expect(value.failedActions.value.size).toBe(0)
      expect(
        value.pendingActions.value.has(
          value.actionKey(snapshot.plugin_id, 'reinstalled-worker', action)
        )
      ).toBe(true)
      newAction.resolve(undefined)
      await newResult
      expect(value.pendingActions.value.size).toBe(0)
    }
  )

  it('preserves pending and unknown outcome feedback when the visible action name changes', async () => {
    const pending = deferred<unknown>()
    wrapper = mount(PluginPanels)
    await flushPromises()
    native.invoke.mockReturnValueOnce(pending.promise)
    await wrapper.find('button').trigger('click')
    const renamed = { ...action, title: 'Kitchen button', label: 'Press' }
    currentSnapshot[0].contributions = [renamed]
    registeredCallback('plugin-host-update')()
    await flushPromises()
    expect(wrapper.find('button').attributes('aria-label')).toBe('Kitchen button: Press')
    expect(wrapper.find('button').attributes('disabled')).toBeDefined()
    expect(wrapper.find('button').attributes('aria-busy')).toBe('true')
    await wrapper.find('button').trigger('click')
    expect(actionCalls()).toHaveLength(1)
    pending.reject(new Error('service response lost'))
    await flushPromises()
    expect(wrapper.find('[role="alert"]').exists()).toBe(true)
    currentSnapshot[0].contributions = [{ ...renamed, title: 'Renamed kitchen button' }]
    registeredCallback('plugin-host-update')()
    await flushPromises()
    expect(wrapper.find('[role="alert"]').exists()).toBe(true)
    expect(actionCalls()).toHaveLength(1)
  })

  it('rejects withdrawn actions and preserves their pending outcome through a same-instance reconnect', async () => {
    const value = dashboard()
    await value.start()
    const pending = deferred<unknown>()
    native.invoke.mockReturnValueOnce(pending.promise)
    const result = value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
    currentSnapshot[0].contributions = []
    await value.refresh()
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
    currentSnapshot[0].contributions = [structuredClone(action)]
    await value.refresh()
    await value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
    expect(actionCalls()).toHaveLength(1)
    expect(value.pendingActions.value.size).toBe(1)
    currentSnapshot[0].contributions = []
    await value.refresh()
    pending.reject(new Error('disconnected after submission'))
    await result
    await flushPromises()
    currentSnapshot[0].contributions = [structuredClone(action)]
    await value.refresh()
    expect(
      value.failedActions.value.has(
        value.actionKey(snapshot.plugin_id, snapshot.instance_id, action)
      )
    ).toBe(true)
    expect(value.pendingActions.value.size).toBe(0)
    expect(actionCalls()).toHaveLength(1)
  })

  it('clears removed instance outcomes and ignores its late failure after uninstall', async () => {
    const value = dashboard()
    await value.start()
    const pending = deferred<unknown>()
    native.invoke.mockReturnValueOnce(pending.promise)
    const result = value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
    currentSnapshot = []
    await value.refresh()
    const before = native.invoke.mock.calls.length
    pending.reject(new Error('worker removed'))
    await result
    expect(value.pendingActions.value.size).toBe(0)
    expect(value.failedActions.value.size).toBe(0)
    expect(native.invoke.mock.calls).toHaveLength(before)
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

  it('coalesces update bursts and applies each snapshot without starvation', async () => {
    const value = dashboard()
    await value.start()
    const first = deferred<PluginSnapshot[]>()
    const second = deferred<PluginSnapshot[]>()
    native.invoke.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    const before = native.invoke.mock.calls.length
    const refreshing = value.refresh()
    for (let count = 0; count < 25; count += 1) registeredCallback('plugin-host-update')()
    expect(native.invoke.mock.calls).toHaveLength(before + 1)
    const changed = { ...structuredClone(snapshot), restart_count: 1 }
    first.resolve([changed])
    await flushPromises()
    expect(value.plugins.value[0].restart_count).toBe(1)
    expect(native.invoke.mock.calls).toHaveLength(before + 2)
    second.resolve([])
    await refreshing
    expect(value.plugins.value).toEqual([])
    expect(native.invoke.mock.calls).toHaveLength(before + 2)
  })

  it('releases a completed action while a continuous update stream keeps snapshot refresh busy', async () => {
    const value = dashboard()
    await value.start()
    const pendingAction = deferred<unknown>()
    native.invoke.mockReturnValueOnce(pendingAction.promise)
    const result = value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
    const firstSnapshot = deferred<PluginSnapshot[]>()
    const nextSnapshot = deferred<PluginSnapshot[]>()
    native.invoke
      .mockReturnValueOnce(firstSnapshot.promise)
      .mockReturnValueOnce(nextSnapshot.promise)
    const refreshing = value.refresh()
    for (let count = 0; count < 100; count += 1) registeredCallback('plugin-host-update')()
    pendingAction.resolve(undefined)
    await result
    expect(value.pendingActions.value.size).toBe(0)
    const changed = { ...structuredClone(snapshot), restart_count: 2 }
    firstSnapshot.resolve([changed])
    await flushPromises()
    expect(value.plugins.value[0].restart_count).toBe(2)
    for (let count = 0; count < 100; count += 1) registeredCallback('plugin-host-update')()
    nextSnapshot.resolve([changed])
    await refreshing
    expect(
      native.invoke.mock.calls.filter(([command]) => command === 'get_plugin_snapshot')
    ).toHaveLength(4)
    expect(actionCalls()).toHaveLength(1)
  })

  it.each(['authentication', 'controller'] as const)(
    'invalidates an in-flight snapshot and action across a new %s lifetime',
    async (change) => {
      const value = dashboard()
      await value.start()
      const pendingAction = deferred<unknown>()
      native.invoke.mockReturnValueOnce(pendingAction.promise)
      const oldAction = value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
      const pendingSnapshot = deferred<PluginSnapshot[]>()
      native.invoke.mockReturnValueOnce(pendingSnapshot.promise)
      const oldRefresh = value.refresh()
      currentSnapshot[0].instance_id = 'current-session-worker'
      let startup: Promise<void> | undefined
      if (change === 'authentication') {
        registeredCallback('auth-state-changed')()
      } else {
        value.stop()
        startup = value.start()
      }
      await flushPromises()
      expect(value.plugins.value).toEqual([])
      expect(value.pendingActions.value.size).toBe(0)
      expect(
        native.invoke.mock.calls.filter(([command]) => command === 'get_plugin_snapshot')
      ).toHaveLength(2)
      pendingSnapshot.resolve([structuredClone(snapshot)])
      pendingAction.reject(new Error('prior lifetime failure'))
      await Promise.all([oldRefresh, oldAction, startup])
      await flushPromises()
      expect(value.plugins.value[0].instance_id).toBe('current-session-worker')
      expect(value.failedActions.value.size).toBe(0)
      expect(
        native.invoke.mock.calls.filter(([command]) => command === 'get_plugin_snapshot')
      ).toHaveLength(3)
    }
  )

  it('clears contributions synchronously on logout and ignores old snapshots and action results', async () => {
    const value = dashboard()
    await value.start()
    const pendingSnapshot = deferred<PluginSnapshot[]>()
    const pendingAction = deferred<unknown>()
    native.invoke.mockReturnValueOnce(pendingAction.promise)
    const actionResult = value.runAction(snapshot.plugin_id, snapshot.instance_id, action)
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

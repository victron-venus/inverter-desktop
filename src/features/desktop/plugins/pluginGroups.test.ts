import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import PluginGroupActions from './PluginGroupActions.vue'
import type { PluginGroup } from './types'
const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }))
let wrapper: VueWrapper | undefined
let groups: PluginGroup[]
let unlocked = true
let fail = false
const callbacks = new Map<string, () => void>()
beforeEach(() => {
  groups = [
    {
      id: 'cameras',
      title: 'Cameras',
      icon: 'camera',
      enabled: false,
      plugin_ids: ['example.camera'],
    },
  ]
  unlocked = true
  fail = false
  native.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'auth_status') return { unlocked }
    if (command === 'get_plugin_groups') {
      if (fail) throw new Error('unavailable')
      return structuredClone(groups)
    }
  })
  native.listen.mockReset().mockImplementation(async (name: string, callback: () => void) => {
    callbacks.set(name, callback)
    return () => callbacks.delete(name)
  })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  callbacks.clear()
})
async function open() {
  wrapper = mount(PluginGroupActions)
  await flushPromises()
  return wrapper
}
function event(name: string) {
  const callback = callbacks.get(name)
  if (!callback) throw new Error(`Missing ${name}`)
  callback()
}
describe('native-owned plugin group monitoring', () => {
  it('can re-enable a stopped package through group authority without worker actions', async () => {
    const mounted = await open()
    expect(mounted.get('button').attributes('aria-pressed')).toBe('false')
    await mounted.get('button').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('set_plugin_group_enabled', {
      groupId: 'cameras',
      enabled: true,
    })
    expect(
      native.invoke.mock.calls.some(
        ([command]) => command === 'plugin_action' || command === 'save_config'
      )
    ).toBe(false)
  })
  it('disables retained group state on refresh failure and recovers without creating a new listener', async () => {
    const mounted = await open()
    fail = true
    event('plugin-host-update')
    await flushPromises()
    expect(mounted.get('button').attributes('disabled')).toBeDefined()
    fail = false
    groups[0].enabled = true
    event('plugin-host-update')
    await flushPromises()
    expect(mounted.get('button').attributes('disabled')).toBeUndefined()
    expect(mounted.get('button').attributes('aria-pressed')).toBe('true')
    expect(native.listen).toHaveBeenCalledTimes(2)
  })
  it('hides group state on logout and ignores an earlier mutation failure', async () => {
    const mounted = await open()
    let reject: ((reason: Error) => void) | undefined
    native.invoke.mockImplementation(async (command: string) => {
      if (command === 'auth_status') return { unlocked }
      if (command === 'get_plugin_groups') return groups
      if (command === 'set_plugin_group_enabled')
        return new Promise((_resolve, fail) => {
          reject = fail
        })
    })
    await mounted.get('button').trigger('click')
    unlocked = false
    event('auth-state-changed')
    await flushPromises()
    if (!reject) throw new Error('Missing pending mutation')
    reject(new Error('earlier operation failed'))
    await flushPromises()
    expect(mounted.find('button').exists()).toBe(false)
    expect(mounted.emitted('error')).toBeUndefined()
  })
  it('renders nothing without installed group declarations and releases listeners on unmount', async () => {
    groups = []
    const mounted = await open()
    expect(mounted.text()).toBe('')
    mounted.unmount()
    wrapper = undefined
    expect(callbacks.size).toBe(0)
  })
})

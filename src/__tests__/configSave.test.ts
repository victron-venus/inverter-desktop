import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Config from '../Config.vue'
import { defaultConfig } from '../config'
import { useConfigForm } from '../composables/useConfigForm'

const boundary = vi.hoisted(() => ({ invoke: vi.fn(), emit: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('@tauri-apps/api/event', () => ({
  emit: boundary.emit,
  listen: vi.fn(async () => () => {}),
}))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
let wrapper: VueWrapper | undefined
beforeEach(() => {
  boundary.invoke.mockReset().mockImplementation(async (name: string) => {
    if (name === 'get_config') return { ...defaultConfig }
    if (name === 'get_state') return {}
    return undefined
  })
  boundary.emit.mockReset().mockResolvedValue(undefined)
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
})

describe('Configuration save result', () => {
  it('disables Save and keeps the load failure visible instead of saving defaults', async () => {
    boundary.invoke.mockRejectedValue(new Error('Cannot decrypt configuration'))
    wrapper = mount(Config)
    await flushPromises()
    expect(wrapper.text()).toContain('Failed to load config: Error: Cannot decrypt configuration')
    const save = wrapper.get('button[title="Save changes"]')
    expect(save.attributes('disabled')).toBeDefined()
    await save.trigger('click')
    expect(boundary.invoke).not.toHaveBeenCalledWith('save_config', expect.anything())
    expect(boundary.emit).not.toHaveBeenCalled()
  })

  it('rejects failed loads and guards saves until a later load succeeds', async () => {
    const form = useConfigForm()
    boundary.invoke.mockRejectedValueOnce(new Error('Read denied'))
    await expect(form.loadConfig()).rejects.toThrow('Read denied')
    expect(form.configLoaded.value).toBe(false)
    expect(await form.saveConfig([], [])).toBe(false)
    expect(boundary.invoke).not.toHaveBeenCalledWith('save_config', expect.anything())
    await form.loadConfig()
    expect(form.configLoaded.value).toBe(true)
    expect(await form.saveConfig([], [])).toBe(true)
  })

  it('keeps the write error visible and does not publish settings or change autostart', async () => {
    wrapper = mount(Config)
    await flushPromises()
    boundary.invoke.mockImplementation(async (name: string) => {
      if (name === 'save_config') throw new Error('Permission denied')
    })
    await wrapper.get('button[title="Save changes"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Failed to save config: Error: Permission denied')
    expect(wrapper.text()).not.toContain('Settings saved successfully')
    expect(boundary.invoke).not.toHaveBeenCalledWith('set_auto_start', expect.anything())
    expect(boundary.emit).not.toHaveBeenCalledWith('config-saved', expect.anything())
    expect(wrapper.get('button[title="Save changes"]').attributes('disabled')).toBeUndefined()
  })

  it('publishes config-saved and applies autostart only after durable save succeeds', async () => {
    wrapper = mount(Config)
    await flushPromises()
    await wrapper.get('button[title="Save changes"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Settings saved successfully')
    expect(boundary.emit).toHaveBeenCalledWith('config-saved', {
      color_scheme: defaultConfig.color_scheme,
    })
    const commands = boundary.invoke.mock.calls.map(([command]) => command)
    expect(commands.indexOf('set_auto_start')).toBeGreaterThan(commands.indexOf('save_config'))
  })
})

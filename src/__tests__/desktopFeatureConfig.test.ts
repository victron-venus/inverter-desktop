import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Config from '../Config.vue'
import SetupWizard from '../components/SetupWizard.vue'
import { defaultConfig } from '../config'
import {
  featureConnection,
  featureConfigSections,
  subscribeFeatureConfig,
} from '../features/desktop'
const native = vi.hoisted(() => ({ invoke: vi.fn(), emit: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ emit: native.emit, listen: native.listen }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
const initial = {
  ...defaultConfig,
  ha_url: 'https://legacy.invalid',
  ha_longlived_token: 'retained-legacy',
  camera_enabled: true,
  camera_live_urls: { front: 'https://private.invalid/live' },
  show_washer: false,
  modules: { future: { schema_version: 7, values: { enabled: false } } },
  ha_entities: [
    { id: 'room', label: 'Room', entity: 'light.room', domain: 'light', enabled: true },
  ],
  future_setting: { retained: true },
}
let wrapper: VueWrapper | undefined
beforeEach(() => {
  native.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'get_config') return structuredClone(initial)
    if (command === 'get_state') return {}
    return undefined
  })
  native.emit.mockReset().mockResolvedValue(undefined)
  native.listen.mockReset().mockResolvedValue(() => {})
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
})
describe('desktop optional integrations', () => {
  it('exposes package settings without bundled service tabs and preserves passive records on core save', async () => {
    wrapper = mount(Config)
    await flushPromises()
    expect(featureConfigSections.map((section) => section.id)).toEqual(['plugins'])
    expect(wrapper.find('#ha_url').exists()).toBe(false)
    expect(wrapper.find('#ha_token').exists()).toBe(false)
    await wrapper.get('button[title="Save changes"]').trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('save_config', {
      config: expect.objectContaining({ ...initial, header_toggles_config: [] }),
    })
    expect(
      native.invoke.mock.calls.some(([command]) =>
        /^(connect_ha|discover_ha|ha_|camera_)/.test(command)
      )
    ).toBe(false)
  })
  it('applies package declaration changes without replacing dirty core or passive settings', async () => {
    let update:
      | ((event: {
          payload: {
            desktop_plugins: NonNullable<import('../config').AppConfig['desktop_plugins']>
          }
        }) => void)
      | undefined
    native.listen.mockImplementationOnce(async (_name, callback) => {
      update = callback
      return () => {}
    })
    const draft: import('../config').AppConfig = { ...initial, mqtt_host: 'unsaved-core-host' }
    let active = true
    const stop = await subscribeFeatureConfig(draft, () => active)
    if (!update) throw new Error('Missing package settings listener')
    const declarations = [
      { plugin_id: 'example.camera', version: '1.0.0', enabled: false, artifacts: {} },
    ]
    update({ payload: { desktop_plugins: declarations } })
    expect(draft.mqtt_host).toBe('unsaved-core-host')
    expect(draft.modules).toEqual(initial.modules)
    expect(draft.desktop_plugins).toEqual(declarations)
    active = false
    update({ payload: { desktop_plugins: [] } })
    expect(draft.desktop_plugins).toEqual(declarations)
    stop()
  })
  it('never connects optional services from a core connection, even with saved legacy credentials', async () => {
    await featureConnection.connect(initial)
    featureConnection.cleanup()
    expect(native.invoke).not.toHaveBeenCalled()
  })
  it('starts setup without bundled integrations or automatic activation', async () => {
    wrapper = mount(SetupWizard)
    await flushPromises()
    expect(wrapper.find('#setup_ha_url').exists()).toBe(false)
    expect(wrapper.findAll('button').some((button) => button.text() === 'Advanced')).toBe(false)
    expect(defaultConfig.ha_url).toBeUndefined()
    expect(defaultConfig.camera_enabled).toBeUndefined()
  })
})

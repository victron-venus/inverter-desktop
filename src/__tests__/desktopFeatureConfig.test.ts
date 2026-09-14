import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Config from '../Config.vue'
import SetupWizard from '../components/SetupWizard.vue'
import IntegrationConfig from '../features/desktop/IntegrationConfig.vue'
import SectionVisibility from '../features/desktop/SectionVisibility.vue'
import { defaultConfig } from '../config'

const boundary = vi.hoisted(() => ({ invoke: vi.fn(), emit: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('@tauri-apps/api/event', () => ({
  emit: boundary.emit,
  listen: vi.fn(async () => () => {}),
}))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
const global = { mocks: { $t: (key: string) => key } }
let wrapper: VueWrapper | undefined
const initial = {
  ...defaultConfig,
  ha_url: '',
  ha_longlived_token: '',
  ha_use_direct_api: false,
  camera_enabled: true,
  show_washer: true,
  header_toggles_config: [{ id: 'limit', label: 'Limit', entity: 'no_feed', state_key: 'no_feed' }],
  future_setting: { retained: true },
}
beforeEach(() => {
  boundary.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'get_config') return structuredClone(initial)
    if (command === 'get_state') return {}
    return undefined
  })
  boundary.emit.mockReset().mockResolvedValue(undefined)
  vi.stubGlobal(
    'confirm',
    vi.fn(() => true)
  )
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  vi.unstubAllGlobals()
})

async function openTab(label: string) {
  const tab = wrapper!.findAll('button').find((button) => button.text() === label)
  if (!tab) throw new Error(`Missing settings tab: ${label}`)
  await tab.trigger('click')
}

function writtenConfig() {
  return boundary.invoke.mock.calls
    .filter(([command]) => command === 'save_config')
    .slice(-1)[0]?.[1].config
}

describe('desktop feature configuration model', () => {
  it('emits checkbox edits without mutating the supplied config and follows replacement props', async () => {
    const supplied = { ...initial }
    wrapper = mount(SectionVisibility, { props: { config: supplied }, global })
    await wrapper.find('input[type="checkbox"]').setValue(false)
    expect(supplied.show_washer).toBe(true)
    expect(wrapper.emitted('update:config')?.[0]?.[0]).toEqual({ ...supplied, show_washer: false })
    await wrapper.setProps({ config: { ...supplied, show_washer: true } })
    expect((wrapper.find('input[type="checkbox"]').element as HTMLInputElement).checked).toBe(true)
  })

  it('edits and saves the shared reactive draft through feature model events without replacing its identity', async () => {
    wrapper = mount(Config, { global })
    await flushPromises()
    await openTab('Home Assistant & Cameras')
    const integration = wrapper.findComponent(IntegrationConfig)
    const draft = integration.props('config')
    await integration.find('#ha_url').setValue('https://home.example.com')
    await integration.find('#ha_token').setValue('test-token')
    await integration.find('#ha_port').setValue('9443')
    await integration.find('input[type="checkbox"]').setValue(false)
    expect(integration.props('config')).toBe(draft)
    expect(draft).toMatchObject({
      ha_url: 'https://home.example.com',
      ha_port: 9443,
      ha_use_direct_api: true,
      camera_enabled: false,
    })
    await openTab('Sections')
    const visibility = wrapper.findComponent(SectionVisibility)
    expect(visibility.props('config')).toBe(draft)
    await visibility.find('input[type="checkbox"]').setValue(false)
    await wrapper.find('button[title="Save changes"]').trigger('click')
    await flushPromises()
    expect(writtenConfig()).toMatchObject({
      ha_url: 'https://home.example.com',
      ha_port: 9443,
      ha_use_direct_api: true,
      camera_enabled: false,
      show_washer: false,
      future_setting: initial.future_setting,
      header_toggles_config: initial.header_toggles_config,
    })
    expect(defaultConfig.show_washer).toBe(true)
    await wrapper.find('button[title="Reset to defaults"]').trigger('click')
    expect(confirm).toHaveBeenCalledOnce()
    expect(draft.show_washer).toBe(true)
    expect(visibility.props('config')).toBe(draft)
    expect((visibility.find('input[type="checkbox"]').element as HTMLInputElement).checked).toBe(
      true
    )
    await openTab('Home Assistant & Cameras')
    expect((wrapper.find('#ha_url').element as HTMLInputElement).value).toBe('')
  })

  it('saves setup feature edits through the shared wizard, preserving numeric model conversion', async () => {
    wrapper = mount(SetupWizard, { global })
    await flushPromises()
    await openTab('Advanced')
    await wrapper.find('#setup_ha_url').setValue('https://home.example.com')
    await wrapper.find('#setup_ha_token').setValue('setup-token')
    await wrapper.find('#setup_ha_port').setValue('8443')
    const save = wrapper.findAll('button').find((button) => button.text() === 'Save & Continue')!
    await save.trigger('click')
    await flushPromises()
    expect(writtenConfig()).toMatchObject({
      ha_url: 'https://home.example.com',
      ha_longlived_token: 'setup-token',
      ha_port: 8443,
      ha_use_direct_api: true,
      mqtt_host: initial.mqtt_host,
      setup_completed: true,
    })
    expect(wrapper.emitted('complete')?.[0]?.[0]).toMatchObject({ ha_port: 8443 })
  })
  it.each(['config-first', 'features-first'])(
    'initializes desktop defaults with %s imports',
    async (order) => {
      vi.resetModules()
      if (order === 'features-first') await import('@features')
      const loaded = await import('../config')
      if (order === 'config-first') await import('@features')
      expect(loaded.defaultConfig).toMatchObject({
        show_washer: true,
        camera_enabled: true,
        mqtt_ha_host: 'HA',
      })
    }
  )
})

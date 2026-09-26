import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Config from '../Config.vue'
import { useConfigForm } from '../composables/useConfigForm'
import { defaultConfig, getAppConfig, sectionKeys, sectionVisibility } from '../config'

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
  boundary.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'get_config') {
      return { ...defaultConfig, ha_url: '', ha_longlived_token: '', ha_use_direct_api: false }
    }
    if (command === 'get_state') return {}
    return undefined
  })
  boundary.emit.mockReset().mockResolvedValue(undefined)
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
})

describe('Section configuration', () => {
  const defaults = {
    show_batteries: true,
    show_solar_production: true,
    show_active_loads: false,
    show_daily_stats: false,
    show_ev: true,
    show_home_section: false,
    show_header_toggles: false,
  }
  const labels = [
    'Batteries',
    'Solar Production',
    'Active Loads',
    'Daily Stats',
    'EV',
    'Home Buttons',
    'config.headerToggles',
  ]

  function checkbox(label: string) {
    const field = wrapper?.findAll('label').find((entry) => entry.text().trim() === label)
    if (!field) throw new Error(`Missing checkbox: ${label}`)
    return field.get('input[type="checkbox"]')
  }

  async function openSections() {
    wrapper = mount(Config)
    await flushPromises()
    const tab = wrapper.findAll('button').find((button) => button.text() === 'Sections')
    if (!tab) throw new Error('Missing Sections tab')
    await tab.trigger('click')
  }

  it('uses the requested first-run defaults in the form and dashboard', async () => {
    await openSections()
    expect(sectionVisibility(defaultConfig)).toEqual(defaults)
    expect(sectionVisibility(null)).toEqual(defaults)
    sectionKeys.forEach((key, index) => {
      expect((checkbox(labels[index]).element as HTMLInputElement).checked).toBe(defaults[key])
    })
    expect(wrapper?.text()).not.toContain('Console')
    expect(wrapper?.text()).not.toContain('Authentication')
    expect(wrapper?.find('#auth_username').exists()).toBe(false)
    expect(wrapper?.findAll('button').some((button) => button.text() === 'UI Controls')).toBe(false)
  })

  it.each([null, undefined])(
    'migrates %s section values to the visible legacy layout consistently',
    async (legacy) => {
      const saved = {
        ...defaultConfig,
        ...Object.fromEntries(sectionKeys.map((key) => [key, legacy])),
      }
      boundary.invoke.mockImplementation(async (command) => (command === 'get_config' ? saved : {}))
      const dashboard = await getAppConfig()
      await openSections()
      sectionKeys.forEach((key, index) => {
        expect(dashboard[key]).toBe(true)
        expect((checkbox(labels[index]).element as HTMLInputElement).checked).toBe(true)
      })
      await wrapper?.get('button[title="Save changes"]').trigger('click')
      await flushPromises()
      const written = boundary.invoke.mock.calls.find(([command]) => command === 'save_config')?.[1]
        .config
      expect(sectionVisibility(written)).toEqual(
        Object.fromEntries(sectionKeys.map((key) => [key, true]))
      )
    }
  )

  it('honors explicit section choices through a save and a fresh dashboard read', async () => {
    let saved = { ...defaultConfig }
    boundary.invoke.mockImplementation(async (command, args) => {
      if (command === 'get_config') return structuredClone(saved)
      if (command === 'save_config') saved = JSON.parse(JSON.stringify(args.config))
      return {}
    })
    await openSections()
    for (const label of labels) await checkbox(label).setValue(false)
    await wrapper?.get('button[title="Save changes"]').trigger('click')
    await flushPromises()
    expect(sectionVisibility(await getAppConfig())).toEqual(
      Object.fromEntries(sectionKeys.map((key) => [key, false]))
    )
    for (const label of labels) await checkbox(label).setValue(true)
    await wrapper?.get('button[title="Save changes"]').trigger('click')
    await flushPromises()
    expect(sectionVisibility(await getAppConfig())).toEqual(
      Object.fromEntries(sectionKeys.map((key) => [key, true]))
    )
  })

  it('preserves existing app authentication and passive migration data on reset and save', async () => {
    const saved = {
      ...defaultConfig,
      auth_enabled: true,
      auth_biometric: true,
      auth_username: 'existing',
      header_toggles_config: [{ id: 'room', label: 'Room', entity: 'light.room' }],
      ha_entities: [
        { id: 'room', label: 'Room', entity: 'light.room', domain: 'light', enabled: true },
      ],
    }
    boundary.invoke.mockImplementation(async (command) => (command === 'get_config' ? saved : {}))
    const form = useConfigForm()
    await form.loadConfig()
    form.resetToDefaults()
    expect(await form.saveConfig()).toBe(true)
    expect(form.config).toMatchObject(saved)
  })
})

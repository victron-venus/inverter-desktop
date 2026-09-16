import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Config from '../Config.vue'
import HeaderTogglesEditor from '../components/HeaderTogglesEditor.vue'
import { useConfigForm } from '../composables/useConfigForm'
import { isDashboardControlTarget } from '../dashboardControlTarget'
import { useCoreControlsConfig as useDashboardControlsConfig } from '../features/coreControlsConfig'
import { defaultConfig } from '../config'
import { DEFAULT_INVERTER_CONTROLS, INVERTER_CONTROL_FLAGS } from '../inverterControl'

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

describe('Dashboard control configuration', () => {
  it.each(INVERTER_CONTROL_FLAGS)('accepts native flag %s and saved HA aliases', (flag) => {
    expect(isDashboardControlTarget(flag)).toBe(true)
    expect(isDashboardControlTarget(`input_boolean.${flag}`)).toBe(true)
    expect(isDashboardControlTarget(`switch.${flag}`)).toBe(true)
  })

  it('accepts custom HA entities and rejects malformed or unknown bare targets', () => {
    expect(isDashboardControlTarget('switch.garage')).toBe(true)
    expect(isDashboardControlTarget('input_boolean.vacation_mode')).toBe(true)
    for (const target of ['', 'garage', 'only_charging ', 'switch.', 'x.y.no_feed', 'switch.a b']) {
      expect(isDashboardControlTarget(target)).toBe(false)
    }
  })

  it('preserves saved aliases and state keys, and clones added inverter presets', () => {
    const manager = useDashboardControlsConfig()
    const saved = {
      id: 'custom_charging_id',
      label: 'Charge',
      entity: 'input_boolean.only_charging',
      state_key: 'only_charging',
    }
    manager.loadFromConfig({ ...defaultConfig, header_toggles_config: [saved] })
    expect(manager.headerTogglesList.value).toEqual([{ ...saved, entity: 'only_charging' }])
    manager.addHeaderToggle(DEFAULT_INVERTER_CONTROLS[1])
    manager.headerTogglesList.value[1].label = 'Custom label'
    expect(DEFAULT_INVERTER_CONTROLS[1].label).not.toBe('Custom label')
  })

  it('adds an inverter preset without reusing an existing custom HA control id', () => {
    const manager = useDashboardControlsConfig()
    manager.loadFromConfig({
      ...defaultConfig,
      header_toggles_config: [
        { id: 'no_feed', label: 'Garage', entity: 'switch.garage' },
        { id: 'no_feed_2', label: 'Porch', entity: 'switch.porch' },
      ],
    })
    manager.addHeaderToggle(
      DEFAULT_INVERTER_CONTROLS.find((control) => control.entity === 'no_feed')
    )
    expect(manager.headerTogglesList.value).toEqual([
      { id: 'no_feed_3', label: 'NO FEED', entity: 'no_feed' },
    ])
  })

  it('preserves opaque optional controls without discovery while editing core controls', () => {
    const manager = useDashboardControlsConfig()
    const external = {
      id: 'room',
      label: 'Room',
      entity: 'light.room',
      domain: 'light',
      enabled: true,
    }
    manager.loadFromConfig({ ...defaultConfig, ha_entities: [external] })
    expect(manager.haEntitiesList.value).toEqual([])
    manager.addHomeControl()
    manager.haEntitiesList.value[0] = {
      id: 'charge',
      label: 'Charge',
      entity: 'only_charging',
      domain: 'inverter_control',
      enabled: true,
    }
    expect(manager.getSavedControls().home).toEqual([external, manager.haEntitiesList.value[0]])
    expect(boundary.invoke).not.toHaveBeenCalled()
  })

  it('offers all native flags and treats a saved HA alias as an existing inverter control', () => {
    wrapper = mount(HeaderTogglesEditor, {
      props: {
        headerTogglesList: [
          { id: 'old', label: 'Charging', entity: 'input_boolean.only_charging' },
        ],
        discoveredEntities: [],
      },
    })
    expect(wrapper.findAll('button[data-control]')).toHaveLength(7)
    expect(wrapper.get('button[data-control="only_charging"]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('button[data-control="no_feed"]').attributes('disabled')).toBeUndefined()
  })

  it('configures and saves native flags without HA credentials or discovery', async () => {
    wrapper = mount(Config)
    await flushPromises()
    const tab = wrapper.findAll('button').find((button) => button.text() === 'UI Controls')
    expect(tab).toBeDefined()
    if (!tab) throw new Error('UI Controls tab was not rendered')
    await tab.trigger('click')
    for (const flag of INVERTER_CONTROL_FLAGS) {
      await wrapper.get(`button[data-control="${flag}"]`).trigger('click')
    }
    await wrapper.get('button[title="Save changes"]').trigger('click')
    await flushPromises()
    expect(boundary.invoke).toHaveBeenCalledWith('save_config', {
      config: expect.objectContaining({
        ha_url: '',
        ha_longlived_token: '',
        header_toggles_config: DEFAULT_INVERTER_CONTROLS,
      }),
    })
    expect(boundary.invoke.mock.calls.some(([command]) => command === 'discover_ha_entities')).toBe(
      false
    )
  })

  it('blocks an invalid target before saving without changing valid legacy records', async () => {
    const form = useConfigForm()
    await form.loadConfig()
    expect(await form.saveConfig([], [{ id: '', label: 'Invalid', entity: 'unknown_flag' }])).toBe(
      false
    )
    expect(form.message.value).toContain('Invalid header control target')
    expect(boundary.invoke).not.toHaveBeenCalledWith('save_config', expect.anything())
    const controls = [
      {
        id: 'legacy',
        label: 'Charge',
        entity: 'input_boolean.only_charging',
        state_key: 'only_charging',
      },
      { id: '', label: 'Garage', entity: 'switch.garage' },
    ]
    expect(await form.saveConfig([], controls)).toBe(true)
    expect(form.config.header_toggles_config).toEqual([
      controls[0],
      { id: 'switch_garage', label: 'Garage', entity: 'switch.garage' },
    ])
    expect(form.config.header_toggles_config?.[0]).toHaveProperty('state_key', 'only_charging')
  })
})

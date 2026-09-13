import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AppHeader from '../components/AppHeader.vue'
import { sendControlAction, useDashboardControls } from '../composables/useDashboardControls'
import {
  appConfig,
  applyInverterState,
  resetInverterState,
  state,
} from '../composables/useInverterState'
import { defaultConfig } from '../config'
import {
  DEFAULT_INVERTER_CONTROLS,
  INVERTER_CONTROL_FLAGS,
  inverterControlFlagKey,
  mqttControlState,
} from '../inverterControl'

const boundary = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

function currentConfig() {
  const config = appConfig.value
  if (!config) throw new Error('Test configuration is missing')
  return config
}

beforeEach(() => {
  resetInverterState()
  appConfig.value = { ...defaultConfig, ha_use_direct_api: false }
  boundary.invoke.mockReset().mockResolvedValue(undefined)
})

describe('inverter-control presentation without HA', () => {
  it('uses published controls and MQTT states without creating the HA composable', () => {
    applyInverterState({
      ui_config: { header_toggles: [{ id: 'export', label: 'EXPORT LIMIT', entity: 'no_feed' }] },
      booleans: { no_feed: true, export: false },
    })
    const controls = useDashboardControls()
    expect(controls.headerControls.value).toEqual([
      { id: 'export', label: 'EXPORT LIMIT', entity: 'no_feed' },
    ])
    expect(controls.headerControlStates.value.export).toBe('on')
    applyInverterState({ booleans: { no_feed: false } })
    expect(controls.headerControlStates.value.export).toBe('off')
    expect(boundary.invoke).not.toHaveBeenCalled()
  })

  it('keeps an explicit empty daemon list and uses defaults only for older metadata', () => {
    const controls = useDashboardControls()
    expect(controls.headerControls.value).toEqual(DEFAULT_INVERTER_CONTROLS)
    applyInverterState({ ui_config: { header_toggles: [] } })
    expect(controls.headerControls.value).toEqual([])
  })

  it('preserves saved order/labels and normalizes legacy targets without mutating config', () => {
    currentConfig().header_toggles_config = [
      { id: 'custom', label: 'Saved label', entity: 'input_boolean.only_charging' },
    ]
    applyInverterState({ booleans: { only_charging: true } })
    const adapter = vi.fn(() => 'off' as const)
    const controls = useDashboardControls(adapter)
    expect(controls.headerControls.value[0]).toEqual({
      id: 'custom',
      label: 'Saved label',
      entity: 'only_charging',
    })
    expect(controls.headerControlStates.value.custom).toBe('on')
    expect(adapter).not.toHaveBeenCalled()
    expect(currentConfig().header_toggles_config?.[0].entity).toBe('input_boolean.only_charging')
  })

  it('keeps actual HA switches in the optional adapter even when their id resembles a flag', () => {
    currentConfig().header_toggles_config = [
      { id: 'no_feed', label: 'HA relay', entity: 'switch.no_feed' },
    ]
    const adapter = vi.fn(() => 'unavailable' as const)
    const controls = useDashboardControls(adapter)
    expect(controls.headerControlStates.value.no_feed).toBe('unavailable')
    expect(adapter).toHaveBeenCalledWith({
      id: 'no_feed',
      label: 'HA relay',
      entity: 'switch.no_feed',
    })
  })

  it('uses the same authoritative MQTT flag for a control placed in Home', () => {
    currentConfig().ha_entities = [
      {
        id: 'custom',
        label: 'Limit',
        entity: 'input_boolean.no_feed',
        domain: 'input_boolean',
        enabled: true,
      },
    ]
    state.value = { booleans: { no_feed: true, home_custom: false } }
    const controls = useDashboardControls()
    expect(controls.homeButtonStates.value.custom).toBe('on')
    expect(controls.homeButtons.value[0].entity).toBe('no_feed')
  })

  it('supports canonical keys and only the documented legacy aliases', () => {
    for (const key of INVERTER_CONTROL_FLAGS) {
      expect(inverterControlFlagKey(key)).toBe(key)
      expect(inverterControlFlagKey(`input_boolean.${key}`)).toBe(key)
      expect(inverterControlFlagKey(`switch.${key}`)).toBeNull()
    }
    for (const value of [true, 1, 'true', '1']) {
      expect(mqttControlState({ id: 'no_feed', entity: 'no_feed' }, { no_feed: value })).toBe('on')
    }
    for (const value of [false, 0, 'false', '0', undefined]) {
      expect(mqttControlState({ id: 'no_feed', entity: 'no_feed' }, { no_feed: value })).toBe('off')
    }
  })

  it('sends the displayed bare flag to the core dispatcher when clicked', async () => {
    const controls = useDashboardControls()
    const wrapper = mount(AppHeader, {
      props: {
        dryRun: false,
        essClass: 'off',
        essText: 'ESS',
        showHeaderToggles: true,
        headerControls: controls.headerControls.value,
        controlStates: controls.headerControlStates.value,
        isDark: false,
      },
    })
    const button = wrapper.findAll('button').find((item) => item.text() === 'ONLY CHARGING')
    if (!button) throw new Error('Inverter control was not rendered')
    await button.trigger('click')
    const event = wrapper.emitted('send')?.[0]
    if (!event) throw new Error('Click did not emit a control command')
    const [action, payload] = event as [string, Record<string, unknown>]
    await sendControlAction(action, payload)
    expect(boundary.invoke).toHaveBeenCalledExactlyOnceWith('perform_action', {
      action: 'toggle',
      payload: { entity: 'only_charging' },
    })
    wrapper.unmount()
  })
})

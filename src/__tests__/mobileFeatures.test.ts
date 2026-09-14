import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import {
  featureConnection,
  featureSetupAvailable,
  getFeatureView,
  useDashboardFeatures,
} from '@features'
import Config from '../Config.vue'
import AppHeader from '../components/AppHeader.vue'
import SetupWizard from '../components/SetupWizard.vue'
import { useDashboardControls, sendControlAction } from '../composables/useDashboardControls'
import { useConnection } from '../composables/useConnection'
import { appConfig, state } from '../composables/useInverterState'
import { defaultConfig } from '../config'

const { invoke, listen, emit } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  emit: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen, emit }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }))
vi.mock('@tauri-apps/plugin-notification', () => ({
  isPermissionGranted: vi.fn(async () => true),
  requestPermission: vi.fn(async () => 'granted'),
}))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

const config = {
  mqtt_host: 'Cerbo',
  mqtt_port: 1883,
  setup_completed: true,
  ha_url: 'http://saved-ha',
  ha_longlived_token: 'saved-token',
  ha_use_direct_api: true,
  mqtt_ha_host: 'camera-broker',
  mqtt_ha_port: 1883,
  camera_enabled: true,
  header_toggles_config: [
    { id: 'limit', label: 'Export limit', entity: 'no_feed', state_key: 'no_feed' },
    {
      id: 'lamp',
      label: 'Living room lamp',
      entity: 'switch.lamp',
      state_key: 'home_lamp',
      future_field: true,
    },
  ],
  ha_entities: [
    {
      id: 'lamp',
      label: 'Living room lamp',
      entity: 'switch.lamp',
      domain: 'switch',
      enabled: true,
    },
  ],
}
const globalOptions = { mocks: { $t: (key: string) => key } }

beforeEach(() => {
  vi.stubGlobal('localStorage', {
    getItem: vi.fn(() => null),
    setItem: vi.fn(),
    removeItem: vi.fn(),
  })
  invoke.mockReset()
  listen.mockReset()
  emit.mockReset()
  listen.mockResolvedValue(() => {})
  invoke.mockImplementation(async (command: string) =>
    command === 'get_config' ? structuredClone(config) : command === 'get_state' ? {} : undefined
  )
  appConfig.value = { ...defaultConfig, ...structuredClone(config) }
  state.value = { booleans: { no_feed: true }, ui_config: {}, features: { ha: true } }
})

describe('mobile build feature boundary', () => {
  it('uses the mobile port with no feature routes or transport calls', async () => {
    expect(featureSetupAvailable).toBe(false)
    expect(getFeatureView('/camera-video')).toBeUndefined()
    await featureConnection.connect(config)
    featureConnection.cleanup()
    expect(invoke).not.toHaveBeenCalled()
    expect(listen).not.toHaveBeenCalled()
  })

  it('renders and dispatches core inverter controls while hiding stored external controls', async () => {
    const Harness = defineComponent({
      setup() {
        const features = useDashboardFeatures()
        const controls = useDashboardControls(features.getControlState, features.allowHomeControls)
        return () =>
          h(AppHeader, {
            dryRun: false,
            essClass: 'on',
            essText: 'ESS',
            isDark: true,
            showHeaderToggles: true,
            headerControls: controls.headerControls.value,
            controlStates: controls.headerControlStates.value,
            onSend: sendControlAction,
          })
      },
    })
    const wrapper = mount(Harness, { global: globalOptions })
    expect(wrapper.text()).toContain('Export limit')
    expect(wrapper.text()).not.toContain('Living room lamp')
    const button = wrapper.findAll('button').find((entry) => entry.text() === 'Export limit')!
    await button.trigger('click')
    expect(invoke).toHaveBeenCalledWith('perform_action', {
      action: 'toggle',
      payload: { entity: 'no_feed' },
    })
    wrapper.unmount()
  })

  it('starts the shared connection without optional-service commands or listeners', async () => {
    const connection = useConnection()
    await connection.connectMqtt()
    expect(invoke).toHaveBeenCalledWith('connect_mqtt', expect.objectContaining({ host: 'Cerbo' }))
    expect(
      invoke.mock.calls.map(([command]) => command).some((command) => /ha|camera/.test(command))
    ).toBe(false)
    expect(listen.mock.calls.map(([name]) => name).some((name) => /ha-|camera/.test(name))).toBe(
      false
    )
    connection.cleanup()
  })

  it('shows core setup and settings and preserves hidden definitions when saving the shared form', async () => {
    const setup = mount(SetupWizard, { global: globalOptions })
    await flushPromises()
    expect(setup.text()).toContain('Cerbo MQTT')
    expect(setup.text()).not.toContain('Advanced')
    expect(setup.find('#setup_ha_url').exists()).toBe(false)
    setup.unmount()

    const wrapper = mount(Config, { global: globalOptions })
    await flushPromises()
    expect(wrapper.text()).toContain('Cerbo Devices')
    expect(wrapper.text()).not.toContain('Home Assistant')
    expect(wrapper.find('#ha_url').exists()).toBe(false)
    const devicesTab = wrapper.findAll('button').find((entry) => entry.text() === 'Cerbo Devices')!
    await devicesTab.trigger('click')
    expect(wrapper.text()).toContain('Water tanks')
    expect(wrapper.find('#water_tank_instance').exists()).toBe(true)
    const mqttTab = wrapper.findAll('button').find((entry) => entry.text() === 'MQTT Broker')!
    await mqttTab.trigger('click')
    await wrapper.find('#mqtt_host').setValue('NewCerbo')
    await wrapper.find('button[title="Save changes"]').trigger('click')
    await flushPromises()
    const saved = invoke.mock.calls
      .filter(([command]) => command === 'save_config')
      .slice(-1)[0]?.[1].config
    expect(saved.mqtt_host).toBe('NewCerbo')
    expect(saved.header_toggles_config[1]).toEqual(config.header_toggles_config[1])
    expect(saved.ha_entities).toEqual(config.ha_entities)
    expect(saved.ha_url).toBe(config.ha_url)
    wrapper.unmount()
  })
})

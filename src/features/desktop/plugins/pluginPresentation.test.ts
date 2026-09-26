import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { appConfig, resetInverterState, state } from '../../../composables/useInverterState'
import { useDashboardControls } from '../../../composables/useDashboardControls'
import { defaultConfig } from '../../../config'
import PluginCompactPanels from './PluginCompactPanels.vue'
import PluginConnectionStatus from './PluginConnectionStatus.vue'
import PluginNumberSlider from './PluginNumberSlider.vue'
import { createPluginPresentation, pluginPresentationKey } from './presentation'
import type { NumberInputContribution, PluginSnapshot } from './types'
const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
let plugins: PluginSnapshot[]
let context: ReturnType<typeof createPluginPresentation>
let wrappers: VueWrapper[]
const callbacks = new Map<string, () => void>()
const input: NumberInputContribution = {
  kind: 'number_input',
  id: 'position',
  title: 'Shade position',
  action_id: 'set-position',
  label: 'Set',
  input_revision: 'read-1',
  value_scaled: 40,
  min_scaled: 0,
  max_scaled: 100,
  step_scaled: 1,
  decimal_places: 0,
}
const snapshot: PluginSnapshot = {
  plugin_id: 'example.home',
  instance_id: 'worker-1',
  state: 'running',
  generation: 1,
  restart_count: 0,
  last_error: null,
  contributions: [
    {
      kind: 'action',
      id: 'action-ref',
      title: 'Room',
      action_id: 'fresh-toggle',
      label: 'Toggle',
      params: { entity: 'light.room' },
    },
    { kind: 'status', id: 'room', title: 'Room', value: 'On', tone: 'success' },
    {
      kind: 'status',
      id: 'hidden-entity',
      title: 'Not projected',
      value: 'hidden diagnostic',
      tone: 'neutral',
    },
    input,
  ],
  presentation: [
    {
      kind: 'control',
      id: 'header-room',
      surface: 'header',
      order: 1,
      title: 'Room',
      state: 'on',
      action: 'action-ref',
    },
    {
      kind: 'control',
      id: 'home-room',
      surface: 'home',
      order: 1,
      title: 'Room',
      state: 'on',
      action: 'action-ref',
      icon: 'light',
    },
    {
      kind: 'group',
      id: 'sensors',
      surface: 'sidebar',
      order: 0,
      title: 'Sensors',
      collapsed: true,
      rows: [
        {
          id: 'row',
          title: 'Room',
          value: 'room',
          actions: [{ id: 'action-ref', label: 'Toggle' }],
          input: 'position',
        },
      ],
    },
    {
      kind: 'summary',
      id: 'washer',
      surface: 'sidebar',
      order: 1,
      title: 'Washer',
      visible: true,
      active: true,
      text: '12 min',
      actions: [],
    },
    {
      kind: 'summary',
      id: 'dryer',
      surface: 'sidebar',
      order: 2,
      title: 'Hidden dryer',
      visible: false,
      active: false,
      text: '',
      actions: [],
    },
    {
      kind: 'weather',
      id: 'weather',
      surface: 'sidebar',
      order: 3,
      title: 'Weather',
      condition: 'sunny',
      temperature: '24',
      unit: '°C',
      forecast: [{ datetime: '2026-09-17', condition: 'sunny', temperature: '25' }],
    },
    { kind: 'connection', id: 'health', title: 'Home service', connected: true },
  ],
}
beforeEach(async () => {
  resetInverterState()
  appConfig.value = { ...defaultConfig }
  plugins = [structuredClone(snapshot)]
  wrappers = []
  native.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'auth_status') return { unlocked: true }
    if (command === 'get_plugin_snapshot') return structuredClone(plugins)
  })
  native.listen.mockReset().mockImplementation(async (name: string, callback: () => void) => {
    callbacks.set(name, callback)
    return () => callbacks.delete(name)
  })
  context = createPluginPresentation()
  await context.dashboard.start()
})
afterEach(() => {
  for (const wrapper of wrappers) wrapper.unmount()
  context.dashboard.stop()
  callbacks.clear()
  appConfig.value = null
})
function mountPanels() {
  const wrapper = mount(PluginCompactPanels, {
    global: { provide: { [pluginPresentationKey as symbol]: context } },
  })
  wrappers.push(wrapper)
  return wrapper
}
describe('compact installed-package presentation', () => {
  it('renders only explicit compact projections, preserves collapse, and dispatches exact contribution refs', async () => {
    const wrapper = mountPanels()
    expect(wrapper.text()).toContain('Sensors (1)')
    expect(wrapper.find('[data-presentation-row]').exists()).toBe(false)
    expect(wrapper.text()).toContain('Washer')
    expect(wrapper.text()).toContain('12 min')
    expect(wrapper.text()).toContain('24°C')
    expect(wrapper.text()).not.toMatch(/hidden diagnostic|Not projected|Hidden dryer|example.home/)
    await wrapper.get('button').trigger('click')
    expect(wrapper.findAll('[data-presentation-row]')).toHaveLength(1)
    const action = wrapper.findAll('button').find((button) => button.text() === 'Toggle')
    if (!action) throw new Error('Missing compact action')
    await action.trigger('click')
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('plugin_action', {
      pluginId: 'example.home',
      instanceId: 'worker-1',
      actionId: 'fresh-toggle',
      params: { entity: 'light.room' },
    })
    plugins = []
    await context.dashboard.refresh()
    expect(wrapper.find('section').exists()).toBe(false)
  })
  it('renders no fallback diagnostic panel for a package without projections', async () => {
    plugins = [{ ...snapshot, presentation: undefined }]
    await context.dashboard.refresh()
    expect(mountPanels().text()).toBe('')
  })
  it('merges controller and plugin ordering and never sends plugin controls through core IPC', async () => {
    if (!appConfig.value) throw new Error('Missing config')
    state.value.ui_config = {}
    state.value.ui_config.header_toggles = [
      { id: 'charge', label: 'Charge', entity: 'only_charging' },
      { id: 'external', label: 'Old room', entity: 'light.room' },
      { id: 'feed', label: 'Feed', entity: 'no_feed' },
    ]
    state.value.ui_config.home_buttons = [
      { id: 'empty', label: 'Unknown', entity: 'light.unknown' },
      { id: 'external', label: 'Room', entity: 'light.room' },
      { id: 'feed', label: 'Feed', entity: 'no_feed' },
    ]
    const core = useDashboardControls(undefined, false)
    expect(
      context.mergeControls('header', core.headerControls.value).map((control) => control.label)
    ).toEqual(['Charge', 'Room', 'Feed'])
    const home = context.mergeControls('home', core.homeButtons.value)
    expect(home.map((control) => control.label)).toEqual(['Room', 'Feed'])
    home[0].activate?.()
    await flushPromises()
    expect(
      native.invoke.mock.calls.filter(([command]) => command === 'plugin_action')
    ).toHaveLength(1)
    expect(native.invoke.mock.calls.some(([command]) => command === 'perform_action')).toBe(false)
  })
  it('keeps unknown-state controls clickable only while their explicit action remains published', async () => {
    for (const item of plugins[0].presentation ?? []) {
      if (item.kind === 'control') item.state = 'unavailable'
    }
    await context.dashboard.refresh()
    const [control] = context.mergeControls('header', [])
    expect(control).toMatchObject({ state: 'unavailable', disabled: false })
    control.activate?.()
    await flushPromises()
    expect(native.invoke).toHaveBeenCalledWith('plugin_action', {
      pluginId: 'example.home',
      instanceId: 'worker-1',
      actionId: 'fresh-toggle',
      params: { entity: 'light.room' },
    })
    for (const item of plugins[0].presentation ?? []) {
      if (item.kind === 'control') delete item.action
    }
    plugins[0].contributions = plugins[0].contributions.filter((item) => item.kind !== 'action')
    await context.dashboard.refresh()
    expect(context.mergeControls('header', [])[0].disabled).toBe(true)
    native.invoke.mockClear()
    control.activate?.()
    await flushPromises()
    expect(native.invoke.mock.calls.some(([command]) => command === 'plugin_action')).toBe(false)
  })
  it('marks stale controls unavailable, shows connection offline, and rejects retained handlers after withdrawal', async () => {
    const wrapper = mount(PluginConnectionStatus, {
      global: { provide: { [pluginPresentationKey as symbol]: context } },
    })
    wrappers.push(wrapper)
    expect(wrapper.find('.status-dot-on').exists()).toBe(true)
    const [control] = context.mergeControls('header', [])
    native.invoke.mockRejectedValueOnce(new Error('snapshot failure'))
    await context.dashboard.refresh()
    expect(context.mergeControls('header', [])[0]).toMatchObject({
      disabled: true,
      state: 'unavailable',
    })
    expect(wrapper.find('.status-dot-on').exists()).toBe(false)
    control.activate?.()
    await flushPromises()
    expect(native.invoke.mock.calls.some(([command]) => command === 'plugin_action')).toBe(false)
    plugins = [{ ...snapshot, contributions: [], presentation: [] }]
    await context.dashboard.refresh()
    control.activate?.()
    await flushPromises()
    expect(native.invoke.mock.calls.some(([command]) => command === 'plugin_action')).toBe(false)
  })
  it.each(['Frigate', 'Kerberos', 'Ring'])(
    'tracks %s MQTT subscription, reconnect, and worker removal in the status bar',
    async (provider) => {
      const title = `${provider} MQTT`
      const connection = {
        kind: 'connection' as const,
        id: `${provider.toLowerCase()}-mqtt`,
        title,
        connected: false,
      }
      plugins = [
        {
          ...snapshot,
          plugin_id: `inverter-desktop.${provider.toLowerCase()}`,
          contributions: [],
          presentation: [connection],
        },
      ]
      await context.dashboard.refresh()
      const wrapper = mount(PluginConnectionStatus, {
        global: { provide: { [pluginPresentationKey as symbol]: context } },
      })
      wrappers.push(wrapper)
      expect(wrapper.text()).toBe(title)
      expect(wrapper.find('.status-dot-on').exists()).toBe(false)
      for (const connected of [true, false, true]) {
        connection.connected = connected
        await context.dashboard.refresh()
        expect(wrapper.text()).toBe(title)
        expect(wrapper.find('.status-dot-on').exists()).toBe(connected)
      }
      plugins[0].state = 'failed'
      await context.dashboard.refresh()
      expect(wrapper.find('.status-dot-on').exists()).toBe(false)
      plugins = []
      await context.dashboard.refresh()
      expect(wrapper.find('.status-dot').exists()).toBe(false)
      expect(wrapper.text()).toBe('')
    }
  )
  it('rejects a slider release when its authority changed during dragging', async () => {
    const wrapper = mount(PluginNumberSlider, {
      props: { input, disabled: false, pending: false, failed: false },
    })
    wrappers.push(wrapper)
    await wrapper.get('input').trigger('pointerdown')
    await wrapper.setProps({ input: { ...input, input_revision: 'read-2' } })
    await wrapper.get('input').setValue('50')
    expect(wrapper.emitted('submit')).toBeUndefined()
    await wrapper.get('input').trigger('pointerdown')
    await wrapper.get('input').setValue('60')
    expect(wrapper.emitted('submit')?.[0]).toEqual([{ ...input, input_revision: 'read-2' }, 60])
  })
})

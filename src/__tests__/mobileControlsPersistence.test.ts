import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useCoreControlsConfig } from '../features/coreControlsConfig'
import { useConfigForm } from '../composables/useConfigForm'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const saved = {
  mqtt_host: 'Cerbo',
  mqtt_port: 1883,
  ha_url: 'http://existing-home',
  ha_entities: [
    {
      id: 'lamp',
      label: 'Lamp',
      entity: 'switch.lamp',
      domain: 'switch',
      enabled: true,
      future_option: { retained: true },
    },
    { id: 'limit', label: 'Limit', entity: 'no_feed', domain: 'flag', enabled: true },
  ],
  header_toggles_config: [
    { id: 'limit', label: 'Limit', entity: 'input_boolean.no_feed', state_key: 'no_feed' },
    {
      id: 'opaque',
      label: 'Other',
      entity: 'future/opaque',
      state_key: 'custom',
      future_option: 42,
    },
    { id: 'charge', label: 'Charge', entity: 'charge_battery', state_key: 'charge_battery' },
  ],
}

beforeEach(() => {
  invoke.mockReset()
  invoke.mockImplementation(async (command: string) =>
    command === 'get_config' ? structuredClone(saved) : undefined
  )
})

describe('mobile control configuration roundtrip', () => {
  it('edits visible inverter controls while preserving hidden controls and their opaque fields', async () => {
    const form = useConfigForm()
    const controls = useCoreControlsConfig()
    controls.loadFromConfig(await form.loadConfig())
    expect(controls.headerTogglesList.value.map((entry) => entry.entity)).toEqual([
      'no_feed',
      'charge_battery',
    ])
    controls.headerTogglesList.value[0].label = 'Export limit'
    controls.moveToggleUp(1)
    const edits = controls.getSavedControls()
    expect(await form.saveConfig(edits.home, edits.header, edits.editableHeader)).toBe(true)
    const written = invoke.mock.calls.find(([command]) => command === 'save_config')?.[1].config
    expect(written.header_toggles_config).toEqual([
      saved.header_toggles_config[2],
      saved.header_toggles_config[1],
      { ...saved.header_toggles_config[0], label: 'Export limit', entity: 'no_feed' },
    ])
    expect(written.ha_entities).toEqual(saved.ha_entities)
    expect(written.ha_url).toBe(saved.ha_url)
  })

  it('resets visible controls without deleting hidden desktop definitions', async () => {
    const form = useConfigForm()
    const controls = useCoreControlsConfig()
    controls.loadFromConfig(await form.loadConfig())
    controls.headerTogglesList.value = []
    controls.haEntitiesList.value = []
    const edits = controls.getSavedControls()
    expect(await form.saveConfig(edits.home, edits.header, edits.editableHeader)).toBe(true)
    const written = invoke.mock.calls.find(([command]) => command === 'save_config')?.[1].config
    expect(written.header_toggles_config).toEqual([saved.header_toggles_config[1]])
    expect(written.ha_entities).toEqual([saved.ha_entities[0]])
  })
  it('reserves hidden IDs for presets and automatically named controls', async () => {
    const form = useConfigForm()
    const controls = useCoreControlsConfig()
    const loaded = await form.loadConfig()
    loaded.header_toggles_config = [{ id: 'no_feed', label: 'Hidden', entity: 'switch.no_feed' }]
    controls.loadFromConfig(loaded)
    controls.addHeaderToggle({ id: 'no_feed', label: 'Export limit', entity: 'no_feed' })
    expect(controls.headerTogglesList.value[0].id).toBe('no_feed_2')
    controls.addHeaderToggle({ id: '', label: 'Another limit', entity: 'no_feed' })
    const edits = controls.getSavedControls()
    expect(await form.saveConfig(edits.home, edits.header, edits.editableHeader)).toBe(true)
    const written = invoke.mock.calls.find(([command]) => command === 'save_config')?.[1].config
    expect(written.header_toggles_config.map((entry: { id: string }) => entry.id)).toEqual([
      'no_feed',
      'no_feed_2',
      'no_feed_3',
    ])
  })
})

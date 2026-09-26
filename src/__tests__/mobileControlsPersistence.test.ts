import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useConfigForm } from '../composables/useConfigForm'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const saved = {
  modules: {
    'example.future': {
      schema_version: 407,
      values: { layout: [{ kind: 'future-card', options: { visible: false } }], empty: null },
    },
    'example.offline': { schema_version: 99, values: { nested: [true, 12.5, 'Label'] } },
  },
  mqtt_host: 'Cerbo',
  mqtt_port: 1883,
  ha_url: 'http://existing-home',
  desktop_plugins: [
    {
      plugin_id: 'example.monitor',
      version: '1.2.3',
      enabled: false,
      artifacts: {
        'aarch64-apple-darwin': {
          url: 'https://packages.example.invalid/monitor-macos.idplugin',
          sha256: 'a'.repeat(64),
          future_artifact: { retained: true },
        },
        'x86_64-pc-windows-msvc': {
          url: 'https://packages.example.invalid/monitor-windows.idplugin',
          sha256: 'b'.repeat(64),
        },
      },
      future_metadata: { labels: ['home', 'status'] },
    },
  ],
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

describe('passive legacy control data', () => {
  it.each([false, true])(
    'preserves migration data, opaque fields and installed packages on core save (reset=%s)',
    async (reset) => {
      const form = useConfigForm()
      await form.loadConfig()
      form.config.mqtt_host = 'ChangedCerbo'
      if (reset) form.resetToDefaults()
      expect(await form.saveConfig()).toBe(true)
      const written = invoke.mock.calls.find(([command]) => command === 'save_config')?.[1].config
      expect(written.mqtt_host).toBe(reset ? 'Cerbo' : 'ChangedCerbo')
      expect(written.header_toggles_config).toEqual(saved.header_toggles_config)
      expect(written.ha_entities).toEqual(saved.ha_entities)
      expect(written.ha_url).toBe(saved.ha_url)
      expect(written.desktop_plugins).toEqual(saved.desktop_plugins)
      expect(written.modules).toEqual(saved.modules)
      expect(invoke.mock.calls.map(([command]) => command)).toEqual(['get_config', 'save_config'])
    }
  )
})

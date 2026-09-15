import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import Config from '../Config.vue'
import SetupWizard from '../components/SetupWizard.vue'
import { defaultConfig, type AppConfig } from '../config'

const boundary = vi.hoisted(() => ({ invoke: vi.fn(), emit: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('@tauri-apps/api/event', () => ({
  emit: boundary.emit,
  listen: vi.fn(async () => () => {}),
}))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
let wrapper: VueWrapper | undefined
let loaded: AppConfig
beforeEach(() => {
  loaded = {
    ...defaultConfig,
    mqtt_host: '',
    gateway_enabled: true,
    gateway_url: 'https://igw.example:9151',
    gateway_access_client_id: null,
    gateway_access_client_secret: null,
    gateway_api_token: 'read-token',
  }
  boundary.invoke.mockReset().mockImplementation(async (name: string) => {
    if (name === 'get_config') return { ...loaded }
    if (name === 'get_state') return {}
    if (name === 'test_gateway_connection') return { status: 'ok', mqtt_connected: true }
    return undefined
  })
  boundary.emit.mockReset().mockResolvedValue(undefined)
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
})

for (const [name, component, prefix] of [
  ['settings', Config, ''],
  ['setup wizard', SetupWizard, 'setup_'],
] as const) {
  describe(`${name} gateway configuration`, () => {
    async function openGateway() {
      wrapper = mount(component)
      await flushPromises()
      if (component === Config) {
        const gatewayTab = wrapper
          .findAll('button')
          .find((button) => button.text() === 'Remote Gateway')
        expect(gatewayTab).toBeDefined()
        await gatewayTab?.trigger('click')
      }
      return wrapper
    }

    async function testConnection(view: VueWrapper) {
      const test = view.findAll('button').find((button) => button.text() === 'Test connection')
      expect(test).toBeDefined()
      await test?.trigger('click')
      await flushPromises()
    }

    async function save(view: VueWrapper) {
      const button =
        component === Config
          ? view.get('button[title="Save changes"]')
          : view.findAll('button').find((candidate) => candidate.text() === 'Save & Continue')
      expect(button).toBeDefined()
      await button?.trigger('click')
      await flushPromises()
    }

    it('tests and saves native HTTPS with bearer-only authentication', async () => {
      const view = await openGateway()
      await view.get(`#${prefix}gateway_url`).setValue(' https://igw.example:9151 ')
      await view.get(`#${prefix}gateway_api_token`).setValue(' read-token ')
      await testConnection(view)
      expect(boundary.invoke).toHaveBeenCalledWith('test_gateway_connection', {
        url: 'https://igw.example:9151',
        accessClientId: '',
        accessClientSecret: '',
        apiToken: 'read-token',
      })
      expect(view.text()).toContain('OK (ok)')
      await save(view)
      expect(boundary.invoke).toHaveBeenCalledWith(
        'save_config',
        expect.objectContaining({
          config: expect.objectContaining({ gateway_enabled: true }),
        })
      )
      if (component === SetupWizard) expect(view.emitted('complete')).toHaveLength(1)
    })

    it('retains paired Cloudflare Access without imposing a bearer requirement', async () => {
      loaded.gateway_access_client_id = 'client-id'
      loaded.gateway_access_client_secret = 'client-secret'
      loaded.gateway_api_token = null
      const view = await openGateway()
      await testConnection(view)
      expect(boundary.invoke).toHaveBeenCalledWith('test_gateway_connection', {
        url: loaded.gateway_url,
        accessClientId: 'client-id',
        accessClientSecret: 'client-secret',
        apiToken: null,
      })
      await save(view)
      expect(boundary.invoke).toHaveBeenCalledWith('save_config', expect.anything())
    })

    it.each(['gateway_access_client_id', 'gateway_access_client_secret'])(
      'blocks an incomplete pair before testing or saving (%s)',
      async (field) => {
        const view = await openGateway()
        await view.get(`#${prefix}${field}`).setValue('fixture-credential')
        await testConnection(view)
        expect(boundary.invoke).not.toHaveBeenCalledWith(
          'test_gateway_connection',
          expect.anything()
        )
        expect(view.text()).toContain('Provide both Cloudflare Access fields or leave both blank')
        await save(view)
        expect(boundary.invoke).not.toHaveBeenCalledWith('save_config', expect.anything())
        expect(view.emitted('complete')).toBeUndefined()
      }
    )
  })
}

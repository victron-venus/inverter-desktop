import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { defaultConfig } from '../config'
import { newDraft, rateGrid, validatePlan, type RateGrid } from '../tariffs/model'
import { loadTariff, saveTariff } from '../tariffs/storage'
import { LOCAL_TARIFF_EVENT, loadLocalPreference } from '../tariffs/localPreference'
import TariffCost from '../tariffs/TariffCost.vue'
import TariffEditor from '../tariffs/TariffEditor.vue'
import IntervalEnergy from '../tariffs/IntervalEnergy.vue'
import TariffConfiguration from './TariffConfiguration.vue'

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), emit: vi.fn() }))
const sheet = vi.hoisted(() => ({ rates: null as RateGrid | null }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke, isTauri: () => true }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen, emit: native.emit }))
vi.mock('../tariffs/TariffSheet.vue', () => ({
  default: defineComponent({
    props: ['rates'],
    setup(props, { expose }) {
      expose({ getRates: async () => sheet.rates ?? props.rates })
      return () => h('div', 'Tariff schedule')
    },
  }),
}))
type EventCallback = (event: { payload: unknown }) => unknown
let callbacks: Map<string, Set<EventCallback>>
let savedConfig = { ...defaultConfig, portal_id: 'site-a' }
const tariff = (rate: number) =>
  validatePlan({ ...newDraft(), name: `Rate ${rate}`, rates: rateGrid(rate) })
let controllerPlan = tariff(0.2)
let status: { writable: boolean; revision: string; request_id?: string; error?: string } = {
  writable: true,
  revision: 'revision-a',
}
let action: { payload: { request_id: string; revision: string; plan: unknown } } | undefined
function send(name: string, payload: unknown) {
  for (const callback of callbacks.get(name) ?? []) callback({ payload })
}
async function click(wrapper: ReturnType<typeof mount>, label: string) {
  const button = wrapper.findAll('button').find((candidate) => candidate.text() === label)
  if (!button) throw new Error(`Missing button: ${label}`)
  await button.trigger('click')
  await flushPromises()
  await vi.dynamicImportSettled()
  await flushPromises()
}
beforeEach(() => {
  vi.useFakeTimers()
  const data = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
  })
  HTMLDialogElement.prototype.showModal = vi.fn()
  HTMLDialogElement.prototype.close = vi.fn()
  callbacks = new Map()
  savedConfig = { ...defaultConfig, portal_id: 'site-a' }
  controllerPlan = tariff(0.2)
  status = { writable: true, revision: 'revision-a' }
  action = undefined
  sheet.rates = null
  native.listen.mockReset().mockImplementation(async (name: string, callback: EventCallback) => {
    const set = callbacks.get(name) ?? new Set()
    set.add(callback)
    callbacks.set(name, set)
    return () => set.delete(callback)
  })
  native.emit.mockReset().mockImplementation(async (name, payload) => send(name, payload))
  native.invoke.mockReset().mockImplementation(async (name, args) => {
    if (name === 'get_config') return structuredClone(savedConfig)
    if (name === 'get_state')
      return {
        ui_config: { electricity_tariff: controllerPlan, electricity_tariff_status: { ...status } },
      }
    if (name === 'perform_action') action = args
  })
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})
it('edits local prices and opens interval history only in settings, syncing the dashboard without a controller write', async () => {
  saveTariff('site-a', tariff(0.3))
  const dashboard = mount(TariffCost, {
    props: { tariffScope: 'site-a', configuredTariff: controllerPlan, kwh: 10 },
  })
  const wrapper = mount(TariffConfiguration)
  await flushPromises()
  expect(wrapper.text()).toContain('Edit controller tariff')
  expect(dashboard.text()).toContain('2.00')
  await click(wrapper, 'Use a local tariff on this device')
  expect(loadLocalPreference('site-a').mode).toBe('local')
  expect(wrapper.text()).toContain('only on this device')
  expect(dashboard.text()).toContain('3.00')
  await click(wrapper, 'Edit local tariff')
  const localEditor = wrapper.getComponent(TariffEditor)
  expect(localEditor.props('tariffScope')).toBe('site-a')
  expect(localEditor.text()).toContain('Saved for this dashboard on this device.')
  expect(localEditor.text()).not.toContain('controller')
  expect(localEditor.get('.tariff-save').text()).toBe('Save tariff')
  sheet.rates = rateGrid(0.6)
  await wrapper.get('.tariff-save').trigger('click')
  await flushPromises()
  expect(wrapper.findComponent(TariffEditor).exists()).toBe(false)
  expect(loadTariff('site-a').plan?.rates).toEqual(rateGrid(0.6))
  expect(dashboard.text()).toContain('6.00')
  expect(action).toBeUndefined()
  expect(native.emit).toHaveBeenCalledWith(LOCAL_TARIFF_EVENT, { scope: 'site-a' })
  await click(wrapper, 'Interval energy cost')
  expect(wrapper.getComponent(IntervalEnergy).props('tariffScope')).toBe('site-a')
  expect(wrapper.getComponent(IntervalEnergy).props('plan').rates).toEqual(rateGrid(0.6))
  expect(dashboard.find('button').exists()).toBe(false)
  wrapper.unmount()
  dashboard.unmount()
})
it('keeps the captured controller revision and waits for its acknowledgement before closing the editor', async () => {
  const wrapper = mount(TariffConfiguration)
  await flushPromises()
  await click(wrapper, 'Edit controller tariff')
  expect(wrapper.getComponent(TariffEditor).text()).toContain('Saved on the controller')
  expect(wrapper.get('.tariff-save').text()).toBe('Save to controller')
  sheet.rates = rateGrid(0.4)
  status.revision = 'revision-b'
  await vi.advanceTimersByTimeAsync(2000)
  await wrapper.get('.tariff-save').trigger('click')
  await flushPromises()
  expect(action?.payload.revision).toBe('revision-a')
  expect(wrapper.findComponent(TariffEditor).exists()).toBe(true)
  expect(loadTariff('site-a').plan).toBeNull()
  controllerPlan = tariff(0.4)
  if (!action) throw new Error('Expected a controller tariff write')
  status.request_id = action.payload.request_id
  await vi.advanceTimersByTimeAsync(200)
  await flushPromises()
  expect(wrapper.findComponent(TariffEditor).exists()).toBe(false)
  expect(wrapper.text()).toContain('Rate 0.4')
  expect(native.emit).not.toHaveBeenCalledWith(LOCAL_TARIFF_EVENT, expect.anything())
  wrapper.unmount()
})
it('uses the saved installation, closes stale dialogs on a profile change, and preserves each installation preference', async () => {
  saveTariff('site-a', tariff(0.3))
  let wrapper = mount(TariffConfiguration)
  await flushPromises()
  await click(wrapper, 'Use a local tariff on this device')
  await click(wrapper, 'Edit local tariff')
  const staleSave = wrapper.getComponent(TariffEditor).props('savePlan')
  if (!staleSave) throw new Error('Expected a tariff save callback')
  savedConfig.portal_id = 'site-b'
  send('config-saved', {})
  await flushPromises()
  expect(wrapper.findComponent(TariffEditor).exists()).toBe(false)
  expect(wrapper.text()).toContain('Edit controller tariff')
  await expect(staleSave(tariff(0.8))).rejects.toThrow('installation changed')
  expect(loadTariff('site-b').plan).toBeNull()
  expect(loadTariff('site-a').plan?.rates).toEqual(rateGrid(0.3))
  await click(wrapper, 'Use a local tariff on this device')
  expect(wrapper.text()).toContain('No local tariff is configured')
  expect(loadLocalPreference('site-a').mode).toBe('local')
  expect(loadLocalPreference('site-b').mode).toBe('local')
  await click(wrapper, 'Use controller tariff')
  expect(loadLocalPreference('site-b').mode).toBe('controller')
  wrapper.unmount()
  savedConfig.portal_id = 'site-a'
  wrapper = mount(TariffConfiguration)
  await flushPromises()
  expect(wrapper.text()).toContain('Edit local tariff')
  expect(wrapper.text()).toContain('Rate 0.3')
  expect(action).toBeUndefined()
  wrapper.unmount()
})
it('disables actions until the saved profile is known and refuses controller edits without write support', async () => {
  status.writable = false
  const wrapper = mount(TariffConfiguration)
  expect(
    wrapper.findAll('button').every((button) => button.attributes('disabled') !== undefined)
  ).toBe(true)
  await flushPromises()
  const edit = wrapper
    .findAll('button')
    .find((button) => button.text() === 'Edit controller tariff')
  if (!edit) throw new Error('Expected the controller tariff editor button')
  expect(edit.attributes('disabled')).toBeDefined()
  await click(wrapper, 'Use a local tariff on this device')
  await click(wrapper, 'Set local tariff')
  expect(wrapper.getComponent(TariffEditor).props('persist')).toBe(false)
  wrapper.unmount()
})

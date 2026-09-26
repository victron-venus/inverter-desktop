import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { newDraft, rateGrid, validatePlan } from './model'
import { loadTariff, saveTariff, tariffKey } from './storage'
import {
  LOCAL_TARIFF_EVENT,
  loadLocalPreference,
  saveLocalPreference,
  tariffModeKey,
} from './localPreference'
import TariffCost from './TariffCost.vue'
import { tariffScope } from './scope'

const native = vi.hoisted(() => ({ emit: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true }))
vi.mock('@tauri-apps/api/event', () => ({ emit: native.emit, listen: native.listen }))
let data: Map<string, string>
let receive: (event: { payload: unknown }) => void
const stop = vi.fn()
const tariff = (rate: number) => validatePlan({ ...newDraft(), rates: rateGrid(rate) })
beforeEach(() => {
  data = new Map()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: vi.fn((key: string, value: string) => data.set(key, value)),
    removeItem: vi.fn((key: string) => data.delete(key)),
  })
  stop.mockReset()
  native.emit.mockReset().mockResolvedValue(undefined)
  native.listen.mockReset().mockImplementation(async (_, callback) => {
    receive = callback
    return stop
  })
})
afterEach(() => vi.unstubAllGlobals())
it('reloads persisted preferences on native invalidation without relying on browser storage events', async () => {
  const wrapper = mount(TariffCost, {
    props: { tariffScope: 'site', kwh: 10, configuredTariff: tariff(0.2) },
  })
  await flushPromises()
  saveTariff('site', tariff(0.5))
  receive({ payload: { scope: 'site' } })
  await flushPromises()
  expect(wrapper.text()).toContain('2.00')
  data.set(tariffModeKey('site'), 'local')
  receive({ payload: { scope: 'site' } })
  await flushPromises()
  expect(wrapper.text()).toContain('5.00')
  const writes = vi.mocked(localStorage.setItem).mock.calls.length
  receive({ payload: { scope: 'other' } })
  receive({ payload: { scope: null } })
  await flushPromises()
  expect(wrapper.text()).toContain('5.00')
  expect(localStorage.setItem).toHaveBeenCalledTimes(writes)
  expect(native.emit).not.toHaveBeenCalled()
  wrapper.unmount()
  expect(stop).toHaveBeenCalledOnce()
})
it('never replays stale mode or plan payloads over newer saves or removals', async () => {
  const wrapper = mount(TariffCost, {
    props: { tariffScope: 'site', kwh: 10, configuredTariff: tariff(0.2) },
  })
  await flushPromises()
  await saveLocalPreference({ scope: 'site', kind: 'plan', value: tariff(0.3) })
  await saveLocalPreference({ scope: 'site', kind: 'mode', value: 'local' })
  const stalePlan = { scope: 'site', kind: 'plan', value: tariff(0.3) }
  const staleMode = { scope: 'site', kind: 'mode', value: 'local' }
  await saveLocalPreference({ scope: 'site', kind: 'plan', value: tariff(0.8) })
  await saveLocalPreference({ scope: 'site', kind: 'mode', value: 'controller' })
  const writes = vi.mocked(localStorage.setItem).mock.calls.length
  receive({ payload: staleMode })
  receive({ payload: stalePlan })
  await flushPromises()
  expect(loadLocalPreference('site').mode).toBe('controller')
  expect(loadTariff('site').plan).toEqual(tariff(0.8))
  expect(wrapper.text()).toContain('2.00')
  await saveLocalPreference({ scope: 'site', kind: 'plan', value: null })
  receive({ payload: stalePlan })
  await flushPromises()
  expect(loadTariff('site').plan).toBeNull()
  expect(localStorage.setItem).toHaveBeenCalledTimes(writes)
  wrapper.unmount()
})
it('persists changes before broadcasting only their scope, including local plan removal', async () => {
  await saveLocalPreference({ scope: 'site', kind: 'plan', value: tariff(0.3) })
  await saveLocalPreference({ scope: 'site', kind: 'mode', value: 'local' })
  expect(loadLocalPreference('site').plan).toEqual(tariff(0.3))
  expect(native.emit).toHaveBeenLastCalledWith(LOCAL_TARIFF_EVENT, {
    scope: 'site',
  })
  await saveLocalPreference({ scope: 'site', kind: 'plan', value: null })
  expect(native.emit).toHaveBeenLastCalledWith(LOCAL_TARIFF_EVENT, {
    scope: 'site',
  })
  expect(data.has(tariffKey('site'))).toBe(false)
  expect(loadLocalPreference('site').mode).toBe('local')
})
it('retains saved preferences on storage failure and reports native synchronization failure accurately', async () => {
  saveTariff('site', tariff(0.3))
  data.set(tariffModeKey('site'), 'local')
  vi.mocked(localStorage.setItem).mockImplementationOnce(() => {
    throw new Error('quota')
  })
  await expect(
    saveLocalPreference({ scope: 'site', kind: 'mode', value: 'controller' })
  ).rejects.toThrow('could not be saved')
  expect(loadLocalPreference('site').mode).toBe('local')
  expect(native.emit).not.toHaveBeenCalled()
  native.emit.mockRejectedValueOnce(new Error('unavailable'))
  await expect(
    saveLocalPreference({ scope: 'site', kind: 'mode', value: 'controller' })
  ).rejects.toThrow('Reopen those windows')
  expect(loadLocalPreference('site').mode).toBe('controller')
  expect(loadTariff('site').plan).toEqual(tariff(0.3))
})
it('cleans up a native listener that finishes registering after the component was closed', async () => {
  let resolve!: (value: () => void) => void
  native.listen.mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done
      })
  )
  const wrapper = mount(TariffCost, { props: { tariffScope: 'site' } })
  wrapper.unmount()
  resolve(stop)
  await flushPromises()
  expect(stop).toHaveBeenCalledOnce()
})
it('preserves the established installation keys shared by tariffs and interval history', () => {
  const config = { portal_id: 'portal', gateway_url: 'https://gateway', mqtt_host: 'cerbo' }
  expect(tariffScope(config)).toBe('portal')
  expect(tariffScope({ ...config, portal_id: null })).toBe('https://gateway')
  expect(tariffScope({ mqtt_host: 'cerbo' })).toBe('cerbo')
  expect(tariffScope(null)).toBe('dashboard')
})

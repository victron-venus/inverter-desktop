import { mount, flushPromises } from '@vue/test-utils'
import { beforeEach, afterEach, expect, it, vi } from 'vitest'
import { newDraft, rateGrid, validatePlan } from './model'
import { clearTariff, saveTariff, tariffKey } from './storage'
import TariffCost from './TariffCost.vue'

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-09-24T20:00:00Z'))
  const data = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
  })
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})
const tariff = (rate: number) =>
  validatePlan({ ...newDraft(), rates: rateGrid(rate), billingDay: 17 })
it('uses a late installation tariff, updates it, and keeps a scoped local override authoritative', async () => {
  const wrapper = mount(TariffCost, { props: { tariffScope: 'site', kwh: 10 } })
  expect(wrapper.text()).not.toContain('Billing period')
  await wrapper.setProps({ configuredTariff: tariff(0.2) })
  expect(wrapper.text()).toContain('Installation tariff')
  expect(wrapper.text()).toContain('2026-09-17 – 2026-10-16')
  expect(wrapper.text()).toContain('2.00')
  await wrapper.setProps({ configuredTariff: tariff(0.4) })
  expect(wrapper.text()).toContain('4.00')
  saveTariff('site', tariff(0.3))
  window.dispatchEvent(new StorageEvent('storage', { key: tariffKey('site') }))
  await flushPromises()
  expect(wrapper.text()).toContain('Local tariff')
  expect(wrapper.text()).toContain('3.00')
  await wrapper.setProps({ configuredTariff: tariff(0.5) })
  expect(wrapper.text()).toContain('3.00')
  clearTariff('site')
  window.dispatchEvent(new StorageEvent('storage', { key: tariffKey('site') }))
  await flushPromises()
  expect(wrapper.text()).toContain('5.00')
  await wrapper.setProps({ tariffScope: 'other-site', configuredTariff: undefined })
  expect(wrapper.text()).not.toContain('Billing period')
  wrapper.unmount()
})
it('rejects malformed installation data and allows a valid local override', async () => {
  const wrapper = mount(TariffCost, {
    props: { tariffScope: 'site', configuredTariff: {}, readOnly: true },
  })
  expect(wrapper.get('[role="alert"]').text()).toContain('installation tariff is invalid')
  expect(wrapper.find('button').exists()).toBe(false)
  saveTariff('site', tariff(0.2))
  window.dispatchEvent(new StorageEvent('storage', { key: tariffKey('site') }))
  await flushPromises()
  expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  wrapper.unmount()
})

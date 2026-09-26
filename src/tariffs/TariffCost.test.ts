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
it('uses the controller tariff by default and requires an explicit local choice', async () => {
  const wrapper = mount(TariffCost, { props: { tariffScope: 'site', kwh: 10 } })
  expect(wrapper.text()).not.toContain('Billing period')
  await wrapper.setProps({ configuredTariff: tariff(0.2) })
  expect(wrapper.text()).toContain('Controller tariff')
  expect(wrapper.text()).toContain('2026-09-17 – 2026-10-16')
  expect(wrapper.text()).toContain('2.00')
  await wrapper.setProps({ configuredTariff: tariff(0.4) })
  expect(wrapper.text()).toContain('4.00')
  saveTariff('site', tariff(0.3))
  window.dispatchEvent(new StorageEvent('storage', { key: tariffKey('site') }))
  await flushPromises()
  expect(wrapper.text()).toContain('4.00')
  await wrapper
    .findAll('button')
    .find((button) => button.text().includes('Use a local tariff'))!
    .trigger('click')
  expect(wrapper.text()).toContain('Local tariff')
  expect(wrapper.text()).toContain('3.00')
  await wrapper.setProps({ configuredTariff: tariff(0.5) })
  expect(wrapper.text()).toContain('3.00')
  clearTariff('site')
  window.dispatchEvent(new StorageEvent('storage', { key: tariffKey('site') }))
  await flushPromises()
  await wrapper
    .findAll('button')
    .find((button) => button.text().includes('Use controller tariff'))!
    .trigger('click')
  expect(wrapper.text()).toContain('5.00')
  await wrapper.setProps({ tariffScope: 'other-site', configuredTariff: undefined })
  expect(wrapper.text()).not.toContain('Billing period')
  wrapper.unmount()
})
it('keeps malformed controller data visible and ignores local overrides in read-only mode', async () => {
  const wrapper = mount(TariffCost, {
    props: { tariffScope: 'site', configuredTariff: {}, readOnly: true },
  })
  expect(wrapper.get('[role="alert"]').text()).toContain('controller tariff is invalid')
  expect(wrapper.find('button').exists()).toBe(false)
  saveTariff('site', tariff(0.2))
  window.dispatchEvent(new StorageEvent('storage', { key: tariffKey('site') }))
  await flushPromises()
  expect(wrapper.find('[role="alert"]').exists()).toBe(true)
  wrapper.unmount()
})

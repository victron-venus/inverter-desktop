import { mount, flushPromises } from '@vue/test-utils'
import { beforeEach, afterEach, expect, it, vi } from 'vitest'
import { newDraft, rateGrid, validatePlan } from './model'
import { saveTariff } from './storage'
import { saveLocalPreference } from './localPreference'
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
it('shows compact controller cost and rate with details only in the tooltip and no controls', async () => {
  saveTariff('site', tariff(0.3))
  const wrapper = mount(TariffCost, {
    props: { tariffScope: 'site', kwh: 10, configuredTariff: tariff(0.2) },
  })
  expect(wrapper.text()).toBe('≈ $2.00 ·0.2000 USD/kWh')
  expect(wrapper.find('button').exists()).toBe(false)
  expect(wrapper.find('dialog').exists()).toBe(false)
  expect(wrapper.get('.tariff-cost').attributes('aria-label')).toContain('Controller tariff')
  expect(wrapper.get('.tariff-cost').attributes('aria-label')).toContain('2026-09-17 – 2026-10-16')
  await wrapper.setProps({ configuredTariff: tariff(0.4) })
  expect(wrapper.text()).toContain('4.00')
  wrapper.unmount()
})
it('honors explicit persisted local mode across remounts and isolates installation preferences', async () => {
  const props = { tariffScope: 'site', kwh: 10, configuredTariff: tariff(0.2) }
  saveTariff('site', tariff(0.3))
  let wrapper = mount(TariffCost, { props })
  await saveLocalPreference({ scope: 'site', kind: 'mode', value: 'local' })
  await flushPromises()
  expect(wrapper.text()).toContain('3.00')
  expect(wrapper.get('.tariff-cost').attributes('aria-label')).toContain('this device only')
  wrapper.unmount()
  wrapper = mount(TariffCost, { props })
  expect(wrapper.text()).toContain('3.00')
  await wrapper.setProps({ configuredTariff: tariff(0.5) })
  expect(wrapper.text()).toContain('3.00')
  await wrapper.setProps({ tariffScope: 'other-site' })
  expect(wrapper.text()).toContain('5.00')
  await wrapper.setProps({ tariffScope: 'site' })
  expect(wrapper.text()).toContain('3.00')
  await saveLocalPreference({ scope: 'site', kind: 'mode', value: 'controller' })
  await flushPromises()
  expect(wrapper.text()).toContain('5.00')
  wrapper.unmount()
})
it('never invents a time-of-use daily total and keeps malformed tariffs visible without editing', async () => {
  const plan = tariff(0.2)
  plan.rates[0][0] = 0.5
  const wrapper = mount(TariffCost, {
    props: { tariffScope: 'site', kwh: 10, configuredTariff: plan },
  })
  expect(wrapper.text()).toBe('0.2000 USD/kWh')
  expect(wrapper.get('.tariff-cost').attributes('aria-label')).toContain('interval consumption')
  await wrapper.setProps({ configuredTariff: {} })
  expect(wrapper.get('[role="alert"]').text()).toBe('Tariff unavailable')
  expect(wrapper.get('.tariff-cost').attributes('aria-label')).toContain(
    'controller tariff is invalid'
  )
  expect(wrapper.find('button').exists()).toBe(false)
  wrapper.unmount()
})

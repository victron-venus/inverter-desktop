import { mount, flushPromises } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import IntervalEnergy from './IntervalEnergy.vue'
import { intervalKey } from './intervals'
import { newDraft, rateGrid, validatePlan } from './model'
const csv = 'start,end,import_kwh\n2026-09-24T00:00:00-07:00,2026-09-24T00:30:00-07:00,2'
const tariff = validatePlan({
  ...newDraft(),
  timeZone: 'America/Los_Angeles',
  rates: rateGrid(0.5),
  billingDay: 17,
})
let values: Map<string, string>
beforeEach(() => {
  values = new Map()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  })
  HTMLDialogElement.prototype.showModal = vi.fn()
})
afterEach(() => vi.unstubAllGlobals())
async function upload(wrapper: ReturnType<typeof mount>, text: string) {
  const input = wrapper.get('input[type="file"]')
  Object.defineProperty(input.element, 'files', {
    configurable: true,
    value: [{ size: text.length, text: async () => text }],
  })
  await input.trigger('change')
  await flushPromises()
}
it('imports CSV through the UI, shows measured subtotal and gaps, persists and isolates sites', async () => {
  const wrapper = mount(IntervalEnergy, {
    props: { plan: tariff, tariffScope: 'site' },
  })
  await upload(wrapper, csv)
  expect(wrapper.text()).toContain('1.00')
  expect(wrapper.text()).toContain('2.000 kWh priced')
  expect(wrapper.text()).toContain('Partial energy charge')
  expect(wrapper.text()).toContain('Missing consumption is not zero')
  expect(values.has(intervalKey('site'))).toBe(true)
  expect((wrapper.get('input[type="date"]').element as HTMLInputElement).value).toBe('2026-09-17')
  await upload(wrapper, csv.replace('import_kwh', 'net_kwh'))
  expect(wrapper.get('[role="alert"]').text()).toContain('CSV header')
  expect(wrapper.text()).toContain('2.000 kWh priced')
  await wrapper.setProps({ tariffScope: 'other' })
  expect(wrapper.text()).not.toContain('2.000 kWh priced')
  await wrapper.setProps({ tariffScope: 'site' })
  expect(wrapper.text()).toContain('2.000 kWh priced')
  const remove = wrapper
    .findAll('button')
    .find((button) => button.text() === 'Remove saved intervals')
  await remove?.trigger('click')
  expect(values.has(intervalKey('site'))).toBe(false)
  wrapper.unmount()
})
it('preserves saved readings on quota failure and does not save an import after a scope switch', async () => {
  const wrapper = mount(IntervalEnergy, {
    props: { plan: tariff, tariffScope: 'site' },
  })
  await upload(wrapper, csv)
  const original = values.get(intervalKey('site'))
  vi.spyOn(localStorage, 'setItem').mockImplementation(() => {
    throw new Error('quota')
  })
  await upload(wrapper, csv.replace(/,2$/, ',3'))
  expect(wrapper.get('[role="alert"]').text()).toContain('previous history is unchanged')
  expect(values.get(intervalKey('site'))).toBe(original)
  vi.restoreAllMocks()
  let resolve: (text: string) => void = () => {}
  const delayed = new Promise<string>((done) => {
    resolve = done
  })
  const input = wrapper.get('input[type="file"]')
  Object.defineProperty(input.element, 'files', {
    configurable: true,
    value: [{ size: csv.length, text: () => delayed }],
  })
  await input.trigger('change')
  await wrapper.setProps({ tariffScope: 'other' })
  resolve(csv)
  await flushPromises()
  expect(values.has(intervalKey('other'))).toBe(false)
  wrapper.unmount()
})

it('retains a user-selected range when telemetry repeats a tariff or a price is edited', async () => {
  const wrapper = mount(IntervalEnergy, {
    props: { plan: tariff, tariffScope: 'site' },
  })
  await upload(wrapper, csv)
  await wrapper.get('input[type="date"]').setValue('2026-08-17')
  await wrapper.setProps({ plan: JSON.parse(JSON.stringify(tariff)) })
  expect((wrapper.get('input[type="date"]').element as HTMLInputElement).value).toBe('2026-08-17')
  await wrapper.setProps({
    plan: { ...tariff, rates: rateGrid(0.8) as number[][] },
  })
  expect((wrapper.get('input[type="date"]').element as HTMLInputElement).value).toBe('2026-08-17')
  expect(wrapper.text()).toContain('1.60')
  wrapper.unmount()
})

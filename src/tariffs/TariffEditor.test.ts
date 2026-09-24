import { mount, flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { newDraft, rateGrid, validatePlan, type RateGrid } from './model'
import { loadTariff } from './storage'
import TariffEditor from './TariffEditor.vue'

const sheetState = vi.hoisted(() => ({ edits: null as RateGrid | null, fail: false }))
vi.mock('./TariffSheet.vue', () => ({
  default: defineComponent({
    props: ['rates'],
    setup(props, { expose }) {
      expose({
        getRates: async () => {
          if (sheetState.fail) throw new Error('Finish editing the cell.')
          return sheetState.edits ?? props.rates
        },
      })
      return () => h('div', { 'data-testid': 'sheet' }, String(props.rates[0][0]))
    },
  }),
}))
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-09-24T20:00:00Z'))
  HTMLDialogElement.prototype.showModal = vi.fn()
  HTMLDialogElement.prototype.close = vi.fn()
  sheetState.edits = null
  sheetState.fail = false
  const data = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: vi.fn((key: string, value: string) => data.set(key, value)),
  })
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})
const tariff = () =>
  validatePlan({
    ...newDraft(),
    timeZone: 'America/Los_Angeles',
    billingDay: 17,
    rates: rateGrid(0.3),
    seasons: [{ name: 'Summer', months: [6, 7, 8, 9], rates: rateGrid(0.5) }],
  })
it('commits pending cells before switching season and preserves both schedules on save', async () => {
  const wrapper = mount(TariffEditor, { props: { plan: tariff(), tariffScope: 'editor-site' } })
  expect(wrapper.get('select').element.value).toBe('0')
  sheetState.edits = rateGrid(0.6)
  await wrapper.get('select').setValue('-1')
  await flushPromises()
  expect(wrapper.get('[data-testid="sheet"]').text()).toBe('0.3')
  sheetState.edits = rateGrid(0.4)
  await wrapper.get('.tariff-save').trigger('click')
  await flushPromises()
  const saved = loadTariff('editor-site').plan
  expect(saved?.seasons[0].rates).toEqual(rateGrid(0.6))
  expect(saved?.rates).toEqual(rateGrid(0.4))
  expect(saved?.billingDay).toBe(17)
  expect(wrapper.emitted('saved')?.[0]).toEqual([saved])
  wrapper.unmount()
})
it('keeps the selected schedule when a cell cannot be committed', async () => {
  const wrapper = mount(TariffEditor, { props: { plan: tariff(), tariffScope: 'editor-site' } })
  sheetState.fail = true
  await wrapper.get('select').setValue('-1')
  await flushPromises()
  expect(wrapper.get('select').element.value).toBe('0')
  expect(wrapper.get('[role="alert"]').text()).toContain('Finish editing')
  expect(localStorage.setItem).not.toHaveBeenCalled()
  wrapper.unmount()
})
it('rejects a fractional billing day and allows clearing a previously configured date', async () => {
  const wrapper = mount(TariffEditor, { props: { plan: tariff(), tariffScope: 'editor-site' } })
  await wrapper.get('input[min="1"]').setValue('17.5')
  await wrapper.get('.tariff-save').trigger('click')
  await flushPromises()
  expect(wrapper.get('[role="alert"]').text()).toContain('whole number')
  expect(localStorage.setItem).not.toHaveBeenCalled()
  await wrapper.get('input[min="1"]').setValue('')
  await wrapper.get('.tariff-save').trigger('click')
  await flushPromises()
  expect(loadTariff('editor-site').plan?.billingDay).toBeUndefined()
  wrapper.unmount()
})

it('creates a manual season and applies a configuration draft without writing local storage', async () => {
  const base = validatePlan({
    ...newDraft(),
    rates: rateGrid(0.3),
    timeZone: 'America/Los_Angeles',
  })
  const wrapper = mount(TariffEditor, { props: { plan: base, persist: false } })
  const add = wrapper.findAll('button').find((button) => button.text() === 'Add season')!
  await add.trigger('click')
  await flushPromises()
  await wrapper.get('input[maxlength="80"]').setValue('Summer')
  await wrapper.get('input[type="checkbox"][value="6"]').setValue(true)
  sheetState.edits = rateGrid(0.5)
  await wrapper.get('.tariff-save').trigger('click')
  await flushPromises()
  expect(wrapper.emitted('saved')?.[0]?.[0]).toMatchObject({
    seasons: [{ name: 'Summer', months: [6], rates: rateGrid(0.5) }],
    rates: rateGrid(0.3),
  })
  expect(localStorage.setItem).not.toHaveBeenCalled()
  wrapper.unmount()
})

import { mount, flushPromises } from '@vue/test-utils'
import { expect, it, vi } from 'vitest'
import { configurationTariff, setConfigurationTariff, TARIFF_MODULE } from '../configurationTariff'
import { defaultConfig } from '../config'
import { newDraft, rateGrid, validatePlan } from '../tariffs/model'
import { useConfigForm } from '../composables/useConfigForm'
import SetupWizard from '../components/SetupWizard.vue'
import TariffConfiguration from '../components/TariffConfiguration.vue'

const native = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
const plan = () => validatePlan({ ...newDraft(), rates: rateGrid(0.3), billingDay: 17 })

it('keeps a configured tariff and unrelated module fields through load, reset and save', async () => {
  const config = {
    ...defaultConfig,
    modules: { other: { schema_version: 3, values: { keep: 42 } } },
  }
  setConfigurationTariff(config, plan())
  native.invoke.mockImplementation(async (name) =>
    name === 'get_config' ? structuredClone(config) : undefined
  )
  const form = useConfigForm()
  await form.loadConfig()
  form.resetToDefaults()
  expect(configurationTariff(form.config)).toEqual(plan())
  expect(await form.saveConfig([], [])).toBe(true)
  const saved = native.invoke.mock.calls.find(([name]) => name === 'save_config')?.[1].config
  expect(saved.modules.other.values.keep).toBe(42)
  expect(saved.modules[TARIFF_MODULE].values.plan.billingDay).toBe(17)
  setConfigurationTariff(form.config, null)
  expect(configurationTariff(form.config)).toBeNull()
  expect(form.config.modules?.other.values.keep).toBe(42)
})
it('stores the first-run tariff with the connection configuration', async () => {
  native.invoke
    .mockReset()
    .mockImplementation(async (name) => (name === 'get_config' ? { ...defaultConfig } : undefined))
  const wrapper = mount(SetupWizard)
  await flushPromises()
  wrapper.getComponent(TariffConfiguration).vm.$emit('update:modelValue', plan())
  await flushPromises()
  const save = wrapper
    .findAll('button')
    .find((button) => button.text().includes('Save & Continue'))!
  await save.trigger('click')
  await flushPromises()
  const saved = native.invoke.mock.calls.find(([name]) => name === 'save_config')?.[1].config
  expect(saved.modules[TARIFF_MODULE].values.plan).toEqual(plan())
  expect(saved.setup_completed).toBe(true)
  wrapper.unmount()
})
it('keeps unsupported module data visible as invalid rather than silently applying it', () => {
  const config = {
    ...defaultConfig,
    modules: { [TARIFF_MODULE]: { schema_version: 7, values: { plan: plan() } } },
  }
  const wrapper = mount(TariffConfiguration, { props: { modelValue: configurationTariff(config) } })
  expect(wrapper.get('[role="alert"]').text()).toContain('invalid or newer')
  expect(config.modules[TARIFF_MODULE].schema_version).toBe(7)
  wrapper.unmount()
})

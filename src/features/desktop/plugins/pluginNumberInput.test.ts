import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createI18n } from 'vue-i18n'
import en from '../messages.en'
import ru from '../messages.ru'
import PluginPanels from './PluginPanels.vue'
import type { ActionContribution, NumberInputContribution, PluginSnapshot } from './types'
import { createPluginDashboard } from './usePluginDashboard'

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }))

const input: NumberInputContribution = {
  kind: 'number_input',
  id: 'room-number',
  title: 'Room target',
  action_id: 'ha-number-0-set',
  label: 'Set room target',
  unit: '°C',
  input_revision: 'input-1',
  value_scaled: 25,
  min_scaled: -135,
  max_scaled: 165,
  step_scaled: 10,
  decimal_places: 2,
}
const fixed: ActionContribution = {
  kind: 'action',
  id: 'button',
  title: 'Doorbell',
  action_id: 'ha-action-0',
  label: 'Press',
  params: {},
}
const plugin: PluginSnapshot = {
  plugin_id: 'inverter-desktop.home-assistant',
  instance_id: 'worker-1',
  state: 'running',
  generation: 1,
  restart_count: 0,
  last_error: null,
  contributions: [input],
}

function deferred() {
  let resolve!: (value?: unknown) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<unknown>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, resolve, reject }
}

let current: PluginSnapshot[]
let response: Promise<unknown> | undefined
let wrapper: VueWrapper | undefined
const callbacks = new Map<string, () => void>()
const dashboards: Array<ReturnType<typeof createPluginDashboard>> = []

beforeEach(() => {
  current = [structuredClone(plugin)]
  response = undefined
  native.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'auth_status') return { unlocked: true }
    if (command === 'get_plugin_snapshot') return structuredClone(current)
    if (command === 'plugin_action') return response
  })
  native.listen.mockReset().mockImplementation(async (name: string, callback: () => void) => {
    callbacks.set(name, callback)
    return () => callbacks.delete(name)
  })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  for (const dashboard of dashboards) dashboard.stop()
  dashboards.length = 0
  callbacks.clear()
})

async function panel(locale = 'en') {
  wrapper = mount(PluginPanels, {
    global: { plugins: [createI18n({ legacy: false, locale, messages: { en, ru } })] },
  })
  await flushPromises()
  return wrapper
}

async function dashboard() {
  const result = createPluginDashboard()
  dashboards.push(result)
  await result.start()
  return result
}

async function publish(contributions: PluginSnapshot['contributions']) {
  current[0].contributions = contributions
  const callback = callbacks.get('plugin-host-update')
  if (!callback) throw new Error('dashboard listener is required')
  callback()
  await flushPromises()
}

function calls() {
  return native.invoke.mock.calls.filter(([command]) => command === 'plugin_action')
}

describe('desktop numeric editor', () => {
  it('preserves a dirty decimal draft across observations and applies only the explicit coefficient', async () => {
    const view = await panel()
    expect(view.find('input').attributes('aria-label')).toBe('Room target: Set room target')
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    await publish([{ ...input, value_scaled: 65 }])
    expect(view.find('input').element.value).toBe('0.65')
    await view.find('input').setValue('0.35')
    await publish([{ ...input, value_scaled: 45, title: 'Renamed room target' }])
    expect(view.find('input').element.value).toBe('0.35')
    expect(view.text()).toContain('Current value: 0.45 °C')
    expect(view.text()).toContain('From -1.35 to 1.65, in steps of 0.10 °C.')
    await view.find('form').trigger('submit')
    await flushPromises()
    expect(calls()).toEqual([
      [
        'plugin_action',
        {
          pluginId: plugin.plugin_id,
          instanceId: plugin.instance_id,
          actionId: input.action_id,
          params: { input_revision: input.input_revision, value_scaled: 35 },
        },
      ],
    ])
    expect(view.text()).toContain('Current value: 0.45 °C')
    expect(view.find('input').element.value).toBe('0.35')
  })

  it.each(['', '0.30', '0.351', '1e2', '-1.45'])(
    'blocks invalid or off-grid drafts %j',
    async (draft) => {
      const view = await panel()
      await view.find('input').setValue(draft)
      expect(view.find('input').attributes('aria-invalid')).toBe('true')
      expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
      expect(view.find('[role="alert"]').text()).toContain('Enter a value from -1.35 to 1.65')
      await view.find('form').trigger('submit')
      expect(calls()).toHaveLength(0)
    }
  )

  it.each([
    { input_revision: 'input-2' },
    { min_scaled: -125, max_scaled: 175 },
    { decimal_places: 3 },
    { unit: 'kW' },
  ])('requires explicit review when authority changes: %j', async (change) => {
    const view = await panel()
    await view.find('input').setValue('0.35')
    const replacement = { ...input, ...change }
    await publish([replacement])
    expect(view.find('input').element.value).toBe('0.35')
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    const feedback = view.find('output[id$="-review"]')
    expect(feedback.text()).toContain('available settings have changed')
    expect(view.find('input').attributes('aria-describedby')?.split(' ')).toContain(
      feedback.attributes('id')
    )
    expect(view.text()).not.toContain('input_revision')
    await view.find('form').trigger('submit')
    expect(calls()).toHaveLength(0)
    const review = view.findAll('button').find((button) => button.text() === 'Use latest settings')
    expect(review).toBeDefined()
    if (!review) throw new Error('Expected the numeric settings review button')
    await review.trigger('click')
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    const draft = replacement.decimal_places === 3 ? '0.045' : '0.45'
    await view.find('input').setValue(draft)
    await view.find('form').trigger('submit')
    await flushPromises()
    expect(calls()[0][1]).toMatchObject({
      params: { input_revision: replacement.input_revision, value_scaled: 45 },
    })
  })

  it('retains unknown-outcome feedback across capability changes, withdrawal and restoration', async () => {
    const pending = deferred()
    response = pending.promise
    const view = await panel()
    await view.find('input').setValue('0.35')
    await view.find('form').trigger('submit')
    await view.find('form').trigger('submit')
    expect(calls()).toHaveLength(1)
    await publish([{ ...input, input_revision: 'input-2', value_scaled: 45 }])
    expect(view.find('button[type="submit"]').attributes('aria-busy')).toBe('true')
    await publish([])
    expect(view.find('form').exists()).toBe(false)
    await publish([{ ...input, input_revision: 'input-3', value_scaled: 55 }])
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    expect(view.find('button[type="submit"]').attributes('aria-busy')).toBe('true')
    pending.reject(new Error('private service response'))
    await flushPromises()
    expect(view.find('[role="alert"]').text()).toContain(
      'It may have completed. Check the current state'
    )
    expect(view.text()).not.toContain('private service response')
    await publish([{ ...input, input_revision: 'input-4', value_scaled: 65, title: 'New name' }])
    expect(view.find('[role="alert"]').text()).toContain('It may have completed.')
    expect(calls()).toHaveLength(1)
  })

  it('keeps an unreviewed draft blocked even if earlier settings are advertised again', async () => {
    const view = await panel()
    await view.find('input').setValue('0.35')
    await publish([{ ...input, input_revision: 'changed-input' }])
    await publish([input])
    expect(view.find('input').element.value).toBe('0.35')
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    await view.find('form').trigger('submit')
    expect(calls()).toHaveLength(0)
    expect(view.find('output[id$="-review"]').text()).toContain('available settings have changed')
  })

  it('resets a draft when a new worker instance reuses the same contribution ID', async () => {
    const view = await panel()
    await view.find('input').setValue('0.35')
    current[0].instance_id = 'worker-2'
    await publish([{ ...input, value_scaled: 45 }])
    expect(view.find('input').element.value).toBe('0.45')
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    await view.find('form').trigger('submit')
    expect(calls()).toHaveLength(0)
  })

  it('localizes Apply, validation and observed value in Russian', async () => {
    const view = await panel('ru')
    expect(view.find('button[type="submit"]').text()).toBe('Применить')
    expect(view.text()).toContain('Текущее значение: 0.25 °C')
    await view.find('input').setValue('0.30')
    expect(view.find('[role="alert"]').text()).toContain('Введите значение от -1.35 до 1.65')
  })
})

describe('numeric dashboard authorization', () => {
  it('rejects stale instances, withdrawn inputs, changed grants and invalid coefficients before IPC', async () => {
    const value = await dashboard()
    await value.runNumberInput(plugin.plugin_id, null, input, 35)
    await value.runNumberInput(plugin.plugin_id, 'old-worker', input, 35)
    for (const coefficient of [30, -145, 175, 35.5, Number.NaN, Number.POSITIVE_INFINITY]) {
      await value.runNumberInput(plugin.plugin_id, plugin.instance_id, input, coefficient)
    }
    for (const replacement of [
      { ...input, input_revision: 'new-input' },
      { ...input, min_scaled: -125, max_scaled: 175 },
      { ...input, unit: 'kW' },
      { ...input, id: 'replacement-card' },
    ]) {
      current[0].contributions = [replacement]
      await value.refresh()
      await value.runNumberInput(plugin.plugin_id, plugin.instance_id, input, 35)
    }
    current[0].contributions = []
    await value.refresh()
    await value.runNumberInput(plugin.plugin_id, plugin.instance_id, input, 35)
    expect(calls()).toHaveLength(0)
    current[0].contributions = [
      { ...input, value_scaled: 45, title: 'Renamed', label: 'New label' },
    ]
    await value.refresh()
    await value.runNumberInput(plugin.plugin_id, plugin.instance_id, input, 35)
    expect(calls()[0][1]).toMatchObject({ params: { input_revision: 'input-1', value_scaled: 35 } })
    native.invoke.mockRejectedValueOnce(new Error('snapshot unavailable'))
    await value.refresh()
    await value.runNumberInput(plugin.plugin_id, plugin.instance_id, input, 45)
    expect(calls()).toHaveLength(1)
  })

  it('keeps one numeric operation pending across new values and grants while preserving fixed actions', async () => {
    current[0].contributions = [input, fixed]
    const value = await dashboard()
    const pending = deferred()
    response = pending.promise
    const first = value.runNumberInput(plugin.plugin_id, plugin.instance_id, input, 35)
    const replacement = { ...input, input_revision: 'new-input', value_scaled: 45 }
    current[0].contributions = [replacement, fixed]
    await value.refresh()
    await value.runNumberInput(plugin.plugin_id, plugin.instance_id, replacement, 55)
    await value.runNumberInput(plugin.plugin_id, plugin.instance_id, replacement, 65)
    expect(calls()).toHaveLength(1)
    expect(value.numberInputKey(plugin.plugin_id, plugin.instance_id, input)).toBe(
      value.numberInputKey(plugin.plugin_id, plugin.instance_id, replacement)
    )
    response = undefined
    await value.runAction(plugin.plugin_id, plugin.instance_id, {
      ...fixed,
      params: { value_scaled: 35 },
    })
    expect(calls()).toHaveLength(1)
    await value.runAction(plugin.plugin_id, plugin.instance_id, fixed)
    expect(calls()[1][1]).toMatchObject({ actionId: fixed.action_id, params: {} })
    pending.reject(new Error('unknown numeric outcome'))
    await first
    expect(
      value.failedActions.value.has(
        value.numberInputKey(plugin.plugin_id, plugin.instance_id, replacement)
      )
    ).toBe(true)
  })

  it('ignores an old numeric result after instance replacement without unlocking the new request', async () => {
    const value = await dashboard()
    const old = deferred()
    response = old.promise
    const oldResult = value.runNumberInput(plugin.plugin_id, plugin.instance_id, input, 35)
    current[0].instance_id = 'worker-2'
    await value.refresh()
    const next = deferred()
    response = next.promise
    const nextResult = value.runNumberInput(plugin.plugin_id, 'worker-2', input, 45)
    old.reject(new Error('old worker outcome'))
    await oldResult
    expect(value.failedActions.value.size).toBe(0)
    expect(
      value.pendingActions.value.has(value.numberInputKey(plugin.plugin_id, 'worker-2', input))
    ).toBe(true)
    next.resolve()
    await nextResult
    expect(value.pendingActions.value.size).toBe(0)
    expect(calls()).toHaveLength(2)
  })
})

import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createI18n } from 'vue-i18n'
import en from '../messages.en'
import PluginContribution from './PluginContribution.vue'
import PluginPanels from './PluginPanels.vue'
import type {
  ActionContribution,
  DashboardContribution,
  NumberInputContribution,
  PluginSnapshot,
} from './types'

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }))

const state: DashboardContribution = { kind: 'text', id: 'state', title: 'Room', text: 'Off' }
const action: ActionContribution = {
  kind: 'action',
  id: 'on',
  title: 'Room',
  label: 'Turn on',
  action_id: 'set-on',
  params: { target: 'fixed' },
  state_id: state.id,
}
const input: NumberInputContribution = {
  kind: 'number_input',
  id: 'level',
  title: 'Room',
  action_id: 'set-level',
  label: 'Set level',
  unit: '%',
  input_revision: 'revision-1',
  value_scaled: 20,
  min_scaled: 0,
  max_scaled: 100,
  step_scaled: 5,
  decimal_places: 0,
  state_id: state.id,
}

function plugin(
  contributions: DashboardContribution[],
  pluginId = 'example.controls'
): PluginSnapshot {
  return {
    plugin_id: pluginId,
    instance_id: 'instance-1',
    state: 'running',
    generation: 1,
    restart_count: 0,
    last_error: null,
    contributions: structuredClone(contributions),
  }
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
let unlocked: boolean
let snapshotFailed: boolean
let response: Promise<unknown> | undefined
let wrapper: VueWrapper | undefined
const callbacks = new Map<string, () => void>()

beforeEach(() => {
  current = [plugin([state, action, input])]
  unlocked = true
  snapshotFailed = false
  response = undefined
  native.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'auth_status') return { unlocked }
    if (command === 'get_plugin_snapshot') {
      if (snapshotFailed) throw new Error('private transport error')
      return structuredClone(current)
    }
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
  callbacks.clear()
})

async function panel() {
  wrapper = mount(PluginPanels, {
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] },
  })
  await flushPromises()
  return wrapper
}

async function update(event = 'plugin-host-update') {
  const callback = callbacks.get(event)
  if (!callback) throw new Error(`Missing listener: ${event}`)
  callback()
  await flushPromises()
}

async function publish(items: DashboardContribution[]) {
  current[0].contributions = structuredClone(items)
  await update()
}

function calls() {
  return native.invoke.mock.calls.filter(([command]) => command === 'plugin_action')
}

describe('generic plugin entity cards', () => {
  it('groups forward references under state order and keeps control order and accessible labels', async () => {
    const metric: DashboardContribution = {
      kind: 'metric',
      id: 'reading',
      title: 'Reading',
      value: 24.5,
      unit: '°C',
    }
    const status: DashboardContribution = {
      kind: 'status',
      id: 'connection',
      title: 'Connection',
      value: 'Ready',
      tone: 'success',
    }
    const off = { ...action, id: 'off', action_id: 'set-off', label: 'Turn off' }
    const refresh = { ...action, id: 'refresh', title: 'Refresh', state_id: metric.id }
    const check = { ...action, id: 'check', title: 'Check', state_id: status.id }
    const loose = { ...action, id: 'loose', state_id: undefined }
    current = [plugin([off, metric, input, state, refresh, status, action, check, loose])]
    const view = await panel()
    expect(view.findAll('[data-card-id]').map((card) => card.attributes('data-card-id'))).toEqual([
      'reading',
      'state',
      'connection',
      'loose',
    ])
    const room = view.find('[data-state-card="state"]')
    expect(
      room.findAll('[data-contribution-id]').map((item) => item.attributes('data-contribution-id'))
    ).toEqual(['state', 'off', 'level', 'on'])
    expect(room.findAll('p').filter((title) => title.text() === 'Room')).toHaveLength(1)
    expect(room.find('button').attributes('aria-label')).toBe('Room: Turn off')
    expect(room.find('input').attributes('aria-label')).toBe('Room: Set level')
    expect(view.find('[data-state-card="reading"]').text()).toContain('24.5°C')
    expect(view.find('[data-state-card="connection"] output').text()).toBe('Ready')
    expect(view.find('[data-card-id="loose"]').text()).toContain('Room')
  })

  it('keeps duplicate friendly names and identical IDs in different plugins independent', async () => {
    const otherState = { ...state, id: 'other-state', text: 'On' }
    const otherAction = {
      ...action,
      id: 'other-on',
      action_id: 'other-set-on',
      state_id: otherState.id,
    }
    current = [
      plugin([state, action, otherState, otherAction]),
      plugin([state, action], 'example.second'),
    ]
    const view = await panel()
    const sections = view.findAll('section')
    expect(sections[0].findAll('[data-state-card]')).toHaveLength(2)
    expect(sections[1].findAll('[data-state-card]')).toHaveLength(1)
    await sections[0].find('[data-contribution-id="other-on"] button').trigger('click')
    await flushPromises()
    await sections[1].find('button').trigger('click')
    await flushPromises()
    expect(calls().map((call) => call[1])).toEqual([
      {
        pluginId: 'example.controls',
        instanceId: 'instance-1',
        actionId: 'other-set-on',
        params: action.params,
      },
      {
        pluginId: 'example.second',
        instanceId: 'instance-1',
        actionId: 'set-on',
        params: action.params,
      },
    ])
  })

  it('never infers a group from names, ID patterns, another plugin or a control reference', async () => {
    const flat = { ...action, state_id: undefined }
    const invalidTarget = { ...action, id: 'invalid', state_id: action.id }
    const foreignTarget = { ...action, id: 'foreign', state_id: 'foreign-state' }
    current = [
      plugin([state, flat, input, invalidTarget, foreignTarget]),
      plugin([{ ...state, id: 'foreign-state' }], 'example.second'),
    ]
    const view = await panel()
    const first = view.find('section')
    expect(first.find('[data-state-card="state"]').findAll('button')).toHaveLength(1)
    expect(first.find('[data-card-id="on"]').exists()).toBe(true)
    expect(first.find('[data-card-id="invalid"]').exists()).toBe(true)
    expect(first.find('[data-card-id="foreign"]').exists()).toBe(true)
    expect(view.findAll('[data-state-card]')).toHaveLength(1)
  })

  it('dispatches only the exact currently advertised action after a presentation change', async () => {
    current = [plugin([state, action])]
    const view = await panel()
    const control = view
      .findAllComponents(PluginContribution)
      .find((item) => item.props('item').kind === 'action')
    if (!control) throw new Error('Expected an action component')
    const old = { ...control.props('item') }
    const replacement = { ...action, title: 'Renamed room', label: 'Enable room' }
    await publish([{ ...state, title: 'Renamed room' }, replacement])
    control.vm.$emit('action', old)
    control.vm.$emit('action', { ...replacement, params: { target: 'caller-changed' } })
    await flushPromises()
    expect(calls()).toHaveLength(0)
    await view.find('button').trigger('click')
    await flushPromises()
    expect(calls()[0][1]).toEqual({
      pluginId: 'example.controls',
      instanceId: 'instance-1',
      actionId: action.action_id,
      params: action.params,
    })
  })

  it('preserves pending and unconfirmed outcomes through regrouping, withdrawal and restoration', async () => {
    const pending = deferred()
    response = pending.promise
    current = [plugin([state, action]), plugin([state, action], 'example.second')]
    const view = await panel()
    await view.find('button').trigger('click')
    await publish([{ ...action, title: 'Renamed room', state_id: undefined }, state])
    expect(view.find('button').attributes('aria-busy')).toBe('true')
    expect(view.findAll('section')[1].find('button').attributes('disabled')).toBeUndefined()
    await view.find('button').trigger('click')
    await publish([state])
    expect(view.find('section').find('button').exists()).toBe(false)
    await publish([state, action])
    expect(view.find('button').attributes('aria-busy')).toBe('true')
    await view.find('button').trigger('click')
    expect(calls()).toHaveLength(1)
    await publish([state])
    pending.reject(new Error('private upstream service detail'))
    await flushPromises()
    await publish([state, { ...action, label: 'Enable' }])
    expect(view.find('[role="alert"]').text()).toContain('It may have completed.')
    expect(view.text()).not.toContain('private upstream')
    expect(view.find('button').attributes('disabled')).toBeUndefined()
    expect(calls()).toHaveLength(1)
  })

  it('preserves numeric drafts through state, title and order changes and requires revision review', async () => {
    const view = await panel()
    const field = view.find('input').element
    await view.find('input').setValue('35')
    await publish([
      input,
      { ...action, label: 'Enable' },
      { ...state, title: 'New title', text: 'Observed 25' },
    ])
    expect(view.find('input').element).toBe(field)
    expect(view.find('input').element.value).toBe('35')
    await publish([
      { ...state, text: 'Observed 30' },
      { ...input, value_scaled: 30, input_revision: 'revision-2' },
      action,
    ])
    expect(view.find('input').element.value).toBe('35')
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    expect(view.find('output[id$="-review"]').text()).toContain('available settings have changed')
    await view.find('form').trigger('submit')
    expect(calls()).toHaveLength(0)
    const review = view.findAll('button').find((button) => button.text() === 'Use latest settings')
    if (!review) throw new Error('Expected explicit settings review')
    await review.trigger('click')
    expect(view.find('input').element.value).toBe('30')
    await view.find('input').setValue('40')
    await view.find('form').trigger('submit')
    await flushPromises()
    expect(calls()[0][1]).toMatchObject({
      actionId: input.action_id,
      params: { input_revision: 'revision-2', value_scaled: 40 },
    })
    expect(view.text()).toContain('Observed 30')
  })

  it('resets a numeric draft deliberately when the presentation moves it to a different card', async () => {
    const view = await panel()
    await view.find('input').setValue('35')
    await publish([state, { ...input, state_id: undefined, value_scaled: 25 }])
    expect(view.find('input').element.value).toBe('25')
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    await view.find('form').trigger('submit')
    await publish([state, { ...input, value_scaled: 30 }])
    expect(view.find('input').element.value).toBe('30')
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    expect(calls()).toHaveLength(0)
  })

  it('retains pending numeric operations and failures when the editor moves or is withdrawn', async () => {
    const pending = deferred()
    response = pending.promise
    const view = await panel()
    await view.find('input').setValue('35')
    await view.find('form').trigger('submit')
    await publish([state, { ...input, state_id: undefined, input_revision: 'revision-2' }])
    expect(view.find('button[type="submit"]').attributes('aria-busy')).toBe('true')
    await publish([state])
    expect(view.find('form').exists()).toBe(false)
    await publish([state, { ...input, input_revision: 'revision-3' }])
    expect(view.find('input').attributes('disabled')).toBeDefined()
    expect(view.find('button[type="submit"]').attributes('aria-busy')).toBe('true')
    await view.find('form').trigger('submit')
    expect(calls()).toHaveLength(1)
    pending.reject(new Error('private numeric failure'))
    await flushPromises()
    expect(view.find('[role="alert"]').text()).toContain('It may have completed.')
    expect(view.text()).not.toContain('private numeric')
  })

  it('disables grouped controls on snapshot failure and clears them on authentication revocation', async () => {
    const pending = deferred()
    response = pending.promise
    const view = await panel()
    await view.find('[data-contribution-id="on"] button').trigger('click')
    snapshotFailed = true
    await update()
    expect(view.find('[data-state-card]').text()).toContain('Off')
    expect(
      view.findAll('button').every((button) => button.attributes('disabled') !== undefined)
    ).toBe(true)
    expect(view.find('input').attributes('disabled')).toBeDefined()
    unlocked = false
    await update('auth-state-changed')
    expect(view.find('section').exists()).toBe(false)
    pending.reject(new Error('late revoked result'))
    await flushPromises()
    expect(view.text()).toBe('')
    snapshotFailed = false
    unlocked = true
    await update('auth-state-changed')
    expect(view.find('[role="alert"]').exists()).toBe(false)
    expect(view.find('input').element.value).toBe('20')
    expect(calls()).toHaveLength(1)
  })

  it('resets numeric drafts and operation state across instances even when all visible IDs are reused', async () => {
    const pending = deferred()
    response = pending.promise
    const view = await panel()
    await view.find('input').setValue('35')
    await view.find('[data-contribution-id="on"] button').trigger('click')
    current[0].instance_id = 'instance-2'
    await publish([state, action, { ...input, value_scaled: 25 }])
    expect(view.find('input').element.value).toBe('25')
    expect(view.find('[data-contribution-id="on"] button').attributes('disabled')).toBeUndefined()
    expect(view.find('button[type="submit"]').attributes('disabled')).toBeDefined()
    pending.reject(new Error('old instance failure'))
    await flushPromises()
    expect(view.find('[role="alert"]').exists()).toBe(false)
    response = undefined
    await view.find('[data-contribution-id="on"] button').trigger('click')
    await flushPromises()
    expect(calls()[1][1]).toMatchObject({ instanceId: 'instance-2' })
  })

  it('renders old contributions flat without changing their order, titles or text escaping', async () => {
    const flatAction = { ...action, state_id: undefined }
    const flatInput = { ...input, state_id: undefined }
    current = [plugin([flatAction, { ...state, text: '<script>private()</script>' }, flatInput])]
    const view = await panel()
    expect(view.findAll('[data-card-id]').map((card) => card.attributes('data-card-id'))).toEqual([
      'on',
      'state',
      'level',
    ])
    expect(view.find('[data-state-card]').exists()).toBe(false)
    expect(view.findAll('p').filter((title) => title.text() === 'Room')).toHaveLength(3)
    expect(view.find('script').exists()).toBe(false)
    expect(view.text()).toContain('<script>private()</script>')
  })
})

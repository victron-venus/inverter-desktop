import { enableAutoUnmount, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createI18n } from 'vue-i18n'
import AppHeader from '../components/AppHeader.vue'
import type { DashboardControlView, LockState } from '../dashboardControlView'
import en from '../i18n/en'

vi.mock('@features', () => ({ isMobileApp: false }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
enableAutoUnmount(afterEach)

function lock(state: LockState): DashboardControlView {
  return {
    id: 'front-lock',
    label: 'Front door',
    entity: '',
    lockState: state,
    state: state === 'locked' ? 'on' : state === 'unlocked' ? 'off' : 'unavailable',
    activate: vi.fn(),
  }
}
function render(control: DashboardControlView) {
  return mount(AppHeader, {
    props: {
      dryRun: false,
      essClass: 'off',
      essText: 'ESS',
      showHeaderToggles: true,
      headerControls: [control],
      controlStates: {},
      isDark: false,
    },
    global: {
      plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })],
    },
  })
}

describe('header lock state', () => {
  it.each(['locked', 'unlocked'] as const)(
    'names the observed %s state before activation',
    async (state) => {
      const control = lock(state)
      const wrapper = render(control)
      const button = wrapper.get(`button[aria-label="Front door: ${en.lock[state]}"]`)
      expect(button.text()).toContain(en.lock[state])
      expect(button.attributes('title')).toBe(`Front door: ${en.lock[state]}`)
      expect(button.attributes('aria-pressed')).toBe(state === 'locked' ? 'true' : 'false')
      expect(button.attributes('disabled')).toBeUndefined()
      await button.trigger('click')
      expect(control.activate).toHaveBeenCalledOnce()
      expect(wrapper.emitted('send')).toBeUndefined()
    }
  )

  it.each(['locking', 'unlocking', 'jammed', 'unknown', 'unavailable'] as const)(
    'distinguishes %s from unlocked and prevents activation',
    async (state) => {
      const control = lock(state)
      const wrapper = render(control)
      const button = wrapper.get(`button[aria-label="Front door: ${en.lock[state]}"]`)
      expect(button.text()).toContain(en.lock[state])
      expect(button.attributes('aria-pressed')).toBeUndefined()
      expect(button.attributes('disabled')).toBeDefined()
      await button.trigger('click')
      expect(control.activate).not.toHaveBeenCalled()
      expect(wrapper.emitted('send')).toBeUndefined()
    }
  )

  it('preserves the observed state while pending and gives a full unconfirmed outcome', async () => {
    const control = lock('unlocked')
    const wrapper = render({ ...control, pending: true })
    const button = wrapper.get('button[aria-label^="Front door:"]')
    expect(button.text()).toContain('Front door')
    expect(button.text()).toContain('Unlocked')
    expect(button.text()).toContain(en.lock.pending)
    expect(button.attributes('aria-busy')).toBe('true')
    await button.trigger('click')
    expect(control.activate).not.toHaveBeenCalled()
    await wrapper.setProps({ headerControls: [{ ...control, failed: true }] })
    expect(button.get('[role="alert"]').text()).toContain(en.lock.notConfirmed)
    expect(button.attributes('aria-label')).toContain(en.lock.unconfirmed)
    expect(button.attributes('aria-pressed')).toBe('false')
    expect(button.attributes('aria-busy')).toBeUndefined()
  })

  it('preserves ordinary core toggle labels and dispatch', async () => {
    const wrapper = render({
      id: 'limit',
      label: 'Export limit',
      entity: 'no_feed',
      state: 'on',
    })
    const button = wrapper.findAll('button').find((item) => item.text() === 'Export limit')!
    expect(button.attributes('aria-pressed')).toBe('true')
    await button.trigger('click')
    expect(wrapper.emitted('send')).toEqual([['toggle', { entity: 'no_feed' }]])
  })
})

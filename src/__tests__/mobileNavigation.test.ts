import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import MobileShell from '../MobileShell.vue'
import AppHeader from '../components/AppHeader.vue'
import ContextMenu from '../components/ContextMenu.vue'
import { defaultConfig } from '../config'
import { i18n } from '../i18n'

const boundary = vi.hoisted(() => ({
  invoke: vi.fn(),
  closeWindow: vi.fn(),
  dashboardMount: vi.fn(),
  dashboardUnmount: vi.fn(),
  callbacks: new Map<string, () => void>(),
  stops: [] as Array<ReturnType<typeof vi.fn>>,
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke: boundary.invoke }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ close: boundary.closeWindow }),
}))
vi.mock('@tauri-apps/api/event', () => ({
  emit: vi.fn(async () => {}),
  listen: vi.fn(async (name: string, callback: () => void) => {
    boundary.callbacks.set(name, callback)
    const stop = vi.fn(() => boundary.callbacks.delete(name))
    boundary.stops.push(stop)
    return stop
  }),
}))
vi.mock('../App.vue', async () => {
  const { defineComponent, h, onMounted, onUnmounted } = await import('vue')
  return {
    default: defineComponent({
      setup() {
        onMounted(boundary.dashboardMount)
        onUnmounted(boundary.dashboardUnmount)
        return () => h('p', { 'data-testid': 'dashboard' }, 'Live dashboard')
      },
    }),
  }
})

let wrapper: VueWrapper | undefined
beforeEach(() => {
  globalThis.history.replaceState(null, '', '/')
  boundary.callbacks.clear()
  boundary.stops = []
  boundary.closeWindow.mockReset()
  boundary.dashboardMount.mockReset()
  boundary.dashboardUnmount.mockReset()
  boundary.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'get_config') return { ...defaultConfig, setup_completed: true }
    if (command === 'get_state') return {}
    if (command === 'close_config_window') boundary.callbacks.get('mobile-close-settings')?.()
    return undefined
  })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  vi.restoreAllMocks()
  globalThis.history.replaceState(null, '', '/')
})

describe('single WebView mobile settings', () => {
  it('keeps the dashboard mounted through save and WebView Back, and can reopen settings', async () => {
    wrapper = mount(MobileShell, { global: { plugins: [i18n] } })
    await flushPromises()
    boundary.callbacks.get('mobile-open-settings')!()
    await flushPromises()
    expect(globalThis.location.hash).toBe('#settings')
    expect(wrapper.find('#mqtt_host').exists()).toBe(true)
    await wrapper.get('#mqtt_host').setValue('configured-cerbo')
    await wrapper.get('button[title="Save changes"]').trigger('click')
    await flushPromises()
    expect(boundary.invoke).toHaveBeenCalledWith('save_config', {
      config: expect.objectContaining({ mqtt_host: 'configured-cerbo' }),
    })
    expect(boundary.invoke).not.toHaveBeenCalledWith('set_auto_start', expect.anything())
    globalThis.history.back()
    await vi.waitFor(() => expect(globalThis.location.hash).toBe(''))
    await flushPromises()
    expect(wrapper.find('#mqtt_host').exists()).toBe(false)
    expect(boundary.dashboardMount).toHaveBeenCalledTimes(1)
    expect(boundary.dashboardUnmount).not.toHaveBeenCalled()
    boundary.callbacks.get('mobile-open-settings')!()
    await flushPromises()
    expect(wrapper.find('#mqtt_host').exists()).toBe(true)
    expect(boundary.dashboardMount).toHaveBeenCalledTimes(1)
  })

  it('closes the settings page without closing the Activity and releases listeners', async () => {
    wrapper = mount(MobileShell, { global: { plugins: [i18n] } })
    await flushPromises()
    boundary.callbacks.get('mobile-open-settings')!()
    await flushPromises()
    await wrapper.get('button[aria-label="Close settings"]').trigger('click')
    await vi.waitFor(() => expect(globalThis.location.hash).toBe(''))
    await flushPromises()
    expect(boundary.invoke).toHaveBeenCalledWith('close_config_window')
    expect(boundary.closeWindow).not.toHaveBeenCalled()
    wrapper.unmount()
    wrapper = undefined
    expect(boundary.stops.every((stop) => stop.mock.calls.length === 1)).toBe(true)
  })

  it('offers a visible Settings action without offering GitHub sideload updates', async () => {
    wrapper = mount(AppHeader, {
      props: {
        dryRun: false,
        essClass: 'on',
        essText: 'ESS',
        headerControls: [],
        controlStates: {},
        isDark: false,
      },
    })
    await wrapper.get('button[aria-label="Settings"]').trigger('click')
    expect(wrapper.emitted('open-config')).toHaveLength(1)
    wrapper.unmount()
    wrapper = mount(ContextMenu, { props: { show: true, x: 0, y: 0 } })
    expect(wrapper.text()).toContain('Settings')
    expect(wrapper.text()).not.toContain('Download updates')
  })

  it('does not subscribe after settings close while secure configuration is still loading', async () => {
    let finishLoad!: (value: unknown) => void
    const loading = new Promise((resolve) => {
      finishLoad = resolve
    })
    boundary.invoke.mockImplementation(async (command: string) => {
      if (command === 'get_config') return loading
      if (command === 'close_config_window') boundary.callbacks.get('mobile-close-settings')?.()
      return {}
    })
    wrapper = mount(MobileShell, { global: { plugins: [i18n] } })
    await flushPromises()
    boundary.callbacks.get('mobile-open-settings')!()
    await flushPromises()
    await wrapper.get('button[aria-label="Close settings"]').trigger('click')
    await vi.waitFor(() => expect(globalThis.location.hash).toBe(''))
    await flushPromises()
    finishLoad({ ...defaultConfig, setup_completed: true })
    await flushPromises()
    expect(boundary.callbacks.has('mqtt-state-update')).toBe(false)
    expect(boundary.invoke).not.toHaveBeenCalledWith('get_state')
  })
})

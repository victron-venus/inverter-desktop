import { flushPromises, mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import DashboardPanels from './DashboardPanels.vue'

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(async () => vi.fn()) }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

describe('desktop dashboard panels', () => {
  it('keeps the home controls without mounting a second plugin entity dashboard', async () => {
    const wrapper = mount(DashboardPanels, {
      global: {
        provide: { 'desktop-home': { getHaControlState: () => undefined } },
        stubs: { HomePanels: true },
      },
    })
    try {
      await flushPromises()
      const home = wrapper.findComponent({ name: 'HomePanels' })
      expect(home.exists()).toBe(true)
      expect(wrapper.findComponent({ name: 'PluginPanels' }).exists()).toBe(false)
      expect(native.invoke).not.toHaveBeenCalled()
      home.vm.$emit('send', 'toggle', { entity: 'light.kitchen' })
      expect(wrapper.emitted('send')).toEqual([['toggle', { entity: 'light.kitchen' }]])
    } finally {
      wrapper.unmount()
    }
  })
})

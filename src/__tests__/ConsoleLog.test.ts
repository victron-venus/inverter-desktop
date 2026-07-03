import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import ConsoleLog from '../components/ConsoleLog.vue'

describe('ConsoleLog', () => {
  it('hidden when lines empty', () => {
    const wrapper = mount(ConsoleLog, { props: { lines: [] } })
    expect(wrapper.find('.console-log').exists()).toBe(false)
  })

  it('shows entry count', () => {
    const wrapper = mount(ConsoleLog, {
      props: { lines: ['msg1', 'msg2', 'msg3'] },
    })
    expect(wrapper.text()).toContain('Console (3)')
  })

  it('collapsed by default', () => {
    const wrapper = mount(ConsoleLog, {
      props: { lines: ['msg1'] },
    })
    expect(wrapper.find('.max-h-40').exists()).toBe(false)
  })

  it('expands on click', async () => {
    const wrapper = mount(ConsoleLog, {
      props: { lines: ['msg1', 'msg2'] },
    })
    await wrapper.find('button').trigger('click')
    await nextTick()
    expect(wrapper.find('.max-h-40').exists()).toBe(true)
    expect(wrapper.text()).toContain('msg1')
    expect(wrapper.text()).toContain('msg2')
  })

  it('collapses on second click', async () => {
    const wrapper = mount(ConsoleLog, {
      props: { lines: ['msg1'] },
    })
    await wrapper.find('button').trigger('click')
    await nextTick()
    expect(wrapper.find('.max-h-40').exists()).toBe(true)
    await wrapper.find('button').trigger('click')
    await nextTick()
    expect(wrapper.find('.max-h-40').exists()).toBe(false)
  })

  it('renders all lines when expanded', async () => {
    const lines = ['line 1', 'line 2', 'line 3', 'line 4', 'line 5']
    const wrapper = mount(ConsoleLog, { props: { lines } })
    await wrapper.find('button').trigger('click')
    await nextTick()
    const items = wrapper.findAll('.break-all')
    expect(items).toHaveLength(5)
  })
})

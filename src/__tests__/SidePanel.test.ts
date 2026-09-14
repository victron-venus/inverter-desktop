import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import SidePanel from '../components/SidePanel.vue'

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

const baseProps = {
  showEv: true,
  evSectionVisible: true,
  carSoc: 80,
  carChargingPower: 0,
  evChargingPower: 0,
  waterVisible: true,
  waterLevel: 42,
  waterValve: false,
  pumpSwitch: false,
  homeButtons: [],
  buttonStates: {},
}

describe('SidePanel', () => {
  it('renders with minimal props', () => {
    const wrapper = mount(SidePanel, { props: baseProps })
    expect(wrapper.exists()).toBe(true)
  })

  it('shows EV section when showEv true', () => {
    const wrapper = mount(SidePanel, {
      props: { ...baseProps, showEv: true },
    })
    expect(wrapper.text()).toContain('sections.ev')
    expect(wrapper.text()).toContain('80%')
  })

  it('hides EV section when showEv false', () => {
    const wrapper = mount(SidePanel, {
      props: { ...baseProps, showEv: false },
    })
    expect(wrapper.text()).not.toContain('sections.ev')
  })

  it('shows water section with level in percent', () => {
    const wrapper = mount(SidePanel, { props: baseProps })
    expect(wrapper.text()).toContain('sections.water')
    expect(wrapper.text()).toContain('42%')
  })

  it('shows pump and valve status badges without toggle buttons', () => {
    const wrapper = mount(SidePanel, { props: baseProps })
    expect(wrapper.text()).toContain('sections.pump')
    expect(wrapper.text()).toContain('sections.valve')
    // dbus-pump owns control - no toggle buttons in the water card
    expect(wrapper.find('.classic-btn[disabled]').exists()).toBe(false)
    const waterCard = wrapper
      .findAll('.classic-card')
      .find((c) => c.text().includes('sections.water'))
    expect(waterCard?.findAll('button').length).toBe(2)
  })

  it('hides pump badge when state unknown', () => {
    const wrapper = mount(SidePanel, {
      props: { ...baseProps, pumpSwitch: null, waterValve: null },
    })
    expect(wrapper.text()).not.toContain('sections.pump')
    expect(wrapper.text()).not.toContain('sections.valve')
  })

  it('shows home buttons when provided', () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        showHomeSection: true,
        homeButtons: [
          { id: 'btn1', label: 'Button 1', entity: 'switch.one' },
          { id: 'btn2', label: 'Button 2', entity: 'switch.two' },
        ],
      },
    })
    const normalized = wrapper.text().replace(/\s+/g, ' ')
    expect(normalized).toContain('Button 1')
    expect(normalized).toContain('Button 2')
  })

  it('marks unavailable home tiles with classic-btn-unavailable', () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        showHomeSection: true,
        homeButtons: [
          { id: 'garage', label: 'Garage opener', entity: 'switch.garage' },
          { id: 'lamp', label: 'Lamp', entity: 'switch.lamp' },
        ],
        buttonStates: { garage: 'unavailable', lamp: 'on' },
      },
    })
    const buttons = wrapper.findAll('button.classic-btn-tile')
    expect(buttons).toHaveLength(2)
    expect(buttons[0].classes()).toContain('classic-btn-unavailable')
    expect(buttons[1].classes()).toContain('classic-btn-on')
    expect(buttons[1].classes()).not.toContain('classic-btn-unavailable')
    expect(wrapper.find('.home-btn-grid').exists()).toBe(true)
    expect(wrapper.find('.home-tile-label').text().replace(/\s+/g, ' ')).toContain('Garage opener')
  })

  it('hides home section when showHomeSection false', () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        showHomeSection: false,
        homeButtons: [{ id: 'btn1', label: 'Btn', entity: 'switch.one' }],
      },
    })
    expect(wrapper.text()).not.toContain('Btn')
  })
})

describe('core controls without optional connections', () => {
  it('keeps inverter controls usable while external controls are disconnected', async () => {
    const wrapper = mount(SidePanel, {
      props: {
        ...baseProps,
        controlsConnected: false,
        showHomeSection: true,
        homeButtons: [
          { id: 'limit', label: 'Export limit', entity: 'no_feed' },
          { id: 'lamp', label: 'Lamp', entity: 'switch.lamp' },
        ],
        buttonStates: { limit: 'on' },
      },
    })
    const buttons = wrapper.findAll('button.classic-btn-tile')
    expect(buttons[0].attributes('disabled')).toBeUndefined()
    expect(buttons[1].attributes('disabled')).toBeDefined()
    await buttons[0].trigger('click')
    expect(wrapper.emitted('send')).toEqual([['toggle', { entity: 'no_feed' }]])
    expect(wrapper.text()).not.toContain('status.haStale')
  })
})

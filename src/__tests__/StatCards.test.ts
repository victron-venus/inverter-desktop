import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import StatCards from '../components/StatCards.vue'

const baseProps = {
  gt: 1200,
  g1: 600,
  g2: 600,
  tt: 800,
  t1: 400,
  t2: 400,
  solarTotal: 2500,
  mpptTotal: 1500,
  pvInvertersTotal: 1000,
  batterySoc: 87,
  batteryPower: -500,
  batteryVoltage: 52.4,
  batteryCurrent: -9.5,
  setpoint: 0,
  inverterState: 'Inverting',
}

describe('StatCards sticky hold', () => {
  it('holds last-known battery metrics when props go nullish', async () => {
    const wrapper = mount(StatCards, { props: baseProps })
    expect(wrapper.text()).toContain('87%')
    expect(wrapper.text()).toContain('52.40V')
    expect(wrapper.text()).toContain('-9.5A')
    expect(wrapper.text()).toContain('-500W')

    await wrapper.setProps({
      batterySoc: undefined,
      batteryVoltage: undefined,
      batteryCurrent: undefined,
      batteryPower: undefined,
      gt: undefined,
      solarTotal: undefined,
    })
    await nextTick()

    expect(wrapper.text()).toContain('87%')
    expect(wrapper.text()).toContain('52.40V')
    expect(wrapper.text()).toContain('-9.5A')
    expect(wrapper.text()).toContain('-500W')
    expect(wrapper.text()).toContain('1.2kW')
    expect(wrapper.text()).toContain('2.5kW')
  })

  it('accepts explicit zero for power without holding previous', async () => {
    const wrapper = mount(StatCards, { props: { ...baseProps, gt: 500 } })
    expect(wrapper.text()).toContain('500W')
    await wrapper.setProps({ gt: 0 })
    await nextTick()
    expect(wrapper.text()).toMatch(/Grid\s*0W/)
  })
})

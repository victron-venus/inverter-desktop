import { enableAutoUnmount, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import StatCards from '../components/StatCards.vue'

enableAutoUnmount(afterEach)
afterEach(() => vi.useRealTimers())
vi.mock('../components/SetpointOverride.vue', () => ({ default: { template: '<span />' } }))

const baseProps = {
  gridBackupObservedAt: Date.now() / 1000,
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
  const gridBackup = {
    enabled: true,
    available: true,
    service: 'com.victronenergy.acload.example',
    device_instance: 78,
    name: 'Home',
    power: -750,
    measurement_time: 1000,
    age_seconds: 1,
  }

  it('expires backup readiness when daemon stops despite other MQTT updates', async () => {
    vi.useFakeTimers()
    const wrapper = mount(StatCards, {
      props: {
        ...baseProps,
        gridBackup,
        gridUsingBackup: true,
        gridBackupObservedAt: Date.now() / 1000,
      },
    })
    expect(wrapper.get('[data-testid="grid-backup"]').text()).toContain('(active)')
    await vi.advanceTimersByTimeAsync(31000)
    await wrapper.setProps({ gt: 200 })
    expect(wrapper.get('[data-testid="grid-backup"]').text()).toBe('· Home —')
    expect(wrapper.get('[data-testid="grid-backup"]').attributes('title')).toContain('stale')
  })

  it('shows only the selected submeter next to Grid, preserving primary totals', () => {
    const wrapper = mount(StatCards, { props: { ...baseProps, gridBackup } })
    const card = wrapper.find('.metric-card')
    expect(card.find('.classic-stat-label').text().replace(/\s+/g, ' ')).toBe('Grid · Home -750W')
    expect(card.find('.classic-stat-value').text()).toBe('1.2kW')
    expect(wrapper.get('[data-testid="grid-backup"]').attributes('title')).toContain('ready')
  })

  it('clears unavailable submeter power and shows recovery including zero', async () => {
    const wrapper = mount(StatCards, { props: { ...baseProps, gridBackup } })
    await wrapper.setProps({ gridBackup: { ...gridBackup, available: false, power: null } })
    expect(wrapper.get('[data-testid="grid-backup"]').text()).toBe('· Home —')
    await wrapper.setProps({ gridBackup: { ...gridBackup, power: 0 }, gridUsingBackup: true })
    expect(wrapper.get('[data-testid="grid-backup"]').text()).toContain('Home 0W (active)')
  })

  it('shows a detected submeter with backup disabled and removes a cleared selection', async () => {
    const wrapper = mount(StatCards, {
      props: { ...baseProps, gridBackup: { ...gridBackup, enabled: false } },
    })
    expect(wrapper.get('[data-testid="grid-backup"]').attributes('title')).toContain('disabled')
    await wrapper.setProps({ gridBackup: { ...gridBackup, service: null } })
    expect(wrapper.find('[data-testid="grid-backup"]').exists()).toBe(false)
  })

  it('does not invent a submeter when no selection is published', () => {
    expect(
      mount(StatCards, { props: baseProps }).find('[data-testid="grid-backup"]').exists()
    ).toBe(false)
  })

  it('clears an explicitly unavailable phase rather than holding its old watts', async () => {
    const wrapper = mount(StatCards, { props: baseProps })
    await wrapper.setProps({ g2: undefined, gridL2Available: false, gt: 600 })
    expect(wrapper.find('.classic-stat-meta').text()).toBe('600W · —')
    await wrapper.setProps({ g1: undefined, gt: undefined, gridL1Available: false })
    expect(wrapper.find('.classic-stat-value').text()).toBe('—')
  })

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

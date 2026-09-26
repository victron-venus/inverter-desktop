import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import DailyStats from '../components/DailyStats.vue'
import { newDraft, rateGrid, validatePlan } from '../tariffs/model'
import { state } from '../composables/useInverterState'

vi.mock('../composables/useInverterState', () => ({
  state: ref({}),
}))

describe('DailyStats', () => {
  it('keeps a writable controller tariff informational on the dashboard', () => {
    ;(state as ReturnType<typeof ref>).value = {
      daily_stats: { grid_kwh: 10 },
      ui_config: {
        electricity_tariff: validatePlan({ ...newDraft(), rates: rateGrid(0.2) }),
        electricity_tariff_status: { writable: true, revision: 'revision' },
      },
    }
    const wrapper = mount(DailyStats)
    expect(wrapper.find('button').exists()).toBe(false)
    expect(wrapper.text()).toContain('2.00')
    expect(wrapper.text()).toContain('0.2000 USD/kWh')
    expect(wrapper.text()).not.toMatch(
      /Edit tariff|Set tariff|Interval energy|Use .*tariff|Billing period|Controller tariff/
    )
    wrapper.unmount()
  })
  it('breakdown parts add up to the headline total', async () => {
    ;(state as ReturnType<typeof ref>).value = {
      daily_stats: {
        produced_today: 15.99,
        pv_inverter_daily: [2.121, 2.619],
        mppt_daily: [3.2, 3.67, 4.38],
      },
      solar_forecast: { date: '2026-08-23', today_kwh: 13.39, tomorrow_kwh: 8.89 },
    }
    const wrapper = mount(DailyStats)
    const text = wrapper.text()
    // (pvInv1+pvInv2+MPPT_TOTAL(mppt1+mppt2+mppt3)) and 2.12+2.62+11.25 === 15.99
    expect(text).toContain('(2.12+2.62+11.25(3.20+3.67+4.38))')
    expect(text).toContain('15.99kWh')
    // forecast comes from top-level state.solar_forecast
    expect(text).toContain('[13.4]')
    expect(text).toContain('[8.9]')
    wrapper.unmount()
  })
})

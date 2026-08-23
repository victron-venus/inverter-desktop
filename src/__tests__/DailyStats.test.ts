import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import DailyStats from '../components/DailyStats.vue'
import { state } from '../composables/useInverterState'

vi.mock('../composables/useInverterState', () => ({
  state: ref({}),
}))

describe('DailyStats', () => {
  it('breakdown parts add up to the headline total', async () => {
    ;(state as ReturnType<typeof ref>).value = {
      daily_stats: {
        produced_today: 15.99,
        tasmota_daily: [2.121, 2.619],
        mppt_daily: [3.2, 3.67, 4.38],
      },
    }
    const wrapper = mount(DailyStats)
    const text = wrapper.text()
    // (tasmota1+tasmota2+MPPT_TOTAL(mppt1+mppt2+mppt3)) and 2.12+2.62+11.25 === 15.99
    expect(text).toContain('(2.12+2.62+11.25(3.20+3.67+4.38))')
    expect(text).toContain('15.99kWh')
  })
})

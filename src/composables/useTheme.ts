import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { state } from './useInverterState'
import { getAppConfig } from '../config'

export function useTheme() {
  const isDark = ref(localStorage.getItem('theme') !== 'light')
  if (!isDark.value) document.body.classList.add('light')

  async function toggleTheme() {
    isDark.value = !isDark.value
    document.body.classList.toggle('light', !isDark.value)
    const scheme = isDark.value ? 'dark' : 'light'
    localStorage.setItem('theme', scheme)
    try {
      const cfg = await getAppConfig()
      cfg.color_scheme = scheme
      await invoke('save_config', { config: cfg })
    } catch (e) {
      console.error('Failed to save theme:', e)
    }
  }

  const dailyStatsHtml = computed(() => {
    const ds = state.value.daily_stats || {}
    const prod = (ds.produced_today || 0).toFixed(2)
    const dollars = (ds.produced_dollars || 0).toFixed(2)
    const grid = (ds.grid_kwh || 0).toFixed(2)
    const gridCost = (parseFloat(grid) * 0.31).toFixed(2)
    const batIn = (ds.battery_in || 0).toFixed(2)
    const batOut = (ds.battery_out || 0).toFixed(2)
    const batInY = (ds.battery_in_yesterday || 0).toFixed(1)
    const batOutY = (ds.battery_out_yesterday || 0).toFixed(1)
    const batDelta = (parseFloat(batIn) - parseFloat(batOut)).toFixed(2)
    const batDeltaY = (parseFloat(batInY) - parseFloat(batOutY)).toFixed(1)
    const tasmotaDaily = ds.tasmota_daily || []
    const mpptDaily = ds.mppt_daily || []
    const pvTotalDaily = ds.pv_total_daily || 0
    let solarParts: string[] = []
    tasmotaDaily.forEach((v: number) => { if (v > 0) solarParts.push(v.toFixed(2)) })
    solarParts.push(pvTotalDaily.toFixed(2) + '(' + mpptDaily.map((v: number) => v.toFixed(2)).join('+') + ')')
    let result = `<span class="highlight">☀️ ${prod}kWh</span> <span class="detail">${solarParts.join('+')}</span> `
    result += `<span class="money">($${dollars})</span> | Grid: ${grid}kWh <span class="money">($${gridCost})</span> | `
    result += `🔋 I: ${batIn}kWh <span class="dim">(${batInY})</span>, O: ${batOut}kWh <span class="dim">(${batOutY})</span>; Δ: ${batDelta}kWh <span class="dim">(${batDeltaY})</span>`
    return result
  })

  return { isDark, toggleTheme, dailyStatsHtml }
}

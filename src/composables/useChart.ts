import { type Ref, ref } from 'vue'

const MAX_HISTORY_POINTS = 1800
const CHART_UPDATE_INTERVAL_MS = 2000

interface TooltipParam {
  value: number[]
  seriesName: string
  color: string
}

const historyData = {
  timestamps: [] as number[],
  grid: [] as number[],
  solar: [] as number[],
  battery: [] as number[],
  setpoint: [] as number[],
}

let chartUpdateCallback: (() => void) | null = null
let chartPaused = false

export function setChartUpdateCallback(cb: () => void) {
  chartUpdateCallback = cb
}

export function setChartPaused(paused: boolean) {
  chartPaused = paused
  if (!paused && chartUpdateCallback) {
    chartUpdateCallback()
  }
}

export function addHistoryPoint(newState: {
  gt?: number
  solar_total?: number
  battery_power?: number
  setpoint?: number
}) {
  if (newState.gt !== undefined) {
    const now = Date.now() / 1000
    historyData.timestamps.push(now)
    historyData.grid.push(newState.gt || 0)
    historyData.solar.push(newState.solar_total || 0)
    historyData.battery.push(newState.battery_power || 0)
    historyData.setpoint.push(newState.setpoint || 0)
    if (historyData.timestamps.length > MAX_HISTORY_POINTS) {
      historyData.timestamps.shift()
      historyData.grid.shift()
      historyData.solar.shift()
      historyData.battery.shift()
      historyData.setpoint.shift()
    }
    // Trigger chart update only if not paused
    if (!chartPaused && chartUpdateCallback) {
      chartUpdateCallback()
    }
  }
}

export function useChart(isDarkRef: Ref<boolean>) {
  const chartOption = ref({})
  let lastChartUpdate = 0

  // Register callback so addHistoryPoint triggers chart updates
  setChartUpdateCallback(() => updateChartOption(false))

  function updateChartOption(force: boolean) {
    if (chartPaused && !force) return

    const now = Date.now()
    if (!force && now - lastChartUpdate < CHART_UPDATE_INTERVAL_MS) return
    lastChartUpdate = now

    const { timestamps, grid, solar, battery, setpoint } = historyData
    const dark = isDarkRef.value
    const textColor = dark ? '#98989d' : '#636366'
    const gridColor = dark ? 'rgba(255,255,255,0.055)' : 'rgba(0,0,0,0.055)'
    const timeData = timestamps.map((ts) => ts * 1000)

    chartOption.value = {
      animation: false,
      backgroundColor: 'transparent',
      tooltip: {
        trigger: 'axis',
        backgroundColor: dark ? '#1c1c1e' : '#ffffff',
        borderColor: dark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.08)',
        borderWidth: 1,
        extraCssText: 'border-radius:10px;box-shadow:0 10px 28px rgba(0,0,0,0.16);padding:7px 9px;',
        axisPointer: { type: 'cross', label: { backgroundColor: dark ? '#3a3a3c' : '#8e8e93' } },
        textStyle: { color: dark ? '#f5f5f7' : '#1c1c1e', fontSize: 10 },
        formatter: (params: TooltipParam[]) => {
          const date = new Date(params[0].value[0])
          const timeStr = date.toLocaleTimeString([], {
            hour: '2-digit',
            minute: '2-digit',
          })
          let result = `${timeStr}<br/>`
          params.forEach((p: TooltipParam) => {
            if (p.seriesName === 'Setpoint') return
            const val = Math.floor(p.value[1])
            const valStr = val >= 1000 ? `${(val / 1000).toFixed(1)}kW` : `${val}W`
            result += `<span style="display:inline-block;margin-right:5px;border-radius:10px;width:10px;height:10px;background-color:${p.color};"></span>`
            result += `${p.seriesName}: ${valStr}<br/>`
          })
          return result
        },
      },
      legend: {
        data: ['Grid', 'Solar', 'Battery', 'Setpoint'],
        top: 0,
        itemWidth: 12,
        itemHeight: 8,
        textStyle: { color: textColor, fontSize: 10, fontWeight: 500 },
      },
      grid: { top: 28, bottom: 24, left: 42, right: 12, containLabel: false },
      xAxis: {
        type: 'time',
        axisLine: { lineStyle: { color: gridColor } },
        axisLabel: {
          color: textColor,
          fontSize: 10,
          formatter: '{HH}:{mm}',
        },
        splitLine: { show: false },
      },
      yAxis: {
        type: 'value',
        splitLine: { lineStyle: { color: gridColor, type: 'dashed' } },
        axisLabel: {
          color: textColor,
          fontSize: 10,
          formatter: (v: number) => (v >= 1000 ? `${(v / 1000).toFixed(1)}k` : v),
        },
      },
      series: [
        {
          name: 'Grid',
          type: 'line',
          smooth: 0.2,
          sampling: 'lttb',
          showSymbol: false,
          data: timeData.map((t, i) => [t, grid[i] || 0]),
          lineStyle: { color: '#3b82f6', width: 1.75 },
          areaStyle: { color: 'rgba(59,130,246,0.09)' },
        },
        {
          name: 'Solar',
          type: 'line',
          smooth: 0.2,
          sampling: 'lttb',
          showSymbol: false,
          data: timeData.map((t, i) => [t, solar[i] || 0]),
          lineStyle: { color: '#f59e0b', width: 1.75 },
          areaStyle: { color: 'rgba(245,158,11,0.09)' },
        },
        {
          name: 'Battery',
          type: 'line',
          smooth: 0.2,
          sampling: 'lttb',
          showSymbol: false,
          data: timeData.map((t, i) => [t, battery[i] || 0]),
          lineStyle: { color: '#22c55e', width: 1.75 },
          areaStyle: { color: 'rgba(34,197,94,0.09)' },
        },
        {
          name: 'Setpoint',
          type: 'line',
          smooth: 0.2,
          sampling: 'lttb',
          showSymbol: false,
          data: timeData.map((t, i) => [t, setpoint[i] || 0]),
          lineStyle: { color: '#06b6d4', width: 1.5, type: 'dashed' },
          areaStyle: { opacity: 0 },
        },
      ],
    }
  }

  function forceUpdateChart() {
    updateChartOption(true)
  }

  return { chartOption, forceUpdateChart, setChartPaused }
}

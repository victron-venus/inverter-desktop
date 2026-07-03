import { type Ref, ref } from 'vue'

const MAX_HISTORY_POINTS = 1800
const CHART_UPDATE_INTERVAL_MS = 1000

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

export function setChartUpdateCallback(cb: () => void) {
  chartUpdateCallback = cb
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
    // Trigger chart update
    if (chartUpdateCallback) chartUpdateCallback()
  }
}

export function useChart(isDarkRef: Ref<boolean>) {
  const chartOption = ref({})
  let lastChartUpdate = 0

  // Register callback so addHistoryPoint triggers chart updates
  setChartUpdateCallback(() => updateChartOption(false))

  function updateChartOption(force: boolean) {
    const now = Date.now()
    if (!force && now - lastChartUpdate < CHART_UPDATE_INTERVAL_MS) return
    lastChartUpdate = now

    const { timestamps, grid, solar, battery, setpoint } = historyData
    const dark = isDarkRef.value
    const textColor = dark ? '#e0e0e0' : '#333'
    const gridColor = dark ? '#444' : '#e0e0e0'
    const timeData = timestamps.map((ts) => ts * 1000)

    chartOption.value = {
      animation: false,
      backgroundColor: 'transparent',
      tooltip: {
        trigger: 'axis',
        backgroundColor: dark ? '#242424' : '#fff',
        borderColor: dark ? '#444' : '#ccc',
        axisPointer: { type: 'cross', label: { backgroundColor: '#6a7985' } },
        textStyle: { color: textColor, fontSize: 10 },
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
        textStyle: { color: textColor, fontSize: 10 },
      },
      grid: { top: 25, bottom: 25, left: 40, right: 10, containLabel: false },
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
          smooth: true,
          showSymbol: false,
          data: timeData.map((t, i) => [t, grid[i] || 0]),
          lineStyle: { color: '#2196f3', width: 2 },
          areaStyle: { color: 'rgba(33,150,243,0.1)' },
        },
        {
          name: 'Solar',
          type: 'line',
          smooth: true,
          showSymbol: false,
          data: timeData.map((t, i) => [t, solar[i] || 0]),
          lineStyle: { color: '#ff9800', width: 2 },
          areaStyle: { color: 'rgba(255,152,0,0.1)' },
        },
        {
          name: 'Battery',
          type: 'line',
          smooth: true,
          showSymbol: false,
          data: timeData.map((t, i) => [t, battery[i] || 0]),
          lineStyle: { color: '#4caf50', width: 2 },
          areaStyle: { color: 'rgba(76,175,80,0.1)' },
        },
        {
          name: 'Setpoint',
          type: 'line',
          smooth: true,
          showSymbol: false,
          data: timeData.map((t, i) => [t, setpoint[i] || 0]),
          lineStyle: { color: '#00bcd4', width: 2, type: 'dashed' },
          areaStyle: { opacity: 0 },
        },
      ],
    }
  }

  function forceUpdateChart() {
    updateChartOption(true)
  }

  return { chartOption, forceUpdateChart }
}

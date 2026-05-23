import { ref } from 'vue'
import { state } from './useInverterState'

const MAX_HISTORY_POINTS = 1800

interface TooltipParam {
  value: number[]
  seriesName: string
  color: string
}

export function useChart() {
  const chartOption = ref({})

  let historyData = {
    timestamps: [] as number[],
    grid: [] as number[],
    solar: [] as number[],
    battery: [] as number[],
    setpoint: [] as number[]
  }

  function updateChartOption() {
    const { timestamps, grid, solar, battery, setpoint } = historyData
    // Force light theme colors regardless of system theme
    const textColor = '#333'
    const gridColor = '#e0e0e0'

    const timeData = timestamps.map(ts => ts * 1000)

    chartOption.value = {
      animation: false,
      tooltip: {
        trigger: 'axis',
        axisPointer: { type: 'cross', label: { backgroundColor: '#6a7985' } },
        formatter: function(params: TooltipParam[]) {
          const date = new Date(params[0].value[0])
          const timeStr = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
          let result = `${timeStr}<br/>`
          params.forEach((p: TooltipParam) => {
            if (p.seriesName === 'Setpoint') return
            const val = Math.floor(p.value[1])
            result += `<span style="display:inline-block;margin-right:5px;border-radius:10px;width:10px;height:10px;background-color:${p.color};"></span>`
            result += `${p.seriesName}: ${val >= 1000 ? (val/1000).toFixed(1) + 'kW' : val + 'W'}<br/>`
          })
          return result
        }
      },
      legend: {
        data: ['Grid', 'Solar', 'Battery', 'Setpoint'],
        top: 0,
        itemWidth: 12,
        itemHeight: 8,
        textStyle: { color: textColor, fontSize: 10 }
      },
      grid: { top: 25, bottom: 25, left: 40, right: 10, containLabel: false },
      xAxis: {
        type: 'time',
        axisLine: { lineStyle: { color: gridColor } },
        axisLabel: { color: textColor, fontSize: 10, formatter: '{HH}:{mm}' },
        splitLine: { show: false }
      },
      yAxis: {
        type: 'value',
        splitLine: { lineStyle: { color: gridColor, type: 'dashed' } },
        axisLabel: { color: textColor, fontSize: 10, formatter: (v: number) => v >= 1000 ? (v/1000).toFixed(1) + 'k' : v }
      },
      series: [
        {
          name: 'Grid', type: 'line', smooth: true, showSymbol: false,
          data: timeData.map((t, i) => [t, grid[i] || 0]),
          lineStyle: { color: '#2196f3', width: 2 },
          areaStyle: { color: 'rgba(33,150,243,0.1)' }
        },
        {
          name: 'Solar', type: 'line', smooth: true, showSymbol: false,
          data: timeData.map((t, i) => [t, solar[i] || 0]),
          lineStyle: { color: '#ff9800', width: 2 },
          areaStyle: { color: 'rgba(255,152,0,0.1)' }
        },
        {
          name: 'Battery', type: 'line', smooth: true, showSymbol: false,
          data: timeData.map((t, i) => [t, battery[i] || 0]),
          lineStyle: { color: '#4caf50', width: 2 },
          areaStyle: { color: 'rgba(76,175,80,0.1)' }
        },
        {
          name: 'Setpoint', type: 'line', smooth: true, showSymbol: false,
          data: timeData.map((t, i) => [t, setpoint[i] || 0]),
          lineStyle: { color: '#00bcd4', width: 2, type: 'dashed' },
          areaStyle: { opacity: 0 }
        }
      ]
    }
  }

  function addHistoryPoint(newState: typeof state.value) {
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
      updateChartOption()
    }
  }

  return { chartOption, addHistoryPoint }
}

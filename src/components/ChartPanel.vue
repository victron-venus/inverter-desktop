<template>
  <div class="classic-card h-full overflow-hidden bg-white">
    <VChart
      class="chart-wrap w-full"
      :option="chartOption"
      :autoresize="true"
    />
  </div>
</template>

<script setup lang="ts">
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { LineChart } from 'echarts/charts'
import {
  TitleComponent,
  TooltipComponent,
  GridComponent,
  LegendComponent,
} from 'echarts/components'
import VChart from 'vue-echarts'

// Register ECharts components
use([
  CanvasRenderer,
  LineChart,
  TitleComponent,
  TooltipComponent,
  GridComponent,
  LegendComponent,
])

defineProps<{
  chartOption: any
}>()
</script>

<style scoped>
.chart-wrap {
  min-height: 250px; /* Reduced height to match old versions and improve density */
}
</style>

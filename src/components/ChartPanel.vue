<template>
  <div class="classic-card h-full overflow-hidden flex flex-col">
    <VChart
      class="chart-wrap w-full flex-1"
      :option="chartOption"
      :autoresize="{ throttle: 300 }"
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

use([CanvasRenderer, LineChart, TitleComponent, TooltipComponent, GridComponent, LegendComponent])

defineProps<{
  chartOption: Record<string, unknown>
}>()
</script>

<style scoped>
.chart-wrap {
  min-height: 250px;
  padding: 2px 2px 0;
}
</style>

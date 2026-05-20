<template>
  <div class="card"><div class="card-body py-1">
    <v-chart
      class="chart-wrap"
      :option="chartOption"
      :autoresize="true"
    />
  </div></div>
</template>

<script setup lang="ts">
import VChart from 'vue-echarts'
import * as echarts from 'echarts'
VChart.inst = echarts

defineProps<{
  chartOption: any
}>()
</script>

<template>
  <div class="daily-stats mb-2">
    <template v-if="hasSolar">
      <span class="highlight">☀️ {{ prod }}kWh</span>
      <span class="detail">{{ solarStr }}</span>
      <span v-if="hasDollars" class="money">(${{ dollars }})</span>
    </template>
    <template v-if="hasGrid">
      <template v-if="hasSolar">&nbsp;</template>
      Grid: {{ grid }}kWh <span class="money">(${{ gridCost }})</span>
    </template>
    <template v-if="hasBattery">
      <template v-if="hasSolar || hasGrid">&nbsp;|&nbsp;</template>
      🔋 I: {{ batIn }}kWh <span class="dim">({{ batInY }})</span>,
      O: {{ batOut }}kWh <span class="dim">({{ batOutY }})</span>;
      Δ: {{ batDelta }}kWh <span class="dim">({{ batDeltaY }})</span>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { state } from '../composables/useInverterState'
import { escapeHtml } from '../utils'

const GRID_COST_PER_KWH = 0.31

const ds = computed(() => state.value.daily_stats || {})

const prod = computed(() => (ds.value.produced_today || 0).toFixed(2))
const dollars = computed(() => (ds.value.produced_dollars || 0).toFixed(2))
const grid = computed(() => (ds.value.grid_kwh || 0).toFixed(2))
const gridCost = computed(() => (Number.parseFloat(grid.value) * GRID_COST_PER_KWH).toFixed(2))
const batIn = computed(() => (ds.value.battery_in || 0).toFixed(2))
const batOut = computed(() => (ds.value.battery_out || 0).toFixed(2))
const batInY = computed(() => (ds.value.battery_in_yesterday || 0).toFixed(1))
const batOutY = computed(() => (ds.value.battery_out_yesterday || 0).toFixed(1))
const batDelta = computed(() => (Number.parseFloat(batIn.value) - Number.parseFloat(batOut.value)).toFixed(2))
const batDeltaY = computed(() => (Number.parseFloat(batInY.value) - Number.parseFloat(batOutY.value)).toFixed(1))

const tasmotaDaily = computed(() => ds.value.tasmota_daily || [])
const mpptDaily = computed(() => ds.value.mppt_daily || [])
const pvTotalDaily = computed(() => ds.value.pv_total_daily || 0)

const solarStr = computed(() => {
  const parts: string[] = []
  tasmotaDaily.value.forEach(v => { if (v > 0) parts.push(v.toFixed(2)) })
  parts.push(pvTotalDaily.value.toFixed(2) + '(' + mpptDaily.value.map(v => v.toFixed(2)).join('+') + ')')
  return escapeHtml(parts.join('+'))
})

const hasSolar = computed(() => Number.parseFloat(prod.value) > 0)
const hasDollars = computed(() => Number.parseFloat(dollars.value) > 0)
const hasGrid = computed(() => Number.parseFloat(grid.value) > 0)
const hasBattery = computed(() => Number.parseFloat(batIn.value) > 0 || Number.parseFloat(batOut.value) > 0)
</script>

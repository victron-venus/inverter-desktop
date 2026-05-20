<template>
  <div class="row g-2 mb-2">
    <div class="col-md-2">
      <div class="card h-100"><div class="card-body text-center">
        <div class="stat-label">Grid</div>
        <div class="stat-value text-grid">{{ formatPower(gt) }}</div>
        <div class="stat-sub">{{ formatPower(g1) }} | {{ formatPower(g2) }}</div>
      </div></div>
    </div>
    <div class="col-md-2">
      <div class="card h-100"><div class="card-body text-center">
        <div class="stat-label">Consumption</div>
        <div class="stat-value text-consumption">{{ formatPower(tt) }}</div>
        <div class="stat-sub">{{ formatPower(t1) }} | {{ formatPower(t2) }}</div>
      </div></div>
    </div>
    <div class="col-md-3">
      <div class="card h-100"><div class="card-body text-center">
        <div class="stat-label">Solar</div>
        <div class="stat-value text-solar">{{ formatPower(solarTotal) }}</div>
        <div class="stat-sub">{{ formatPower(mpptTotal) }} | {{ formatPower(tasmotaTotal) }}</div>
      </div></div>
    </div>
    <div class="col-md-3">
      <div class="card h-100"><div class="card-body text-center">
        <div class="stat-label">Battery (Shunt)</div>
        <div class="stat-value text-battery">{{ Math.floor(batterySoc || 0) }}%</div>
        <div class="stat-sub">{{ formatPower(batteryPower) }} | {{ (batteryVoltage || 0).toFixed(2) }}V | {{ (batteryCurrent || 0).toFixed(1) }}A</div>
      </div></div>
    </div>
    <div class="col-md-2">
      <div class="card h-100"><div class="card-body text-center">
        <div class="stat-label">Setpoint</div>
        <div class="stat-value text-accent">{{ formatPower(setpoint) }}</div>
        <div class="stat-sub">{{ inverterState || '--' }}</div>
      </div></div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { formatPower } from '../utils'

defineProps<{
  gt?: number
  g1?: number
  g2?: number
  tt?: number
  t1?: number
  t2?: number
  solarTotal?: number
  mpptTotal: number
  tasmotaTotal: number
  batterySoc?: number
  batteryPower?: number
  batteryVoltage?: number
  batteryCurrent?: number
  setpoint?: number
  inverterState?: string
}>()
</script>

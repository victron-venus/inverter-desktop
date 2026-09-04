<template>
  <div class="grid grid-cols-2 md:grid-cols-5 gap-1.5">
    <!-- Grid -->
    <div class="classic-card px-1.5 py-1.5 flex flex-col items-center justify-center min-h-[76px]">
      <div class="classic-stat-label">Grid</div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-grid">
        {{ formatPower(gt) }}
      </div>
      <div class="classic-stat-meta mt-0.5">
        {{ formatPower(g1) }} <span class="opacity-30 mx-0.5">·</span> {{ formatPower(g2) }}
      </div>
    </div>

    <!-- Consumption -->
    <div class="classic-card px-1.5 py-1.5 flex flex-col items-center justify-center min-h-[76px]">
      <div class="classic-stat-label">Consumption</div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-consumption">
        {{ formatPower(tt) }}
      </div>
      <div class="classic-stat-meta mt-0.5">
        {{ formatPower(t1) }} <span class="opacity-30 mx-0.5">·</span> {{ formatPower(t2) }}
      </div>
    </div>

    <!-- Solar -->
    <div class="classic-card px-1.5 py-1.5 flex flex-col items-center justify-center min-h-[76px]">
      <div class="classic-stat-label">Solar</div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-solar">
        {{ formatPower(solarTotal) }}
      </div>
      <div class="classic-stat-meta mt-0.5">
        {{ formatPower(mpptTotal) }} <span class="opacity-30 mx-0.5">·</span>
        {{ formatPower(pvInvertersTotal) }}
      </div>
    </div>

    <!-- Battery -->
    <div class="classic-card px-1.5 py-1.5 flex flex-col items-center justify-center min-h-[76px]">
      <div class="classic-stat-label">Battery</div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-battery">
        {{ Math.floor(batterySoc || 0) }}%
      </div>
      <div class="classic-stat-meta mt-0.5 truncate w-full text-center">
        {{ formatPower(batteryPower) }} <span class="opacity-30 mx-0.5">·</span>
        {{ (batteryVoltage || 0).toFixed(2) }}V <span class="opacity-30 mx-0.5">·</span>
        {{ (batteryCurrent || 0).toFixed(1) }}A
      </div>
    </div>

    <!-- Setpoint -->
    <div class="classic-card px-1.5 py-1.5 flex flex-col items-center justify-center min-h-[76px]">
      <div class="classic-stat-label">Setpoint</div>
      <div class="classic-stat-value text-[1.75rem] font-bold text-accent">
        {{ formatPower(setpoint) }}
      </div>
      <div class="classic-stat-meta mt-0.5 truncate w-full text-center">
        {{ formatInverterState(inverterState) }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { formatInverterState, formatPower } from '../utils'

defineProps<{
  gt?: number
  g1?: number
  g2?: number
  tt?: number
  t1?: number
  t2?: number
  solarTotal?: number
  mpptTotal: number
  pvInvertersTotal: number
  batterySoc?: number
  batteryPower?: number
  batteryVoltage?: number
  batteryCurrent?: number
  setpoint?: number
  inverterState?: string
}>()
</script>

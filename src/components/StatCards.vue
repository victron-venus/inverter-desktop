<template>
  <div class="grid grid-cols-2 md:grid-cols-5 gap-1.5 mb-0.5">
    <!-- Grid -->
    <div class="classic-card p-1.5 flex flex-col items-center justify-center min-h-[75px]">
      <div class="text-[10px] uppercase tracking-wider text-slate-600 dark:text-slate-200 font-bold mb-0.5">Grid</div>
      <div class="text-3xl font-bold text-grid leading-none tracking-tight">{{ formatPower(gt) }}</div>
      <div class="text-[11px] text-slate-500 dark:text-slate-300 font-bold">
        {{ formatPower(g1) }} <span class="opacity-30 mx-0.5">|</span> {{ formatPower(g2) }}
      </div>
    </div>

    <!-- Consumption -->
    <div class="classic-card p-1.5 flex flex-col items-center justify-center min-h-[75px]">
      <div class="text-[10px] uppercase tracking-wider text-slate-600 dark:text-slate-200 font-bold mb-0.5">Consumption</div>
      <div class="text-3xl font-bold text-consumption leading-none tracking-tight">{{ formatPower(tt) }}</div>
      <div class="text-[11px] text-slate-500 dark:text-slate-300 font-bold">
        {{ formatPower(t1) }} <span class="opacity-30 mx-0.5">|</span> {{ formatPower(t2) }}
      </div>
    </div>

    <!-- Solar -->
    <div class="classic-card p-1.5 flex flex-col items-center justify-center min-h-[75px]">
      <div class="text-[10px] uppercase tracking-wider text-slate-600 dark:text-slate-200 font-bold mb-0.5">Solar</div>
      <div class="text-3xl font-bold text-solar leading-none tracking-tight">{{ formatPower(solarTotal) }}</div>
      <div class="text-[11px] text-slate-500 dark:text-slate-300 font-bold">
        {{ formatPower(mpptTotal) }} <span class="opacity-30 mx-0.5">|</span> {{ formatPower(tasmotaTotal) }}
      </div>
    </div>

    <!-- Battery -->
    <div class="classic-card p-1.5 flex flex-col items-center justify-center min-h-[75px]">
      <div class="text-[10px] uppercase tracking-wider text-slate-600 dark:text-slate-200 font-bold mb-0.5">Battery</div>
      <div class="text-3xl font-bold text-battery leading-none tracking-tight">{{ Math.floor(batterySoc || 0) }}%</div>
      <div class="text-[11px] text-slate-500 dark:text-slate-300 font-bold truncate w-full text-center">
        {{ formatPower(batteryPower) }} <span class="opacity-30 mx-0.5">|</span> {{ (batteryVoltage || 0).toFixed(2) }}V <span class="opacity-30 mx-0.5">|</span> {{ (batteryCurrent || 0).toFixed(1) }}A
      </div>
    </div>

    <!-- Setpoint -->
    <div class="classic-card p-1.5 flex flex-col items-center justify-center min-h-[75px]">
      <div class="text-[10px] uppercase tracking-wider text-slate-600 dark:text-slate-200 font-bold mb-0.5">Setpoint</div>
      <div class="text-3xl font-bold text-accent leading-none tracking-tight">{{ formatPower(setpoint) }}</div>
      <div class="text-[11px] text-slate-500 dark:text-slate-300 font-bold truncate w-full text-center uppercase tracking-tighter">
        {{ inverterState || 'Bulk' }}
      </div>
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

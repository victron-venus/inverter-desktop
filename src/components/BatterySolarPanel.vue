<template>
  <div class="grid grid-cols-1 md:grid-cols-2 gap-1.5 mb-1">
    <!-- Batteries Section -->
    <div class="classic-card">
      <div class="classic-header flex items-center gap-1.5">
        <BatteryMedium :size="10" /> Batteries
      </div>
      <div class="p-1 flex flex-wrap gap-x-3 gap-y-1.5">
        <div v-for="bat in batteries" :key="bat.name" class="flex-1 min-w-[130px] border border-slate-100 dark:border-slate-800 p-1 rounded-sm">
          <div class="text-[10px] font-bold text-slate-700 dark:text-slate-300 uppercase tracking-tighter">{{ bat.name }}</div>
          <div class="flex justify-between items-baseline gap-1 mt-0.5">
            <span class="text-[12px] font-bold text-slate-900 dark:text-slate-100 leading-none">{{ bat.voltage.toFixed(2) }}V</span>
            <span v-if="bat.current !== undefined" class="text-[11px] font-medium text-slate-600 dark:text-slate-400 leading-none">{{ bat.current.toFixed(1) }}A</span>
            <span v-if="bat.power !== undefined" class="text-[11px] font-bold text-slate-600 dark:text-slate-400 leading-none">{{ Math.floor(bat.power) }}W</span>
          </div>
          <div class="flex justify-between items-center mt-1 pt-1 border-t border-slate-50 dark:border-slate-800/30">
            <span class="text-[12px] font-bold leading-none" :class="bat.soc > 50 ? 'text-battery' : bat.soc > 20 ? 'text-orange-500' : 'text-red-500'">
              {{ bat.soc.toFixed(1) }}%
            </span>
            <span class="text-[10px] text-slate-500 font-medium truncate max-w-[80px] uppercase">
              {{ bat.state }}<span v-if="bat.timeToGo"> · {{ bat.timeToGo }}</span>
            </span>
          </div>
        </div>
      </div>
    </div>

    <!-- Solar Production Section -->
    <div class="classic-card">
      <div class="classic-header flex items-center gap-1.5">
        <SunMedium :size="10" /> Solar Production
      </div>
      <div class="p-1 flex flex-wrap gap-x-2 gap-y-1.5">
        <div v-for="src in solarSources" :key="src.name" class="flex-1 min-w-[90px] border border-slate-50 dark:border-slate-800/30 p-1 rounded-sm">
          <div class="text-[10px] font-bold text-slate-700 dark:text-slate-300 uppercase tracking-tighter">{{ src.name }}</div>
          <div class="flex flex-col">
            <div class="flex justify-between items-baseline">
              <span v-if="src.pvVoltage" class="text-[10px] font-bold text-solar opacity-80">{{ src.pvVoltage.toFixed(2) }}V</span>
              <span v-if="src.current" class="text-[10px] font-medium text-slate-600 dark:text-slate-400">{{ src.current.toFixed(1) }}A</span>
            </div>
            <div class="text-xl font-bold text-solar leading-none mt-0.5">{{ Math.floor(src.power) }}W</div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { BatteryMedium, SunMedium } from 'lucide-vue-next'

defineProps<{
  batteries: Array<{ name: string; voltage: number; current?: number; power?: number; soc: number; state: string; timeToGo?: string }>
  solarSources: Array<{ name: string; pvVoltage?: number; current?: number; power: number }>
}>()
</script>

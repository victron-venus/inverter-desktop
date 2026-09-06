<template>
  <div class="grid grid-cols-1 md:grid-cols-2 gap-1.5">
    <!-- Batteries Section -->
    <div v-if="showBatteries !== false" class="classic-card">
      <div class="classic-header"><BatteryMedium :size="10" /> Batteries</div>
      <div class="p-1.5 flex flex-wrap gap-x-2 gap-y-1.5">
        <div
          v-for="(bat, bi) in batteries"
          :key="bat.serial ?? bat.instance ?? `${bi}-${bat.name}`"
          class="classic-inset flex-1 min-w-[130px]"
          :title="bat.name"
        >
          <div class="text-[10px] font-semibold text-main tracking-tight truncate">
            {{ bat.name }}
          </div>
          <div class="flex justify-between items-baseline gap-1 mt-0.5 tabular">
            <span class="text-[12px] text-muted leading-none">{{ bat.voltage.toFixed(2) }}V</span>
            <span
              v-if="bat.current != null"
              class="text-[11px] font-semibold text-muted leading-none"
              >{{ bat.current.toFixed(1) }}A</span
            >
            <span v-if="bat.power != null" class="text-[11px] font-semibold text-muted leading-none"
              >{{ Math.floor(bat.power) }}W</span
            >
          </div>
          <div
            class="flex justify-between items-center mt-1 pt-1 border-t border-black/[0.04] dark:border-white/[0.06]"
          >
            <span
              class="text-[12px] font-bold leading-none shrink-0 tabular"
              :class="
                bat.soc > 50 ? 'text-battery' : bat.soc > 20 ? 'text-orange-500' : 'text-red-500'
              "
            >
              {{ bat.soc.toFixed(1) }}%
            </span>
            <span class="text-[10px] text-muted font-medium truncate ml-2 text-right flex-1">
              {{ bat.state }}<span v-if="bat.timeToGo"> · {{ bat.timeToGo }}</span>
            </span>
          </div>
        </div>
      </div>
    </div>

    <!-- Solar Production Section -->
    <div v-if="showSolar !== false" class="classic-card">
      <div class="classic-header"><SunMedium :size="10" /> Solar Production</div>
      <div class="p-1.5 flex flex-wrap gap-x-2 gap-y-1.5">
        <div
          v-for="(src, si) in solarSources"
          :key="src.serial ?? src.instance ?? `${si}-${src.name}`"
          class="classic-inset flex-1 min-w-[90px]"
          :title="src.name"
        >
          <div class="text-[10px] font-semibold text-main tracking-tight truncate">
            {{ src.name }}
          </div>
          <div class="flex flex-col">
            <div class="flex justify-between items-baseline tabular">
              <span v-if="src.pvVoltage" class="text-[10px] font-semibold text-solar opacity-85"
                >{{ src.pvVoltage.toFixed(2) }}V</span
              >
              <span v-if="src.current" class="text-[10px] font-medium text-muted"
                >{{ src.current.toFixed(1) }}A</span
              >
            </div>
            <div class="text-xl font-bold text-solar leading-none mt-0.5 tabular tracking-tight">
              {{ Math.floor(src.power) }}W
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { BatteryMedium, SunMedium } from '@lucide/vue'

defineProps<{
  batteries: Array<{
    name: string
    serial?: string
    instance?: number
    voltage: number
    current?: number
    power?: number
    soc: number
    state: string
    timeToGo?: string
  }>
  solarSources: Array<{
    name: string
    serial?: string
    instance?: number
    pvVoltage?: number
    current?: number
    power: number
  }>
  showBatteries?: boolean
  showSolar?: boolean
}>()
</script>

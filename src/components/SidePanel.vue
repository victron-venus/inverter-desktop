<template>
  <div class="flex flex-col gap-1 h-full">
    <!-- EV Section -->
    <div v-if="features?.ev !== false" class="classic-card">
      <div class="classic-header flex items-center gap-1.5"><Car :size="10" /> EV</div>
      <div class="p-1 flex justify-between items-center gap-2">
        <div v-if="parseFloat(evCharging) > 0">
          <div class="text-xl font-bold text-solar leading-none">{{ evCharging }}</div>
          <div class="text-[10px] text-slate-500 font-bold text-center">Charging</div>
        </div>
        <div class="text-center" v-if="parseFloat(evPower) > 0">
          <div class="text-xl font-bold text-slate-500 leading-none">{{ evPower }}</div>
          <div class="text-[10px] text-slate-500 font-bold tracking-tighter uppercase">VUE</div>
        </div>
        <div class="text-right">
          <div class="text-2xl font-bold text-accent leading-none">
            {{ Math.floor(carSoc || 0) }}%
          </div>
          <div class="text-[10px] text-slate-500 font-bold text-center uppercase tracking-tighter">
            SoC
          </div>
        </div>
      </div>
    </div>

    <!-- Water Section -->
    <div v-if="features?.water !== false" class="classic-card">
      <div class="classic-header flex items-center gap-1.5"><Droplets :size="10" /> Water</div>
      <div class="p-1 flex justify-between items-center gap-2 px-2">
        <div class="text-xl font-bold" :class="waterValve ? 'text-red-500' : 'text-green-500'">
          {{ waterLevel || 0 }} cm
        </div>
        <div class="flex gap-1">
          <button
            class="classic-btn"
            :class="{ 'classic-btn-on': pumpSwitch }"
            @click="$emit('send', 'toggle', { entity: pumpSwitchEntity })"
          >
            PUMP
          </button>
          <button
            class="classic-btn"
            :class="{ 'classic-btn-on': waterValve }"
            @click="$emit('send', 'toggle', { entity: waterValveEntity })"
          >
            VALVE
          </button>
        </div>
      </div>
    </div>

    <!-- Home Controls -->
    <div
      v-if="features?.ha !== false && homeButtons.length > 0"
      class="classic-card flex-1 min-h-0"
    >
      <div class="classic-header flex items-center gap-1.5"><HomeIcon :size="10" /> Home</div>
      <div class="p-1 flex flex-wrap gap-0.5 overflow-y-auto max-h-[300px]">
        <button
          v-for="btn in homeButtons"
          :key="btn.id"
          class="classic-btn !flex-1 !min-w-[50px] !normal-case"
          :class="{ 'classic-btn-on': buttonStates[btn.id] === 'on' }"
          @click="$emit('send', 'toggle', { entity: btn.entity })"
        >
          {{ btn.label }}
        </button>
      </div>
    </div>

    <!-- Appliances -->
    <div
      v-if="
        dishwasherRunning ||
        (washerTime || 0) > 0 ||
        (dryerTime || 0) > 0 ||
        washerPower ||
        dryerPower
      "
      class="flex flex-col gap-0.5"
    >
      <div
        v-if="dishwasherRunning"
        class="classic-card px-2 py-0.5 flex justify-between items-center"
      >
        <span class="text-[10px] font-bold text-slate-500 uppercase tracking-tighter"
          >Dishwasher</span
        >
        <div class="flex items-center gap-1.5">
          <span class="text-[10px] font-bold text-green-600 uppercase tracking-tighter"
            >Running</span
          >
          <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
            formatDuration(dishwasherDuration)
          }}</span>
        </div>
      </div>

      <div
        v-if="(washerTime || 0) > 0 || washerPower"
        class="classic-card px-2 py-0.5 flex justify-between items-center"
      >
        <span class="text-[10px] font-bold text-slate-500 uppercase tracking-tighter">Washer</span>
        <div class="flex items-center gap-1.5">
          <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
            formatDuration(washerTime)
          }}</span>
          <div
            class="w-1.5 h-1.5 rounded-full"
            :class="washerPower ? 'bg-green-500' : 'bg-slate-200'"
          ></div>
        </div>
      </div>

      <div
        v-if="(dryerTime || 0) > 0 || dryerPower"
        class="classic-card px-2 py-0.5 flex justify-between items-center"
      >
        <span class="text-[10px] font-bold text-slate-500 uppercase tracking-tighter">Dryer</span>
        <div class="flex items-center gap-1.5">
          <span class="text-[11px] font-bold text-slate-700 dark:text-slate-300">{{
            formatDuration(dryerTime)
          }}</span>
          <div
            class="w-1.5 h-1.5 rounded-full"
            :class="dryerPower ? 'bg-green-500' : 'bg-slate-200'"
          ></div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Car, Droplets, Utensils, WashingMachine, Wind, Home as HomeIcon } from 'lucide-vue-next'
import { formatDuration } from '../utils'

defineProps<{
  features?: Record<string, boolean>
  evCharging: string
  evPower: string
  carSoc?: number
  waterLevel?: number
  waterValve?: boolean
  pumpSwitch?: boolean
  pumpSwitchEntity: string
  waterValveEntity: string
  dishwasherRunning?: boolean
  dishwasherDuration?: number
  washerTime?: number
  washerPower?: boolean
  dryerTime?: number
  dryerPower?: boolean
  homeButtons: Array<{ id: string; label: string; entity: string }>
  buttonStates: Record<string, string>
}>()

defineEmits<{
  send: [action: string, payload?: any]
}>()
</script>

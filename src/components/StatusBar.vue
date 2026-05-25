<template>
  <div class="flex items-center justify-center gap-2 text-[11px] font-medium text-slate-400 mt-1.5 pb-1">
    <div class="flex items-center gap-1">
      <div class="w-2.5 h-2.5 rounded-full shadow-inner transition-colors" :class="haConnected ? 'bg-green-500' : 'bg-slate-300 dark:bg-slate-700'"></div>
      <span>HA</span>
    </div>
    
    <span class="opacity-30 mx-0.5">|</span>
    
    <div class="flex items-center gap-1">
      <span class="text-slate-400 dark:text-slate-500">Uptime:</span>
      <span class="text-slate-600 dark:text-white">{{ formatUptime(uptime || 0) }}</span>
    </div>

    <span class="opacity-30 mx-0.5">|</span>

    <div class="flex items-center gap-1">
      <div class="w-2.5 h-2.5 rounded-full shadow-inner transition-colors" :class="mqttConnected ? 'bg-green-500' : 'bg-slate-300 dark:bg-slate-700'"></div>
      <span class="text-slate-600 dark:text-white">MQTT</span>
    </div>

    <span class="opacity-30 mx-0.5">|</span>
    <span class="text-slate-400 dark:text-slate-500 font-medium">Desktop {{ appVersion }}</span>

    <span v-if="stateVersion" class="opacity-30 mx-0.5">|</span>
    <span v-if="stateVersion" class="text-slate-400 dark:text-slate-500 font-medium">Control {{ stateVersion }}</span>
  </div>
</template>

<script setup lang="ts">
import { formatUptime } from '../utils'

defineProps<{
  haEnabled: boolean
  haConnected: boolean
  mqttConnected: boolean
  uptime?: number
  appVersion: string
  stateVersion?: string
}>()
</script>

<template>
  <div
    class="flex items-center justify-center gap-2 text-[11px] font-medium text-slate-500 mt-1.5 pb-1"
  >
    <div v-if="haEnabled" class="flex items-center gap-1">
      <div
        class="w-2.5 h-2.5 rounded-full shadow-inner transition-colors ring-1 ring-slate-400 dark:ring-slate-500"
        :class="haConnected ? 'bg-green-400' : 'bg-slate-500 dark:bg-slate-700'"
      ></div>
      <span>{{ $t('status.ha') }}</span>
    </div>

    <span v-if="haEnabled" class="opacity-30 mx-0.5">|</span>

    <div class="flex items-center gap-1">
      <span class="text-slate-500 dark:text-slate-500">{{ $t('status.uptime') }}:</span>
      <span class="text-slate-500 dark:text-white">{{ formatUptime(uptime || 0) }}</span>
    </div>

    <span class="opacity-30 mx-0.5">|</span>

    <div class="flex items-center gap-1">
      <div
        class="w-2.5 h-2.5 rounded-full shadow-inner transition-colors ring-1 ring-slate-400 dark:ring-slate-500"
        :class="mqttConnected ? 'bg-green-400' : 'bg-slate-500 dark:bg-slate-700'"
      ></div>
      <span class="text-slate-500 dark:text-white">{{ $t('status.mqtt') }}</span>
    </div>

    <span v-if="haMqttConnected !== null" class="opacity-30 mx-0.5">|</span>

    <div v-if="haMqttConnected !== null" class="flex items-center gap-1">
      <div
        class="w-2.5 h-2.5 rounded-full shadow-inner transition-colors ring-1 ring-slate-400 dark:ring-slate-500"
        :class="haMqttConnected ? 'bg-green-400' : 'bg-slate-500 dark:bg-slate-700'"
      ></div>
      <span class="text-slate-500 dark:text-white">{{ $t('status.haMqtt') }}</span>
    </div>

    <span class="opacity-30 mx-0.5">|</span>
    <span class="text-slate-500 dark:text-slate-500 font-medium">
      {{ $t('status.desktop') }} {{ appVersion }}
    </span>

    <span v-if="stateVersion" class="opacity-30 mx-0.5">|</span>
    <span v-if="stateVersion" class="text-slate-500 dark:text-slate-500 font-medium">
      {{ $t('status.control') }} {{ stateVersion }}
    </span>

    <NotificationHistory />
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { formatUptime } from '../utils'
import NotificationHistory from './NotificationHistory.vue'

const { t: $t } = useI18n()

defineProps<{
  haEnabled: boolean
  haConnected: boolean
  mqttConnected: boolean
  haMqttConnected?: boolean | null
  uptime?: number
  appVersion: string
  stateVersion?: string
}>()
</script>

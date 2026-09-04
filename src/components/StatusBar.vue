<template>
  <div
    class="flex items-center justify-center gap-2 text-[10px] font-medium text-muted mt-1 pb-0.5"
  >
    <div v-if="haEnabled" class="flex items-center gap-1.5">
      <div class="status-dot" :class="{ 'status-dot-on': haConnected }"></div>
      <span>{{ $t('status.ha') }}</span>
    </div>

    <span v-if="haEnabled" class="soft-divider"></span>

    <div class="flex items-center gap-1">
      <span>{{ $t('status.uptime') }}:</span>
      <span class="text-main tabular">{{ formatUptime(uptime || 0) }}</span>
    </div>

    <span class="soft-divider"></span>

    <div class="flex items-center gap-1.5">
      <div class="status-dot" :class="{ 'status-dot-on': mqttConnected }"></div>
      <span class="text-main">{{ $t('status.mqtt') }}</span>
    </div>

    <span v-if="haMqttConnected !== null" class="soft-divider"></span>

    <div v-if="haMqttConnected !== null" class="flex items-center gap-1.5">
      <div class="status-dot" :class="{ 'status-dot-on': haMqttConnected }"></div>
      <span class="text-main">{{ $t('status.haMqtt') }}</span>
    </div>

    <span class="soft-divider"></span>
    <span> {{ $t('status.desktop') }} {{ appVersion }} </span>

    <span v-if="stateVersion" class="soft-divider"></span>
    <span v-if="stateVersion"> {{ $t('status.control') }} {{ stateVersion }} </span>

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

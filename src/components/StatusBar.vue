<template>
  <div
    class="flex items-center justify-center gap-2 text-[10px] font-medium text-muted mt-1 pb-0.5"
  >
    <HoverTip v-if="haEnabled" :text="$t('status.tipHa')" class="flex items-center gap-1.5">
      <div class="status-dot" :class="{ 'status-dot-on': haConnected }"></div>
      <span>{{ $t('status.ha') }}</span>
    </HoverTip>

    <span v-if="haEnabled" class="soft-divider"></span>

    <div class="flex items-center gap-1">
      <span>{{ $t('status.uptime') }}:</span>
      <span class="text-main tabular">{{ formatUptime(uptime || 0) }}</span>
    </div>

    <span class="soft-divider"></span>

    <HoverTip
      :text="dataSource === 'igw' ? $t('status.tipIgw') : $t('status.tipMqtt')"
      class="flex items-center gap-1.5"
    >
      <div class="status-dot" :class="{ 'status-dot-on': mqttConnected }"></div>
      <span class="text-main">{{
        dataSource === 'igw' ? $t('status.igw') : $t('status.mqtt')
      }}</span>
    </HoverTip>

    <span class="soft-divider"></span>
    <span
      data-testid="telemetry-quality"
      role="status"
      :title="telemetryTitle"
      :class="telemetry.quality === 'live' ? 'text-muted' : 'text-consumption'"
    >
      {{ $t(`status.telemetry${telemetry.quality}`) }}
    </span>

    <span v-if="haMqttConnected !== null" class="soft-divider"></span>

    <HoverTip
      v-if="haMqttConnected !== null"
      :text="$t('status.tipHaMqtt')"
      class="flex items-center gap-1.5"
    >
      <div class="status-dot" :class="{ 'status-dot-on': haMqttConnected }"></div>
      <span class="text-main">{{ $t('status.haMqtt') }}</span>
    </HoverTip>

    <span class="soft-divider"></span>
    <span> {{ $t('status.desktop') }} {{ appVersion }} </span>

    <span v-if="stateVersion" class="soft-divider"></span>
    <span v-if="stateVersion"> {{ $t('status.control') }} {{ stateVersion }} </span>

    <NotificationHistory />
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { computed } from 'vue'
import { telemetry } from '../composables/useInverterState'
import { formatUptime } from '../utils'
import HoverTip from './HoverTip.vue'
import NotificationHistory from './NotificationHistory.vue'

const { t: $t } = useI18n()
const telemetryTitle = computed(() => {
  const observed = telemetry.value.observed_at
  if (observed === null) return $t('status.telemetryunknown')
  return `${telemetry.value.source?.toUpperCase()} · ${$t('status.receivedAt')}: ${new Date(observed).toLocaleString()}`
})

withDefaults(
  defineProps<{
    haEnabled: boolean
    haConnected: boolean
    mqttConnected: boolean
    /** Active live-data transport for the status label. */
    dataSource?: 'mqtt' | 'igw'
    haMqttConnected?: boolean | null
    uptime?: number
    appVersion: string
    stateVersion?: string
  }>(),
  { dataSource: 'mqtt', haMqttConnected: null }
)
</script>

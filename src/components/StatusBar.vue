<template>
  <div class="mt-2 text-center small" style="color:#666">
    <span class="status-dot" :class="haConnected ? 'online' : 'offline'"></span>
    HA {{ haConnected ? '(OK)' : '(Offline)' }}
    &nbsp;|&nbsp;
    <span class="status-dot" :class="mqttConnected ? 'online' : 'offline'"></span>
    MQTT {{ mqttConnected ? 'OK' : (mqttConnected === false ? 'Disconnected' : 'Connecting...') }}
    <br>
    Uptime: {{ formatUptime(uptime || 0) }}
    &nbsp;|&nbsp; {{ stateVersion ? 'Control v' + stateVersion : '' }}
    &nbsp;|&nbsp; App v{{ appVersion }}
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

<template>
  <span v-if="haMqttConnected !== null" class="soft-divider"></span>

  <HoverTip
    v-if="haMqttConnected !== null"
    :text="$t('status.tipHaMqtt')"
    class="flex items-center gap-1.5"
  >
    <div class="status-dot" :class="{ 'status-dot-on': haMqttConnected }"></div>
    <span class="text-main">{{ $t('status.haMqtt') }}</span>
  </HoverTip>
</template>
<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import HoverTip from '../../components/HoverTip.vue'
import { haMqttConnected } from '../../composables/useInverterState'
const { t: $t } = useI18n()
</script>

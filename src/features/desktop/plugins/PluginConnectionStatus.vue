<template>
  <template v-for="status in statuses" :key="status.key">
    <HoverTip :text="status.details" class="flex items-center gap-1.5">
      <div class="status-dot" :class="{ 'status-dot-on': status.connected }"></div>
      <span>{{ status.label }}</span>
    </HoverTip>
    <span class="soft-divider"></span>
  </template>
</template>
<script setup lang="ts">
import { computed } from 'vue'
import HoverTip from '../../../components/HoverTip.vue'
import { usePluginPresentation } from './presentation'
import { connectionStatuses } from './connectionStatuses'
const { dashboard } = usePluginPresentation()
const statuses = computed(() =>
  connectionStatuses(
    dashboard.plugins.value,
    (plugin) => dashboard.canAct(plugin) && !dashboard.configurationUnavailable.value,
    dashboard.configuredPlugins.value
  )
)
</script>

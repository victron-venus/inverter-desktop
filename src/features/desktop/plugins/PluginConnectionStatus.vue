<template>
  <template v-for="{ plugin, item, key } in connections" :key="key">
    <HoverTip :text="item.title" class="flex items-center gap-1.5">
      <div
        class="status-dot"
        :class="{ 'status-dot-on': item.connected && dashboard.canAct(plugin) }"
      ></div>
      <span>{{ item.title }}</span>
    </HoverTip>
    <span class="soft-divider"></span>
  </template>
</template>
<script setup lang="ts">
import HoverTip from '../../../components/HoverTip.vue'
import { usePluginPresentation } from './presentation'
const { connections, dashboard } = usePluginPresentation()
</script>

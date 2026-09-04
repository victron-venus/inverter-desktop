<template>
  <div v-if="loads.length" class="classic-card mb-1 overflow-hidden">
    <div class="classic-header py-0 px-2 flex items-center gap-1.5 h-[22px]">
      <Zap :size="10" /> Active Loads
    </div>
    <div
      class="divide-y divide-slate-50 dark:divide-slate-800/30 max-h-[min(40vh,280px)] overflow-y-auto overscroll-contain"
    >
      <div
        v-for="load in loads"
        :key="load.id || load.name"
        class="flex justify-between items-center px-2 py-0.5 hover:bg-slate-50/50 dark:hover:bg-slate-800/30 transition-colors"
      >
        <span
          class="text-[11px] font-medium text-slate-600 dark:text-slate-400 capitalize tracking-tight"
          >{{ load.name }}</span
        >
        <span class="text-[11px] font-bold" :class="loadColor(load)"
          >{{ Math.floor(load.value) }}W</span
        >
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Zap } from '@lucide/vue'

defineProps<{
  loads: Array<{ id?: string; name: string; value: number; isGeneration?: boolean }>
}>()

function loadColor(load: { name: string; value: number; isGeneration?: boolean }): string {
  if (load.isGeneration) return 'text-green-600'
  if (/(total|balance)/i.test(load.name)) return 'text-red-600'
  return load.value < 0 ? 'text-green-600' : 'text-slate-700 dark:text-slate-300'
}
</script>

<template>
  <div v-if="loads.length" class="classic-card overflow-hidden">
    <div class="classic-header !py-0 !px-2 !h-[22px]"><Zap :size="10" /> Active Loads</div>
    <div
      class="divide-y divide-black/[0.04] dark:divide-white/[0.05] max-h-[min(40vh,280px)] overflow-y-auto overscroll-contain"
    >
      <div
        v-for="load in loads"
        :key="load.id || load.name"
        class="row-hover flex justify-between items-center px-2 py-0.5"
      >
        <span class="text-[11px] font-medium text-muted capitalize tracking-tight">{{
          load.name
        }}</span>
        <span class="text-[11px] font-semibold tabular" :class="loadColor(load)"
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
  if (load.isGeneration) return 'text-battery'
  if (/(total|balance)/i.test(load.name)) return 'text-consumption'
  return load.value < 0 ? 'text-battery' : 'text-main'
}
</script>

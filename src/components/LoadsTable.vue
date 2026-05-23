<template>
  <div v-if="sortedLoads.length" class="classic-card mb-1 overflow-hidden bg-white">
    <div class="classic-header py-0 px-2 flex items-center gap-1.5 h-[22px]">
      <Zap :size="10" /> Active Loads
    </div>
    <div class="divide-y divide-slate-100">
      <div v-for="[name, val] in sortedLoads" :key="name" class="flex justify-between items-center px-2 py-0.5 hover:bg-slate-50 transition-colors">
        <span class="text-[11px] font-medium text-slate-500 capitalize tracking-tight">{{ name.replace(/_/g, ' ') }}</span>
        <span class="text-[11px] font-bold text-slate-700">{{ Math.floor(val) }}W</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Zap } from 'lucide-vue-next'

defineProps<{
  sortedLoads: Array<[string, number]>
}>()
</script>

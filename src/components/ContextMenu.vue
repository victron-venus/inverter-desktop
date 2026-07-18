<template>
  <div
    v-if="show"
    class="fixed z-[100] min-w-[140px] apple-card p-1 shadow-2xl animate-in fade-in zoom-in duration-100"
    :style="{ left: x + 'px', top: y + 'px' }"
    @click.stop
  >
    <button
      type="button"
      class="w-full text-left px-3 py-1.5 rounded-lg text-xs font-semibold flex items-center gap-2 hover:bg-apple-blue hover:text-white transition-colors group"
      @click="$emit('open-config')"
    >
      <Settings :size="14" class="group-hover:text-white transition-colors" />
      Settings
    </button>
  </div>
</template>

<script setup lang="ts">
import { Settings } from '@lucide/vue'

defineProps<{
  show: boolean
  x: number
  y: number
}>()
defineEmits<{
  'open-config': []
}>()
</script>

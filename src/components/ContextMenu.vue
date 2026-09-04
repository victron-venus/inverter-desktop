<template>
  <div
    v-if="show"
    class="fixed z-[100] min-w-[168px] apple-card p-1 animate-in fade-in zoom-in duration-100"
    :style="{ left: x + 'px', top: y + 'px' }"
    @click.stop
  >
    <button
      type="button"
      class="w-full text-left px-2.5 py-1.5 rounded-md text-[12px] font-semibold tracking-tight flex items-center gap-2 text-main hover:bg-black/[0.04] dark:hover:bg-white/[0.06] transition-colors"
      @click="$emit('open-config')"
    >
      <Settings :size="13" class="text-muted" />
      Settings
    </button>

    <div class="my-1 mx-1 h-px bg-black/[0.06] dark:bg-white/[0.08]"></div>

    <button
      type="button"
      class="w-full text-left px-2.5 py-1.5 rounded-md text-[12px] font-semibold tracking-tight flex items-center gap-2 text-main hover:bg-black/[0.04] dark:hover:bg-white/[0.06] transition-colors"
      @click="$emit('check-updates')"
    >
      <Download :size="13" class="text-muted" />
      Check for Updates
    </button>
  </div>
</template>

<script setup lang="ts">
import { Settings, Download } from '@lucide/vue'

defineProps<{
  show: boolean
  x: number
  y: number
}>()
defineEmits<{
  'open-config': []
  'check-updates': []
}>()
</script>

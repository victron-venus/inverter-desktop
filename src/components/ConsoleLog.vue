<template>
  <div v-if="lines.length > 0" class="console-log classic-card mt-1">
    <button
      class="w-full flex items-center justify-between px-2 py-0.5 text-xs font-mono opacity-70 hover:opacity-100 transition-opacity"
      @click="expanded = !expanded"
    >
      <span>Console ({{ lines.length }})</span>
      <span>{{ expanded ? '▼' : '▶' }}</span>
    </button>
    <div
      v-if="expanded"
      ref="scrollEl"
      class="max-h-40 overflow-y-auto px-2 pb-1 font-mono text-[10px] leading-tight space-y-px"
    >
      <div v-for="(line, i) in lines" :key="i" class="text-gray-400 dark:text-gray-500 break-all">
        {{ line }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'

const props = defineProps<{
  lines: string[]
}>()

const expanded = ref(false)
const scrollEl = ref<HTMLElement | null>(null)

watch(
  () => props.lines.length,
  async () => {
    if (expanded.value) {
      await nextTick()
      scrollEl.value?.scrollTo(0, scrollEl.value.scrollHeight)
    }
  }
)
</script>

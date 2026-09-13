<template>
  <section :aria-label="text" v-bind="$attrs" @mouseenter="onEnter" @mouseleave="onLeave">
    <slot />
  </section>
  <!-- WKWebView often ignores native title=; float above overflow:hidden parents -->
  <Teleport to="body">
    <div
      v-if="tip"
      class="pointer-events-none fixed z-[9999] max-w-[min(320px,90vw)] -translate-x-1/2 -translate-y-full rounded-md px-2 py-1 text-[11px] font-semibold leading-snug text-main shadow-lg border border-black/10 dark:border-white/15 bg-white/95 dark:bg-zinc-900/95"
      :style="{ left: `${tip.x}px`, top: `${tip.y}px` }"
    >
      {{ tip.text }}
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref } from 'vue'

defineOptions({ inheritAttrs: false })

const props = defineProps<{
  text: string
}>()

const tip = ref<{ text: string; x: number; y: number } | null>(null)

function onEnter(e: MouseEvent) {
  const name = props.text?.trim()
  if (!name) {
    tip.value = null
    return
  }
  const el = e.currentTarget as HTMLElement
  const r = el.getBoundingClientRect()
  tip.value = {
    text: name,
    x: r.left + r.width / 2,
    y: r.top - 6,
  }
}

function onLeave() {
  tip.value = null
}
</script>

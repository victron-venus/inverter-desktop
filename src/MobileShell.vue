<template>
  <div class="mobile-shell">
    <!-- Keep the dashboard connection and telemetry alive while editing settings. -->
    <div v-show="!settingsOpen" class="h-full"><App /></div>
    <Config v-if="settingsOpen" />
  </div>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import App from './App.vue'
import Config from './Config.vue'
import { logger } from './logger'

const settingsOpen = ref(globalThis.location.hash === '#settings')
const unlisteners: UnlistenFn[] = []
let disposed = false

function syncPage() {
  settingsOpen.value = globalThis.location.hash === '#settings'
}

function openSettings() {
  if (settingsOpen.value) return
  globalThis.history.pushState({ inverterSettings: true }, '', '#settings')
  syncPage()
}

function closeSettings() {
  if (!settingsOpen.value) return
  if (globalThis.history.state?.inverterSettings) {
    // Tauri's Android back dispatcher uses this same WebView history entry.
    globalThis.history.back()
  } else {
    globalThis.history.replaceState(null, '', globalThis.location.pathname)
    syncPage()
  }
}

onMounted(async () => {
  globalThis.addEventListener('popstate', syncPage)
  globalThis.addEventListener('hashchange', syncPage)
  try {
    for (const [event, callback] of [
      ['mobile-open-settings', openSettings],
      ['mobile-close-settings', closeSettings],
    ] as const) {
      const unlisten = await listen(event, callback)
      if (disposed) unlisten()
      else unlisteners.push(unlisten)
    }
  } catch (error) {
    logger.error('Could not initialize mobile navigation:', error)
  }
})

onUnmounted(() => {
  disposed = true
  globalThis.removeEventListener('popstate', syncPage)
  globalThis.removeEventListener('hashchange', syncPage)
  unlisteners.forEach((unlisten) => {
    unlisten()
  })
})
</script>

<style scoped>
.mobile-shell {
  height: 100dvh;
  padding: env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom)
    env(safe-area-inset-left);
  box-sizing: border-box;
  overflow: hidden;
}
.mobile-shell :deep(.app-shell) {
  height: 100%;
}
</style>

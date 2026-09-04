<template>
  <ErrorBoundary>
    <div class="app-shell h-screen flex items-center justify-center p-4 select-none">
      <div class="classic-card p-6 w-full max-w-sm flex flex-col items-center text-center">
        <img
          :class="{ hidden: imageError }"
          @error="onImageError"
          src="/icons/128x128.png"
          width="64"
          height="64"
          class="mb-4 rounded-xl shadow-sm border border-black/5 dark:border-white/10"
          alt="Inverter Desktop"
        />

        <h2 class="text-[15px] font-semibold tracking-tight text-main mb-0.5">Inverter Desktop</h2>
        <div class="classic-label mb-4">Version {{ appVersion }}</div>

        <p class="text-[12px] leading-relaxed text-muted mb-6 px-2">
          Desktop application for monitoring and controlling Victron energy inverter systems via
          MQTT. Integrates with Home Assistant for unified device control.
        </p>

        <button
          type="button"
          @click.prevent="openRepo"
          class="text-[11px] font-semibold text-accent hover:opacity-80 mb-6"
        >
          github.com/victron-venus/inverter-desktop
        </button>

        <UiButton variant="primary" size="lg" class="w-full" @click="closeWindow"> Close </UiButton>
      </div>
    </div>
  </ErrorBoundary>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import ErrorBoundary from './components/ErrorBoundary.vue'
import UiButton from './components/UiButton.vue'
import { getVersion } from '@tauri-apps/api/app'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'

const appVersion = ref('...')
const imageError = ref(false)
function onImageError() {
  imageError.value = true
}

onMounted(async () => {
  try {
    appVersion.value = await getVersion()
  } catch {
    appVersion.value = '1.1.2'
  }
})

async function closeWindow() {
  try {
    const window = await getCurrentWindow()
    await window.close()
  } catch {
    try {
      await invoke('close_config_window')
    } catch {
      // ignore
    }
  }
}

function openRepo() {
  openUrl('https://github.com/victron-venus/inverter-desktop')
}
</script>

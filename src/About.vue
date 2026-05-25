<template>
  <div class="h-screen bg-background-light dark:bg-background-dark text-slate-800 dark:text-slate-200 flex items-center justify-center p-4 select-none font-sans">
    <div class="classic-card p-6 w-full max-w-sm flex flex-col items-center text-center shadow-lg bg-white dark:bg-slate-900">
      <img
        src="/icons/128x128.png"
        width="64" height="64"
        class="mb-4 rounded-xl shadow-sm border border-slate-100 dark:border-slate-800"
        alt="Inverter Dashboard"
        onerror="this.style.display='none'"
      />
      
      <h2 class="text-lg font-bold tracking-tight mb-0.5">Inverter Dashboard</h2>
      <div class="text-[10px] font-bold text-slate-500 uppercase tracking-widest mb-4">Version {{ appVersion }}</div>
      
      <p class="text-[12px] leading-relaxed text-slate-500 dark:text-slate-500 mb-6 px-2">
        Desktop application for monitoring and controlling
        Victron energy inverter systems via MQTT.
        Integrates with Home Assistant for unified device control.
      </p>
      
      <button
        @click.prevent="openRepo"
        class="text-[11px] font-bold text-accent hover:underline mb-6"
      >
        github.com/victron-venus/inverter-desktop
      </button>
      
      <button 
        @click="closeWindow"
        class="classic-btn w-full !normal-case !py-2 !text-xs !classic-btn-on"
      >
        Close
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'

const appVersion = ref('...')

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

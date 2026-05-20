<template>
  <v-app>
    <v-main class="d-flex align-center justify-center pa-6">
      <div class="text-center" style="max-width:340px">
        <img
          src="/icons/128x128.png"
          width="64" height="64"
          class="mb-4"
          alt="Inverter Dashboard"
          onerror="this.style.display='none'"
        />
        <h3 style="font-weight:600">Inverter Dashboard</h3>
        <div class="text-caption mb-4" style="color:#888">v1.0.17</div>
        <p class="mb-3" style="font-size:0.85rem;line-height:1.5;color:#aaa">
          Desktop application for monitoring and controlling
          Victron energy inverter systems via MQTT.
          Integrates with Home Assistant for unified device control,
          providing real-time power flow visualization, solar tracking,
          and automated notifications.
        </p>
        <a
          href="#"
          style="font-size:0.8rem;color:#00d4aa;text-decoration:none"
          @click.prevent="openRepo"
        >
          github.com/victron-venus/inverter-desktop
        </a>
        <div class="mt-4">
          <v-btn size="small" variant="flat" color="primary" @click="closeWindow">
            Close
          </v-btn>
        </div>
      </div>
    </v-main>
  </v-app>
</template>

<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'

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

<style scoped>
.v-application {
  background: #0a0a0a !important;
}
</style>

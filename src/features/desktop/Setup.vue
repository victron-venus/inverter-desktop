<template>
  <!-- Advanced: optional HA -->
  <div class="flex flex-col gap-4">
    <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
      <h3 class="classic-section-title">Home Assistant</h3>
      <p class="text-[10px] text-muted mt-1">Optional — home devices, laundry, covers.</p>
    </header>

    <div class="flex flex-col gap-3 p-3 classic-inset !rounded-lg !p-3">
      <div class="flex flex-col gap-1">
        <label for="setup_ha_url" class="classic-label px-1">URL / IP</label>
        <input
          id="setup_ha_url"
          v-model="fields.ha_url"
          type="text"
          class="classic-input w-full"
          placeholder="http://homeassistant.local"
          autocomplete="off"
        />
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="flex flex-col gap-1">
          <label for="setup_ha_port" class="classic-label px-1">Port</label>
          <input
            id="setup_ha_port"
            v-model.number="fields.ha_port"
            type="number"
            class="classic-input w-full"
            placeholder="8123"
          />
        </div>
        <div class="flex flex-col gap-1">
          <label for="setup_ha_token" class="classic-label px-1">Long-lived Token</label>
          <input
            id="setup_ha_token"
            v-model="fields.ha_longlived_token"
            type="password"
            class="classic-input w-full"
            placeholder="Token"
            autocomplete="new-password"
          />
        </div>
      </div>
      <div class="flex items-center gap-2 pt-1">
        <UiButton class="flex-1" :loading="testingHa" @click="testHaConnection">
          Test HA connection
        </UiButton>
      </div>
      <p
        v-if="haTestResult"
        class="text-[11px] px-1"
        :class="haTestSuccess ? 'text-generation' : 'text-consumption'"
      >
        {{ haTestResult }}
      </p>
    </div>

    <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2 mt-1">
      <h3 class="classic-section-title">HA MQTT</h3>
      <p class="text-[10px] text-muted mt-1">Optional broker for camera / HA events.</p>
    </header>
    <div class="grid grid-cols-2 gap-3">
      <div class="flex flex-col gap-1">
        <label for="setup_mqtt_ha_host" class="classic-label px-1">Host</label>
        <input
          id="setup_mqtt_ha_host"
          v-model="fields.mqtt_ha_host"
          type="text"
          class="classic-input w-full"
          autocomplete="off"
        />
      </div>
      <div class="flex flex-col gap-1">
        <label for="setup_mqtt_ha_port" class="classic-label px-1">Port</label>
        <input
          id="setup_mqtt_ha_port"
          v-model.number="fields.mqtt_ha_port"
          type="number"
          class="classic-input w-full"
        />
      </div>
      <div class="flex flex-col gap-1">
        <label for="setup_mqtt_ha_login" class="classic-label px-1">Username</label>
        <input
          id="setup_mqtt_ha_login"
          v-model="fields.mqtt_ha_login"
          type="text"
          class="classic-input w-full"
          placeholder="Optional"
          autocomplete="off"
        />
      </div>
      <div class="flex flex-col gap-1">
        <label for="setup_mqtt_ha_password" class="classic-label px-1">Password</label>
        <input
          id="setup_mqtt_ha_password"
          v-model="fields.mqtt_ha_password"
          type="password"
          class="classic-input w-full"
          placeholder="Optional"
          autocomplete="new-password"
        />
      </div>
    </div>
  </div>
</template>
<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { reactive, ref } from 'vue'
import type { AppConfig } from '../../config'
import UiButton from '../../components/UiButton.vue'
import { configField } from './configField'
const config = defineModel<AppConfig>('config', { required: true })
const fields = reactive({
  ha_url: configField(config, 'ha_url'),
  ha_port: configField(config, 'ha_port'),
  ha_longlived_token: configField(config, 'ha_longlived_token'),
  mqtt_ha_host: configField(config, 'mqtt_ha_host'),
  mqtt_ha_port: configField(config, 'mqtt_ha_port'),
  mqtt_ha_login: configField(config, 'mqtt_ha_login'),
  mqtt_ha_password: configField(config, 'mqtt_ha_password'),
})
const testingHa = ref(false)
const haTestResult = ref('')
const haTestSuccess = ref(false)
async function testHaConnection() {
  if (!config.value.ha_url || !config.value.ha_longlived_token) {
    haTestResult.value = 'URL and token required'
    haTestSuccess.value = false
    return
  }
  testingHa.value = true
  haTestResult.value = ''
  try {
    await invoke('test_ha_connection', {
      url: config.value.ha_url,
      port: config.value.ha_port ?? null,
      token: config.value.ha_longlived_token,
    })
    haTestResult.value = 'Connection OK'
    haTestSuccess.value = true
  } catch (e) {
    haTestResult.value = `Failed: ${e?.toString() || e}`
    haTestSuccess.value = false
  } finally {
    testingHa.value = false
  }
}
</script>

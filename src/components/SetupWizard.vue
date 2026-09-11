<template>
  <div class="fixed inset-0 z-[200] flex items-center justify-center bg-black/55 backdrop-blur-md">
    <div
      class="w-[min(520px,94vw)] max-h-[min(90vh,720px)] classic-card !rounded-xl shadow-2xl flex flex-col overflow-hidden"
    >
      <div class="px-5 pt-5 pb-3 border-b border-black/[0.06] dark:border-white/[0.07]">
        <div class="flex items-center gap-3">
          <div
            class="w-10 h-10 rounded-full flex items-center justify-center bg-accent/15 text-accent shrink-0"
          >
            <Settings :size="18" />
          </div>
          <div class="min-w-0">
            <h2 class="text-[15px] font-semibold tracking-tight text-main">Welcome — Setup</h2>
            <p class="text-[11px] text-muted leading-relaxed">
              Configure a data source before using Inverter Desktop.
            </p>
          </div>
        </div>

        <div class="flex gap-1 mt-4" role="tablist" aria-label="Setup sections">
          <UiButton
            class="flex-1"
            toggle
            :active="activeTab === 'main'"
            @click="activeTab = 'main'"
          >
            Main
          </UiButton>
          <UiButton
            class="flex-1"
            toggle
            :active="activeTab === 'advanced'"
            @click="activeTab = 'advanced'"
          >
            Advanced
          </UiButton>
        </div>
      </div>

      <div class="flex-1 overflow-y-auto px-5 py-4 bg-[#f7f7f8] dark:bg-[#121214]">
        <!-- Main: pick one path -->
        <div v-if="activeTab === 'main'" class="flex flex-col gap-4">
          <div class="flex gap-1" role="radiogroup" aria-label="Connection mode">
            <UiButton
              class="flex-1"
              toggle
              :active="connectionMode === 'mqtt'"
              @click="selectMqttMode"
            >
              Cerbo MQTT
            </UiButton>
            <UiButton
              class="flex-1"
              toggle
              :active="connectionMode === 'igw'"
              @click="selectIgwMode"
            >
              Remote IGW
            </UiButton>
          </div>

          <div v-if="connectionMode === 'mqtt'" class="flex flex-col gap-3">
            <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
              <h3 class="classic-section-title">Cerbo MQTT</h3>
              <p class="text-[10px] text-muted mt-1">LAN broker on your Victron GX device.</p>
            </header>
            <div class="grid grid-cols-2 gap-3">
              <div class="flex flex-col gap-1">
                <label for="setup_mqtt_host" class="classic-label px-1">Host</label>
                <input
                  id="setup_mqtt_host"
                  v-model="config.mqtt_host"
                  type="text"
                  class="classic-input w-full"
                  placeholder="Cerbo.local"
                  autocomplete="off"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label for="setup_mqtt_port" class="classic-label px-1">Port</label>
                <input
                  id="setup_mqtt_port"
                  v-model.number="config.mqtt_port"
                  type="number"
                  class="classic-input w-full"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label for="setup_mqtt_login" class="classic-label px-1">Username</label>
                <input
                  id="setup_mqtt_login"
                  v-model="config.mqtt_login"
                  type="text"
                  class="classic-input w-full"
                  placeholder="Optional"
                  autocomplete="off"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label for="setup_mqtt_password" class="classic-label px-1">Password</label>
                <input
                  id="setup_mqtt_password"
                  v-model="config.mqtt_password"
                  type="password"
                  class="classic-input w-full"
                  placeholder="Optional"
                  autocomplete="new-password"
                />
              </div>
            </div>
          </div>

          <div v-else class="flex flex-col gap-3">
            <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
              <h3 class="classic-section-title">Remote IGW</h3>
              <p class="text-[10px] text-muted mt-1">
                Cloudflare Access + inverter-gateway (HTTPS).
              </p>
            </header>
            <div class="flex flex-col gap-3 p-3 classic-inset !rounded-lg !p-3">
              <div class="flex flex-col gap-1">
                <label for="setup_gateway_url" class="classic-label px-1">Gateway URL</label>
                <input
                  id="setup_gateway_url"
                  v-model="config.gateway_url"
                  type="url"
                  class="classic-input w-full"
                  placeholder="https://victron.example.com"
                  autocomplete="off"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label for="setup_gateway_access_client_id" class="classic-label px-1"
                  >CF Access Client ID</label
                >
                <input
                  id="setup_gateway_access_client_id"
                  v-model="config.gateway_access_client_id"
                  type="text"
                  class="classic-input w-full"
                  placeholder="….access"
                  autocomplete="off"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label for="setup_gateway_access_client_secret" class="classic-label px-1"
                  >CF Access Client Secret</label
                >
                <input
                  id="setup_gateway_access_client_secret"
                  v-model="config.gateway_access_client_secret"
                  type="password"
                  class="classic-input w-full"
                  placeholder="Service token secret"
                  autocomplete="new-password"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label for="setup_gateway_api_token" class="classic-label px-1"
                  >API Bearer Token</label
                >
                <input
                  id="setup_gateway_api_token"
                  v-model="config.gateway_api_token"
                  type="password"
                  class="classic-input w-full"
                  placeholder="GATEWAY_API_TOKEN"
                  autocomplete="new-password"
                />
              </div>
              <div class="flex items-center gap-2 pt-1">
                <UiButton class="flex-1" :loading="testingGateway" @click="testGatewayConnection">
                  Test connection
                </UiButton>
              </div>
              <p
                v-if="gatewayTestResult"
                class="text-[11px] px-1"
                :class="gatewayTestSuccess ? 'text-generation' : 'text-consumption'"
              >
                {{ gatewayTestResult }}
              </p>
            </div>
          </div>
        </div>

        <!-- Advanced: optional HA -->
        <div v-else class="flex flex-col gap-4">
          <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
            <h3 class="classic-section-title">Home Assistant</h3>
            <p class="text-[10px] text-muted mt-1">Optional — home devices, laundry, covers.</p>
          </header>

          <div class="flex flex-col gap-3 p-3 classic-inset !rounded-lg !p-3">
            <div class="flex flex-col gap-1">
              <label for="setup_ha_url" class="classic-label px-1">URL / IP</label>
              <input
                id="setup_ha_url"
                v-model="config.ha_url"
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
                  v-model.number="config.ha_port"
                  type="number"
                  class="classic-input w-full"
                  placeholder="8123"
                />
              </div>
              <div class="flex flex-col gap-1">
                <label for="setup_ha_token" class="classic-label px-1">Long-lived Token</label>
                <input
                  id="setup_ha_token"
                  v-model="config.ha_longlived_token"
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
                v-model="config.mqtt_ha_host"
                type="text"
                class="classic-input w-full"
                autocomplete="off"
              />
            </div>
            <div class="flex flex-col gap-1">
              <label for="setup_mqtt_ha_port" class="classic-label px-1">Port</label>
              <input
                id="setup_mqtt_ha_port"
                v-model.number="config.mqtt_ha_port"
                type="number"
                class="classic-input w-full"
              />
            </div>
            <div class="flex flex-col gap-1">
              <label for="setup_mqtt_ha_login" class="classic-label px-1">Username</label>
              <input
                id="setup_mqtt_ha_login"
                v-model="config.mqtt_ha_login"
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
                v-model="config.mqtt_ha_password"
                type="password"
                class="classic-input w-full"
                placeholder="Optional"
                autocomplete="new-password"
              />
            </div>
          </div>
        </div>
      </div>

      <div
        class="px-5 py-4 border-t border-black/[0.06] dark:border-white/[0.07] flex flex-col gap-2"
      >
        <p v-if="error" class="text-[11px] text-consumption text-center">{{ error }}</p>
        <UiButton variant="primary" size="lg" class="w-full" :loading="saving" @click="handleSave">
          Save &amp; Continue
        </UiButton>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Settings } from '@lucide/vue'
import { invoke } from '@tauri-apps/api/core'
import { onMounted, reactive, ref } from 'vue'
import type { AppConfig } from '../config'
import { defaultConfig } from '../config'
import { logger } from '../logger'
import UiButton from './UiButton.vue'

const emit = defineEmits<{
  complete: [config: AppConfig]
}>()

const activeTab = ref<'main' | 'advanced'>('main')
const connectionMode = ref<'mqtt' | 'igw'>('mqtt')
const config = reactive<AppConfig>({ ...defaultConfig })
const saving = ref(false)
const error = ref('')
const testingGateway = ref(false)
const gatewayTestResult = ref('')
const gatewayTestSuccess = ref(false)
const testingHa = ref(false)
const haTestResult = ref('')
const haTestSuccess = ref(false)

function selectMqttMode() {
  connectionMode.value = 'mqtt'
  config.gateway_enabled = false
}

function selectIgwMode() {
  connectionMode.value = 'igw'
  config.gateway_enabled = true
}

function validateMain(): string | null {
  if (connectionMode.value === 'mqtt') {
    if (!config.mqtt_host?.trim()) return 'MQTT host is required'
    if (!config.mqtt_port || config.mqtt_port <= 0) return 'MQTT port is required'
    return null
  }
  if (!(config.gateway_url || '').trim()) return 'Gateway URL is required'
  if (!(config.gateway_access_client_id || '').trim()) return 'CF Access Client ID is required'
  if (!(config.gateway_access_client_secret || '').trim())
    return 'CF Access Client Secret is required'
  if (!(config.gateway_api_token || '').trim()) return 'API bearer token is required'
  return null
}

async function testGatewayConnection() {
  const url = (config.gateway_url || '').trim()
  const clientId = (config.gateway_access_client_id || '').trim()
  const clientSecret = (config.gateway_access_client_secret || '').trim()
  if (!url || !clientId || !clientSecret) {
    gatewayTestResult.value = 'URL, Access Client ID and Secret required'
    gatewayTestSuccess.value = false
    return
  }
  testingGateway.value = true
  gatewayTestResult.value = ''
  try {
    const result = await invoke<{ status: string; mqtt_connected?: boolean | null }>(
      'test_gateway_connection',
      {
        url,
        accessClientId: clientId,
        accessClientSecret: clientSecret,
        apiToken: (config.gateway_api_token || '').trim() || null,
      }
    )
    let mqtt = 'MQTT unknown'
    if (result.mqtt_connected === true) {
      mqtt = 'MQTT connected'
    } else if (result.mqtt_connected === false) {
      mqtt = 'MQTT disconnected'
    }
    gatewayTestResult.value = `OK (${result.status}) — ${mqtt}`
    gatewayTestSuccess.value = true
  } catch (e) {
    gatewayTestResult.value = `Failed: ${e?.toString() || e}`
    gatewayTestSuccess.value = false
  } finally {
    testingGateway.value = false
  }
}

async function testHaConnection() {
  if (!config.ha_url || !config.ha_longlived_token) {
    haTestResult.value = 'URL and token required'
    haTestSuccess.value = false
    return
  }
  testingHa.value = true
  haTestResult.value = ''
  try {
    await invoke('test_ha_connection', {
      url: config.ha_url,
      port: config.ha_port ?? null,
      token: config.ha_longlived_token,
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

async function handleSave() {
  const validationError = validateMain()
  if (validationError) {
    error.value = validationError
    activeTab.value = 'main'
    return
  }
  error.value = ''
  saving.value = true
  try {
    if (connectionMode.value === 'igw') {
      config.gateway_enabled = true
    } else {
      config.gateway_enabled = false
    }
    if (config.ha_url?.trim() && config.ha_longlived_token?.trim()) {
      config.ha_use_direct_api = true
    }
    config.setup_completed = true
    await invoke('save_config', { config: { ...config } })
    emit('complete', { ...config })
  } catch (e) {
    error.value = String(e)
    logger.error('Setup save failed:', e)
  } finally {
    saving.value = false
  }
}

onMounted(async () => {
  try {
    const loaded = await invoke<AppConfig>('get_config')
    Object.assign(config, defaultConfig, loaded)
    if (config.gateway_enabled && (config.gateway_url || '').trim()) {
      connectionMode.value = 'igw'
    } else {
      connectionMode.value = 'mqtt'
      config.gateway_enabled = false
    }
    if (!config.mqtt_port) config.mqtt_port = 1883
    if (config.ha_port == null) config.ha_port = 8123
    if (config.mqtt_ha_port == null) config.mqtt_ha_port = 1883
  } catch (e) {
    logger.warn('Setup wizard failed to load config:', e)
  }
})
</script>

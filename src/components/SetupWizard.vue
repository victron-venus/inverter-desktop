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
            v-if="featureSetupAvailable"
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
            <label class="flex items-center gap-2 text-[12px]">
              <input id="setup_mqtt_tls" v-model="config.mqtt_tls" type="checkbox" />
              TLS encrypted connection
            </label>
            <p class="text-[11px] text-muted">TLS is required when using a username or password.</p>
          </div>

          <div v-else class="flex flex-col gap-3">
            <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
              <h3 class="classic-section-title">Remote IGW</h3>
              <p class="text-[10px] text-muted mt-1">
                Connect directly over HTTPS, or through Cloudflare Access when it protects your
                gateway URL.
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
                <p class="text-[10px] text-muted px-1">
                  Use a local or public HTTPS URL with a certificate trusted by your system.
                </p>
              </div>
              <div class="flex flex-col gap-1">
                <label for="setup_gateway_access_client_id" class="classic-label px-1"
                  >Cloudflare Access Client ID (optional)</label
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
                  >Cloudflare Access Client Secret (optional)</label
                >
                <input
                  id="setup_gateway_access_client_secret"
                  v-model="config.gateway_access_client_secret"
                  type="password"
                  class="classic-input w-full"
                  placeholder="Service token secret"
                  autocomplete="new-password"
                />
                <p class="text-[10px] text-muted px-1">
                  Leave both Access fields blank for a direct HTTPS gateway, or supply both.
                </p>
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
                <p class="text-[10px] text-muted px-1">
                  Enter the API token configured on your gateway.
                </p>
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

        <FeatureSetup v-else v-model:config="featureConfig" />
        <TariffConfiguration v-model="installationTariff" class="mt-4" />
      </div>

      <div
        class="px-5 py-4 border-t border-black/[0.06] dark:border-white/[0.07] flex flex-col gap-2"
      >
        <p v-if="error" class="text-[11px] text-consumption text-center">{{ error }}</p>
        <PrivacyLink class="text-[12px]" />
        <UiButton variant="primary" size="lg" class="w-full" :loading="saving" @click="handleSave">
          Save &amp; Continue
        </UiButton>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { FeatureSetup, featureSetupAvailable, prepareFeatureConfig } from '@features'
import { Settings } from '@lucide/vue'
import { invoke } from '@tauri-apps/api/core'
import { computed, onMounted, reactive, ref } from 'vue'
import type { AppConfig } from '../config'
import TariffConfiguration from './TariffConfiguration.vue'
import { configurationTariff, setConfigurationTariff } from '../configurationTariff'
import { defaultConfig } from '../config'
import { gatewayConfigError } from '../connectionPolicy'
import { logger } from '../logger'
import UiButton from './UiButton.vue'
import PrivacyLink from './PrivacyLink.vue'

const emit = defineEmits<{
  complete: [config: AppConfig]
}>()

const activeTab = ref<'main' | 'advanced'>('main')
const connectionMode = ref<'mqtt' | 'igw'>('mqtt')
const config = reactive<AppConfig>({ ...defaultConfig })
const installationTariff = computed({
  get: () => configurationTariff(config),
  set: (plan) =>
    setConfigurationTariff(config, plan as import('../tariffs/model').TariffPlan | null),
})
const featureConfig = computed({
  get: () => config,
  set: (updated: AppConfig) => {
    Object.assign(config, updated)
  },
})
const saving = ref(false)
const error = ref('')
const testingGateway = ref(false)
const gatewayTestResult = ref('')
const gatewayTestSuccess = ref(false)

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
    if (!config.mqtt_tls && (config.mqtt_login || config.mqtt_password))
      return 'Enable TLS before using an MQTT username or password'
    return null
  }
  return gatewayConfigError(config)
}

async function testGatewayConnection() {
  const url = (config.gateway_url || '').trim()
  const clientId = (config.gateway_access_client_id || '').trim()
  const clientSecret = (config.gateway_access_client_secret || '').trim()
  const validationError = gatewayConfigError(config)
  if (validationError) {
    gatewayTestResult.value = validationError
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
    prepareFeatureConfig(config)
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
    prepareFeatureConfig(config)
  } catch (e) {
    logger.warn('Setup wizard failed to load config:', e)
  }
})
</script>

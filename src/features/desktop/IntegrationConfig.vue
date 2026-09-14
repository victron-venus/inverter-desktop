<template>
  <!-- Home Assistant Section -->
  <div class="flex flex-col gap-4">
    <header class="border-b border-black/[0.06] dark:border-white/[0.07] pb-2">
      <h2 class="classic-section-title">Home Assistant</h2>
      <p class="text-[10px] text-muted mt-1">
        {{ $t('config.haControlsHelp') }}
      </p>
    </header>

    <div class="flex flex-col gap-3 p-3 classic-inset !rounded-lg !p-3">
      <div class="flex flex-col gap-1">
        <label for="ha_url" class="classic-label px-1">Server URL</label>
        <input
          id="ha_url"
          v-model="config.ha_url"
          type="text"
          class="classic-input w-full"
          placeholder="http://homeassistant.local"
        />
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="flex flex-col gap-1">
          <label for="ha_port" class="classic-label px-1">API Port</label>
          <input
            id="ha_port"
            v-model.number="config.ha_port"
            type="number"
            class="classic-input w-full"
            placeholder="8123"
          />
        </div>
        <div class="flex flex-col gap-1">
          <span class="classic-label px-1">Status</span>
          <div
            class="h-8 flex items-center px-2 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] text-[10px] font-semibold"
          >
            <span :class="haDirectMonitoringEnabled ? 'text-green-500' : 'text-muted'">
              API: {{ haDirectMonitoringEnabled ? 'Enabled' : 'Disabled' }}
            </span>
          </div>
        </div>
      </div>
      <div class="flex flex-col gap-1">
        <label for="ha_token" class="classic-label px-1">Access Token</label>
        <input
          id="ha_token"
          v-model="config.ha_longlived_token"
          type="password"
          class="classic-input w-full"
          placeholder="Token"
        />
      </div>
      <div class="flex gap-2 mt-1">
        <UiButton class="flex-1" :loading="testingHa" @click="testHaConnection">
          {{ testingHa ? 'Testing...' : 'Test Connection' }}
        </UiButton>
        <UiButton
          class="flex-1"
          :disabled="discoveryLoading || !haDirectMonitoringEnabled"
          :loading="discoveryLoading"
          @click="handleFetchHaEntities"
        >
          Fetch Entities
        </UiButton>
      </div>

      <div
        v-if="haTestResult"
        :class="haTestSuccess ? 'text-green-500' : 'text-red-500'"
        class="text-[10px] font-bold text-center mt-1"
      >
        {{ haTestResult }}
      </div>
    </div>

    <div class="flex flex-col gap-3 mt-2">
      <h3 class="classic-subsection-title">{{ $t('config.cameraEventsTitle') }}</h3>
      <p class="text-[10px] text-muted px-1">
        {{ $t('config.cameraEventsHelp') }}
      </p>
      <div class="grid grid-cols-2 gap-3">
        <div class="flex flex-col gap-1">
          <label for="mqtt_ha_host" class="classic-label px-1">{{ $t('config.haMqttHost') }}</label>
          <input
            id="mqtt_ha_host"
            v-model="config.mqtt_ha_host"
            type="text"
            class="classic-input w-full"
            :placeholder="$t('config.haMqttHostPlaceholder')"
          />
        </div>
        <div class="flex flex-col gap-1">
          <label for="mqtt_ha_port" class="classic-label px-1">{{ $t('config.haMqttPort') }}</label>
          <input
            id="mqtt_ha_port"
            v-model.number="config.mqtt_ha_port"
            type="number"
            class="classic-input w-full"
            placeholder="1883"
          />
        </div>
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="flex flex-col gap-1">
          <label for="mqtt_ha_login" class="classic-label px-1">{{
            $t('config.haMqttUsername')
          }}</label>
          <input
            id="mqtt_ha_login"
            v-model="config.mqtt_ha_login"
            type="text"
            class="classic-input w-full"
            :placeholder="$t('config.optional')"
          />
        </div>
        <div class="flex flex-col gap-1">
          <label for="mqtt_ha_password" class="classic-label px-1">{{
            $t('config.haMqttPassword')
          }}</label>
          <input
            id="mqtt_ha_password"
            v-model="config.mqtt_ha_password"
            type="password"
            class="classic-input w-full"
            :placeholder="$t('config.optional')"
          />
        </div>
      </div>
      <div class="flex flex-col gap-1">
        <span class="classic-label px-1">{{ $t('config.cameraMonitoring') }}</span>
        <label
          class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
        >
          <input
            type="checkbox"
            v-model="config.camera_enabled"
            class="rounded border-slate-300 text-accent focus:ring-accent"
          />
          <span class="text-[11px] font-bold text-main">{{ $t('config.cameraEnabled') }}</span>
        </label>
      </div>
      <div class="flex flex-col gap-1">
        <label for="camera_topic" class="classic-label px-1">{{ $t('config.cameraTopic') }}</label>
        <input
          id="camera_topic"
          v-model="config.camera_topic"
          type="text"
          :disabled="!config.camera_enabled"
          class="classic-input w-full disabled:opacity-50"
          :placeholder="$t('config.cameraTopicPlaceholder')"
        />
        <p class="text-[10px] text-muted px-1 italic">
          {{ $t('config.cameraTopicHelp') }}
        </p>
      </div>
      <div v-if="config.camera_enabled" class="flex flex-col gap-1">
        <label for="frigate_base_url" class="classic-label px-1">{{
          $t('config.frigateBaseUrl')
        }}</label>
        <input
          id="frigate_base_url"
          v-model="config.frigate_base_url"
          type="text"
          class="classic-input w-full"
          :placeholder="$t('config.frigateBaseUrlPlaceholder')"
        />
        <p class="text-[10px] text-muted px-1 italic">
          {{ $t('config.frigateBaseUrlHelp') }}
        </p>
      </div>
      <div v-if="config.camera_enabled" class="flex flex-col gap-1">
        <label for="ring_snapshot_url_template" class="classic-label px-1">{{
          $t('config.ringSnapshotUrlTemplate')
        }}</label>
        <input
          id="ring_snapshot_url_template"
          v-model="config.ring_snapshot_url_template"
          type="text"
          class="classic-input w-full"
          :placeholder="$t('config.ringSnapshotUrlTemplatePlaceholder')"
        />
        <p class="text-[10px] text-muted px-1 italic">
          {{ $t('config.ringSnapshotUrlTemplateHelp') }}
        </p>
      </div>
      <label
        class="flex items-center gap-2 px-2 py-1.5 rounded-lg border border-black/[0.08] dark:border-white/[0.1] bg-white dark:bg-[#1c1c1e] cursor-pointer group hover:border-accent/40 transition-colors"
      >
        <input
          type="checkbox"
          v-model="config.show_advanced_settings"
          class="rounded border-slate-300 text-accent focus:ring-accent"
        />
        <span class="text-[11px] font-bold text-main">{{ $t('config.advancedSettings') }}</span>
      </label>
    </div>

    <!-- Appliance Entities -->
    <div
      v-if="config.show_advanced_settings"
      class="flex flex-col gap-3 p-3 classic-inset !rounded-lg !p-3"
    >
      <h3 class="classic-subsection-title">Appliance Entities</h3>
      <div class="flex flex-col gap-1">
        <label for="ha_dryer_entity" class="classic-label px-1">Dryer Entity</label>
        <input
          id="ha_dryer_entity"
          v-model="config.ha_dryer_entity"
          type="text"
          class="classic-input w-full"
          placeholder="sensor.dryer_remaining_time"
        />
        <p class="text-[10px] text-muted px-1 italic">
          Remaining time sensor (e.g. 10:02). Section shows while the dryer is running.
        </p>
      </div>
      <div class="flex flex-col gap-1">
        <label for="ha_dryer_start_entity" class="classic-label px-1">Dryer Start Entity</label>
        <input
          id="ha_dryer_start_entity"
          v-model="config.ha_dryer_start_entity"
          type="text"
          class="classic-input w-full"
          placeholder="button.dryer_remote_start"
        />
        <p class="text-[10px] text-muted px-1 italic">
          Optional START button shown in the Dryer section.
        </p>
      </div>
      <div class="flex flex-col gap-1">
        <label for="ha_dryer_pause_entity" class="classic-label px-1">Dryer Pause Entity</label>
        <input
          id="ha_dryer_pause_entity"
          v-model="config.ha_dryer_pause_entity"
          type="text"
          class="classic-input w-full"
          placeholder="button.dryer_pause"
        />
        <p class="text-[10px] text-muted px-1 italic">
          Optional PAUSE button shown in the Dryer section.
        </p>
      </div>
      <div class="flex flex-col gap-1">
        <label for="ha_washer_entity" class="classic-label px-1">Washer Entity</label>
        <input
          id="ha_washer_entity"
          v-model="config.ha_washer_entity"
          type="text"
          class="classic-input w-full"
          placeholder="sensor.washer_remaining_time"
        />
        <p class="text-[10px] text-muted px-1 italic">
          Remaining time sensor. Section shows while the washer is running.
        </p>
      </div>
      <div class="flex flex-col gap-1">
        <label for="ha_washer_start_entity" class="classic-label px-1">Washer Start Entity</label>
        <input
          id="ha_washer_start_entity"
          v-model="config.ha_washer_start_entity"
          type="text"
          class="classic-input w-full"
          placeholder="button.washer_remote_start"
        />
        <p class="text-[10px] text-muted px-1 italic">
          Optional START button shown in the Washer section.
        </p>
      </div>
      <div class="flex flex-col gap-1">
        <label for="ha_washer_pause_entity" class="classic-label px-1">Washer Pause Entity</label>
        <input
          id="ha_washer_pause_entity"
          v-model="config.ha_washer_pause_entity"
          type="text"
          class="classic-input w-full"
          placeholder="button.washer_pause"
        />
        <p class="text-[10px] text-muted px-1 italic">
          Optional PAUSE button shown in the Washer section.
        </p>
      </div>
      <div class="flex flex-col gap-1">
        <label for="ha_dishwasher_running_entity" class="classic-label px-1"
          >Dishwasher Running Entity</label
        >
        <input
          id="ha_dishwasher_running_entity"
          v-model="config.ha_dishwasher_running_entity"
          type="text"
          class="classic-input w-full"
          placeholder="binary_sensor.dishwasher_running"
        />
        <p class="text-[10px] text-muted px-1 italic">
          Binary sensor that turns on while the dishwasher runs.
        </p>
      </div>
      <div class="flex flex-col gap-1">
        <label for="ha_dishwasher_duration_entity" class="classic-label px-1"
          >Dishwasher Duration Entity</label
        >
        <input
          id="ha_dishwasher_duration_entity"
          v-model="config.ha_dishwasher_duration_entity"
          type="text"
          class="classic-input w-full"
          placeholder="sensor.dishwasher_duration"
        />
        <p class="text-[10px] text-muted px-1 italic">
          Runtime since midnight (e.g. 00:32:24), shown next to the section.
        </p>
      </div>
    </div>
  </div>
</template>
<script setup lang="ts">
import type { AppConfig } from '../../config'
import type { useDashboardControlsConfig } from '../../composables/useDashboardControlsConfig'
import { invoke } from '@tauri-apps/api/core'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import UiButton from '../../components/UiButton.vue'
const { config, controls } = defineProps<{
  config: AppConfig
  controls: ReturnType<typeof useDashboardControlsConfig>
}>()
const { t: $t } = useI18n()
const { fetchHaEntities, discoveryLoading } = controls
const testingHa = ref(false)
const haTestResult = ref('')
const haTestSuccess = ref(false)
const haDirectMonitoringEnabled = computed(() => {
  return !!(
    config.ha_url &&
    config.ha_longlived_token &&
    config.ha_url.trim() &&
    config.ha_longlived_token.trim()
  )
})

watch(
  [() => config.ha_longlived_token, () => config.ha_url],
  ([token, url]) => {
    config.ha_use_direct_api = !!(token && url && token.trim() && url.trim())
  },
  { immediate: true }
)

async function testHaConnection() {
  if (!config.ha_url || !config.ha_longlived_token) {
    haTestResult.value = 'URL and Token required'
    haTestSuccess.value = false
    return
  }
  testingHa.value = true
  haTestResult.value = ''
  try {
    await invoke('test_ha_connection', {
      url: config.ha_url,
      port: config.ha_port || 8123,
      token: config.ha_longlived_token,
    })
    haTestResult.value = 'Connection successful'
    haTestSuccess.value = true
  } catch (e) {
    haTestResult.value = `Failed: ${e?.toString() || e}`
    haTestSuccess.value = false
  } finally {
    testingHa.value = false
  }
}

async function handleFetchHaEntities() {
  if (!config.ha_url || !config.ha_longlived_token) {
    haTestResult.value = 'Please enter HA URL and Token first'
    haTestSuccess.value = false
    return
  }
  try {
    await fetchHaEntities(config.ha_url, config.ha_port, config.ha_longlived_token)
  } catch (e) {
    haTestResult.value = `Discovery failed: ${e?.toString() || e}`
    haTestSuccess.value = false
  }
}
</script>

<template>
  <v-app>
    <v-app-bar color="primary" density="compact">
      <v-app-bar-title>Configuration</v-app-bar-title>
      <v-spacer></v-spacer>
      <v-btn icon @click="resetToDefaults">
        <v-icon>mdi-refresh</v-icon>
        <v-tooltip activator="parent">Reset to defaults</v-tooltip>
      </v-btn>
      <v-btn icon @click="saveConfig" :loading="saving">
        <v-icon>mdi-content-save</v-icon>
        <v-tooltip activator="parent">Save</v-tooltip>
      </v-btn>
      <v-btn icon @click="closeWindow">
        <v-icon>mdi-close</v-icon>
        <v-tooltip activator="parent">Close</v-tooltip>
      </v-btn>
    </v-app-bar>

    <v-main>
      <v-container fluid class="fill-height">
        <v-row>
          <v-col cols="12" md="8" lg="6">
            <v-card>
              <v-card-text>
                <v-form ref="formRef">
                  <!-- MQTT Settings -->
                  <v-divider class="mb-4">
                    <v-chip size="small" color="primary">MQTT</v-chip>
                  </v-divider>

                  <v-row>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model="config.mqtt_host"
                        label="MQTT Host"
                        required
                        variant="outlined"
                        density="compact"
                      ></v-text-field>
                    </v-col>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model.number="config.mqtt_port"
                        label="MQTT Port"
                        type="number"
                        required
                        variant="outlined"
                        density="compact"
                      ></v-text-field>
                    </v-col>
                  </v-row>

                  <v-row>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model="config.mqtt_login"
                        label="MQTT Login (optional)"
                        variant="outlined"
                        density="compact"
                      ></v-text-field>
                    </v-col>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model="config.mqtt_password"
                        label="MQTT Password (optional)"
                        type="password"
                        variant="outlined"
                        density="compact"
                        :append-inner-icon="showPassword ? 'mdi-eye' : 'mdi-eye-off'"
                        @click:append-inner="showPassword = !showPassword"
                      ></v-text-field>
                    </v-col>
                  </v-row>

                  <!-- Home Assistant Settings -->
                  <v-divider class="mb-4 mt-4">
                    <v-chip size="small" color="primary">Home Assistant</v-chip>
                  </v-divider>

                  <v-text-field
                    v-model="config.ha_longlived_token"
                    label="HA Long-lived Access Token"
                    type="password"
                    variant="outlined"
                    density="compact"
                    class="mb-4"
                  ></v-text-field>

                  <v-row>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model="config.ha_water_valve_entity"
                        label="Water Valve Entity"
                        placeholder="switch.shutoff_valve"
                        variant="outlined"
                        density="compact"
                      ></v-text-field>
                    </v-col>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model="config.ha_pump_switch_entity"
                        label="Pump Switch Entity"
                        placeholder="switch.pump_switch"
                        variant="outlined"
                        density="compact"
                      ></v-text-field>
                    </v-col>
                  </v-row>

                  <!-- Entity Mappings -->
                  <v-divider class="mb-4 mt-4">
                    <v-chip size="small" color="primary">Entity Mappings (JSON)</v-chip>
                  </v-divider>

                  <v-textarea
                    v-model="booleanEntitiesJson"
                    label="Boolean Entities"
                    placeholder='{"only_charging": "input_boolean.only_charging", ...}'
                    variant="outlined"
                    density="compact"
                    rows="4"
                    class="mb-2"
                    hide-details="auto"
                  ></v-textarea>

                  <v-textarea
                    v-model="switchEntitiesJson"
                    label="Switch Entities"
                    placeholder='{"pump": {"label": "Pump", "entity": "switch.pump"}}'
                    variant="outlined"
                    density="compact"
                    rows="4"
                    hide-details="auto"
                  ></v-textarea>

                  <v-alert
                    v-if="jsonError"
                    type="error"
                    density="compact"
                    class="mt-2"
                  >
                    {{ jsonError }}
                  </v-alert>

                  <!-- UI Settings -->
                  <v-divider class="mb-4 mt-4">
                    <v-chip size="small" color="primary">UI Settings</v-chip>
                  </v-divider>

                  <v-row>
                    <v-col cols="12" sm="6">
                      <v-select
                        v-model="config.color_scheme"
                        label="Color Scheme"
                        :items="['dark', 'light']"
                        variant="outlined"
                        density="compact"
                      ></v-select>
                    </v-col>
                  </v-row>

                  <!-- Header Toggles -->
                  <v-divider class="mb-4 mt-4">
                    <v-chip size="small" color="primary">Header Toggles (JSON)</v-chip>
                  </v-divider>

                  <v-textarea
                    v-model="headerTogglesJson"
                    label="Header Toggles Configuration"
                    placeholder='[{"id": "only_charging", "label": "ONLY CHARGING", "entity": "input_boolean.only_charging"}]'
                    variant="outlined"
                    density="compact"
                    rows="3"
                    hide-details="auto"
                  ></v-textarea>
                </v-form>
              </v-card-text>
            </v-card>

            <v-alert
              v-if="message"
              :type="messageType"
              density="compact"
              class="mt-4"
              closable
            >
              {{ message }}
            </v-alert>
          </v-col>
        </v-row>
      </v-container>
    </v-main>
  </v-app>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

interface AppConfig {
  mqtt_host: string
  mqtt_port: number
  mqtt_login?: string | null
  mqtt_password?: string | null
  ha_longlived_token?: string | null
  color_scheme?: string | null
  ha_boolean_entities?: Record<string, string> | null
  ha_switch_entities?: Record<string, { label?: string; entity: string }> | null
  ha_water_valve_entity?: string | null
  ha_pump_switch_entity?: string | null
  header_toggles?: Array<{ id: string; label: string; entity: string }> | null
}

const defaultConfig: AppConfig = {
  mqtt_host: '192.168.160.150',
  mqtt_port: 1883,
  mqtt_login: null,
  mqtt_password: null,
  ha_longlived_token: null,
  color_scheme: 'dark',
  ha_boolean_entities: null,
  ha_switch_entities: null,
  ha_water_valve_entity: null,
  ha_pump_switch_entity: null,
  header_toggles: null
}

const config = reactive<AppConfig>({ ...defaultConfig })
const saving = ref(false)
const message = ref('')
const messageType = ref<'success' | 'error' | 'info'>('info')
const jsonError = ref('')
const showPassword = ref(false)

// Computed JSON strings for textareas
const booleanEntitiesJson = ref('{}')
const switchEntitiesJson = ref('{}')
const headerTogglesJson = ref('[]')

// Watch JSON inputs and parse to update config
watch(booleanEntitiesJson, (val) => {
  try {
    if (val.trim()) {
      const parsed = JSON.parse(val)
      config.ha_boolean_entities = parsed
      jsonError.value = ''
    } else {
      config.ha_boolean_entities = {}
      jsonError.value = ''
    }
  } catch (e) {
    jsonError.value = `Invalid JSON: ${e}`
  }
})

watch(switchEntitiesJson, (val) => {
  try {
    if (val.trim()) {
      const parsed = JSON.parse(val)
      config.ha_switch_entities = parsed
      jsonError.value = ''
    } else {
      config.ha_switch_entities = {}
      jsonError.value = ''
    }
  } catch (e) {
    jsonError.value = `Invalid JSON: ${e}`
  }
})

watch(headerTogglesJson, (val) => {
  try {
    if (val.trim()) {
      const parsed = JSON.parse(val)
      config.header_toggles = parsed
      jsonError.value = ''
    } else {
      config.header_toggles = []
      jsonError.value = ''
    }
  } catch (e) {
    jsonError.value = `Invalid JSON: ${e}`
  }
})

async function loadConfig() {
  try {
    const loaded = await invoke<AppConfig>('get_config')
    Object.assign(config, loaded)
    // Update JSON textareas
    booleanEntitiesJson.value = JSON.stringify(loaded.ha_boolean_entities || {}, null, 2)
    switchEntitiesJson.value = JSON.stringify(loaded.ha_switch_entities || {}, null, 2)
    headerTogglesJson.value = JSON.stringify(loaded.header_toggles || [], null, 2)
    message.value = ''
  } catch (e) {
    message.value = `Failed to load config: ${e}`
    messageType.value = 'error'
  }
}

async function saveConfig() {
  // Validate JSON fields first
  try {
    if (booleanEntitiesJson.value.trim()) JSON.parse(booleanEntitiesJson.value)
    if (switchEntitiesJson.value.trim()) JSON.parse(switchEntitiesJson.value)
    if (headerTogglesJson.value.trim()) JSON.parse(headerTogglesJson.value)
  } catch (e) {
    jsonError.value = `Invalid JSON: ${e}`
    message.value = 'Cannot save with invalid JSON'
    messageType.value = 'error'
    return
  }

  saving.value = true
  try {
    await invoke('save_config', { config })
    message.value = 'Configuration saved successfully'
    messageType.value = 'success'
  } catch (e) {
    message.value = `Failed to save config: ${e}`
    messageType.value = 'error'
  } finally {
    saving.value = false
  }
}

function resetToDefaults() {
  Object.assign(config, defaultConfig)
  booleanEntitiesJson.value = JSON.stringify(defaultConfig.ha_boolean_entities || {}, null, 2)
  switchEntitiesJson.value = JSON.stringify(defaultConfig.ha_switch_entities || {}, null, 2)
  headerTogglesJson.value = JSON.stringify(defaultConfig.header_toggles || [], null, 2)
  message.value = 'Reset to defaults (unsaved)'
  messageType.value = 'info'
}

async function closeWindow() {
  const window = await getCurrentWindow()
  window.close()
}

onMounted(() => {
  loadConfig()
})
</script>

<style scoped>
.fill-height {
  min-height: 100vh;
}
</style>

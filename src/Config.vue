<template>
  <v-app>
    <v-app-bar color="primary" density="compact">
      <v-app-bar-title>Configuration</v-app-bar-title>
      <v-spacer></v-spacer>
      <v-btn icon @click="handleReset">
        <v-icon>mdi-refresh</v-icon>
        <v-tooltip activator="parent">Reset to defaults</v-tooltip>
      </v-btn>
      <v-btn icon @click="handleSave" :loading="saving">
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

                  <v-divider class="mb-4 mt-4">
                    <v-chip size="small" color="primary">MQTT HA</v-chip>
                  </v-divider>

                  <v-row>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model="config.mqtt_ha_host"
                        label="MQTT HA Host"
                        variant="outlined"
                        density="compact"
                      ></v-text-field>
                    </v-col>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model.number="config.mqtt_ha_port"
                        label="MQTT HA Port"
                        type="number"
                        variant="outlined"
                        density="compact"
                      ></v-text-field>
                    </v-col>
                  </v-row>

                  <v-row>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model="config.mqtt_ha_login"
                        label="MQTT HA Login (optional)"
                        variant="outlined"
                        density="compact"
                      ></v-text-field>
                    </v-col>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model="config.mqtt_ha_password"
                        label="MQTT HA Password (optional)"
                        type="password"
                        variant="outlined"
                        density="compact"
                        :append-inner-icon="showPasswordHa ? 'mdi-eye' : 'mdi-eye-off'"
                        @click:append-inner="showPasswordHa = !showPasswordHa"
                      ></v-text-field>
                    </v-col>
                  </v-row>

                  <v-divider class="mb-4 mt-4">
                    <v-chip size="small" color="primary">Home Assistant</v-chip>
                  </v-divider>

                  <v-text-field
                    v-model="config.ha_url"
                    label="HA URL"
                    placeholder="http://homeassistant.local"
                    variant="outlined"
                    density="compact"
                    class="mb-2"
                    :rules="urlRules"
                  ></v-text-field>

                  <v-row>
                    <v-col cols="12" sm="6">
                      <v-text-field
                        v-model.number="config.ha_port"
                        label="HA Port"
                        type="number"
                        placeholder="8123"
                        variant="outlined"
                        density="compact"
                        :rules="portRules"
                      ></v-text-field>
                    </v-col>
                    <v-col cols="12" sm="6" class="d-flex align-center">
                      <v-chip
                        :color="haDirectMonitoringEnabled ? 'success' : 'grey'"
                        label
                      >
                        HA Direct: {{ haDirectMonitoringEnabled ? 'Enabled' : 'Disabled' }}
                      </v-chip>
                    </v-col>
                  </v-row>

                  <v-text-field
                    v-model="config.ha_longlived_token"
                    label="HA Long-lived Access Token"
                    type="password"
                    variant="outlined"
                    density="compact"
                    class="mb-2"
                  ></v-text-field>

                  <v-btn
                    color="primary"
                    variant="flat"
                    size="small"
                    @click="testHaConnection"
                    :loading="testingHa"
                    class="mr-2 mb-4"
                  >
                    Test HA Connection
                  </v-btn>
                  <v-btn
                    color="primary"
                    variant="flat"
                    size="small"
                    @click="handleFetchHaEntities"
                    :loading="discoveryLoading"
                    :disabled="!isHaConfigured"
                    class="mb-4"
                  >
                    Fetch from HA
                  </v-btn>

                  <v-alert
                    v-if="haTestResult"
                    :type="haTestSuccess ? 'success' : 'error'"
                    density="compact"
                    class="mb-2"
                  >
                    {{ haTestResult }}
                  </v-alert>

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

                  <v-divider class="mb-4 mt-4">
                    <v-chip size="small" color="primary">Home Buttons</v-chip>
                  </v-divider>

                  <div
                    v-if="haEntitiesList.length === 0"
                    class="text-caption grey--text mb-2"
                  >
                    No home buttons configured. Add entities below.
                  </div>

                  <div
                    v-for="(entity, index) in haEntitiesList"
                    :key="entity.id || `home-${index}`"
                    class="entity-card mb-2 pa-2 rounded border"
                  >
                    <v-row dense>
                      <v-col cols="12" sm="3">
                        <v-text-field
                          v-model="entity.label"
                          label="Label"
                          variant="outlined"
                          density="compact"
                          hide-details
                          required
                        ></v-text-field>
                      </v-col>
                      <v-col cols="12" sm="4">
                        <v-autocomplete
                          v-model="entity.entity"
                          :items="discoveredEntities"
                          item-title="entity_id"
                          item-value="entity_id"
                          label="Entity ID"
                          variant="outlined"
                          density="compact"
                          hide-details
                          clearable
                          :rules="entityRules"
                        ></v-autocomplete>
                      </v-col>
                      <v-col cols="12" sm="2" class="d-flex align-center">
                        <v-checkbox
                          v-model="entity.enabled"
                          label="Active"
                          hide-details
                          density="compact"
                        ></v-checkbox>
                      </v-col>
                      <v-col cols="12" sm="2" class="d-flex flex-column align-center">
                        <v-btn
                          icon
                          size="x-small"
                          @click="moveEntityUp(index)"
                          :disabled="index === 0"
                        >
                          <v-icon>mdi-arrow-up</v-icon>
                        </v-btn>
                        <v-btn
                          icon
                          size="x-small"
                          @click="moveEntityDown(index)"
                          :disabled="index === haEntitiesList.length - 1"
                        >
                          <v-icon>mdi-arrow-down</v-icon>
                        </v-btn>
                      </v-col>
                      <v-col cols="12" sm="1" class="d-flex align-center justify-center">
                        <v-btn
                          icon
                          size="x-small"
                          color="red"
                          @click="removeHaEntity(index)"
                        >
                          <v-icon>mdi-delete</v-icon>
                        </v-btn>
                      </v-col>
                    </v-row>
                  </div>

                  <v-btn
                    color="primary"
                    variant="flat"
                    size="small"
                    @click="addHaEntity"
                    class="mt-2"
                  >
                    <v-icon start>mdi-plus</v-icon>
                    Add Home Entity
                  </v-btn>

                  <v-divider class="mb-4 mt-4">
                    <v-chip size="small" color="primary">Header Toggles</v-chip>
                  </v-divider>

                  <div
                    v-for="(toggle, index) in headerTogglesList"
                    :key="toggle.id || `toggle-${index}`"
                    class="toggle-card mb-2 pa-2 rounded border"
                  >
                    <v-row dense>
                      <v-col cols="12" sm="4">
                        <v-text-field
                          v-model="toggle.label"
                          label="Label"
                          variant="outlined"
                          density="compact"
                          hide-details
                          required
                        ></v-text-field>
                      </v-col>
                      <v-col cols="12" sm="5">
                        <v-autocomplete
                          v-model="toggle.entity"
                          :items="discoveredEntities"
                          item-title="entity_id"
                          item-value="entity_id"
                          label="Entity ID"
                          variant="outlined"
                          density="compact"
                          hide-details
                          clearable
                          :rules="entityRules"
                        ></v-autocomplete>
                      </v-col>
                      <v-col cols="12" sm="1" class="d-flex align-center justify-center">
                        <v-btn
                          icon
                          size="x-small"
                          color="red"
                          @click="removeHeaderToggle(index)"
                        >
                          <v-icon>mdi-delete</v-icon>
                        </v-btn>
                      </v-col>
                      <v-col cols="12" sm="2" class="d-flex flex-column align-center">
                        <v-btn
                          icon
                          size="x-small"
                          @click="moveToggleUp(index)"
                          :disabled="index === 0"
                        >
                          <v-icon>mdi-arrow-up</v-icon>
                        </v-btn>
                        <v-btn
                          icon
                          size="x-small"
                          @click="moveToggleDown(index)"
                          :disabled="index === headerTogglesList.length - 1"
                        >
                          <v-icon>mdi-arrow-down</v-icon>
                        </v-btn>
                      </v-col>
                    </v-row>
                  </div>

                  <v-btn
                    color="primary"
                    variant="flat"
                    size="small"
                    @click="addHeaderToggle"
                    class="mt-2"
                  >
                    <v-icon start>mdi-plus</v-icon>
                    Add Header Toggle
                  </v-btn>

                  <v-divider class="mb-4 mt-4">
                    <v-chip size="small" color="info">Preview</v-chip>
                  </v-divider>

                  <div class="preview-section mb-4">
                    <div class="mb-2">
                      <strong>Header Toggles:</strong>
                    </div>
                    <div class="d-flex flex-wrap gap-1 mb-3">
                      <v-btn
                        v-for="toggle in headerTogglesList"
                        :key="toggle.id || toggle.entity"
                        outlined
                        class="custom-3d state-off"
                        size="small"
                      >
                        {{ toggle.label }}
                      </v-btn>
                      <span
                        v-if="headerTogglesList.length === 0"
                        class="text-caption grey--text"
                      >
                        None
                      </span>
                    </div>
                    <div class="mb-2">
                      <strong>Home Buttons:</strong>
                    </div>
                    <div class="d-flex flex-wrap gap-1">
                      <v-btn
                        v-for="entity in haEntitiesList"
                        :key="entity.id || entity.entity"
                        outlined
                        class="custom-3d state-off"
                        size="small"
                      >
                        {{ entity.label }}
                      </v-btn>
                      <span
                        v-if="haEntitiesList.length === 0"
                        class="text-caption grey--text"
                      >
                        None
                      </span>
                    </div>
                  </div>

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

                  <v-row class="mt-4">
                    <v-col cols="12" class="d-flex justify-end gap-2">
                      <v-btn @click="handleReset" :disabled="saving">
                        Reset to defaults
                      </v-btn>
                      <v-btn @click="handleSave" :loading="saving" color="primary">
                        Save
                      </v-btn>
                      <v-btn @click="closeWindow" text>Close</v-btn>
                    </v-col>
                  </v-row>
                </v-form>
              </v-card-text>
            </v-card>

            <v-dialog v-model="discoveryDialog" max-width="600px">
              <v-card title="Discover HA Entities">
                <v-card-text>
                  <v-progress-linear
                    v-if="discoveryLoading"
                    indeterminate
                  ></v-progress-linear>
                  <v-data-table
                    v-else
                    :items="discoveredEntities"
                    :headers="discoveryHeaders"
                    show-select
                    v-model:selected="selectedDiscovery"
                  ></v-data-table>
                  <v-radio-group
                    v-model="discoveryTargetGroup"
                    row
                    dense
                    class="mt-4"
                  >
                    <v-radio label="Add to Home Buttons" value="home"></v-radio>
                    <v-radio label="Add to Header Toggles" value="toggle"></v-radio>
                  </v-radio-group>
                </v-card-text>
                <v-card-actions>
                  <v-spacer></v-spacer>
                  <v-btn @click="discoveryDialog = false">Cancel</v-btn>
                  <v-btn
                    @click="addDiscoveredEntities"
                    color="primary"
                    :disabled="!selectedDiscovery || selectedDiscovery.length === 0"
                  >
                    Add Selected ({{ selectedDiscovery.length }})
                  </v-btn>
                </v-card-actions>
              </v-card>
            </v-dialog>

            <v-alert
              v-if="message"
              :type="messageType"
              density="compact"
              class="mt-4"
              closable
              @click:close="clearMessage"
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
import { ref, computed, watch, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useConfigForm } from './composables/useConfigForm'
import { useHAEntityManager } from './composables/useHAEntityManager'

const { config, saving, message, messageType, loadConfig, saveConfig, resetToDefaults, clearMessage } = useConfigForm()
const {
  haEntitiesList, headerTogglesList,
  discoveryDialog, discoveredEntities, selectedDiscovery, discoveryLoading,
  discoveryTargetGroup,
  loadFromConfig, fetchHaEntities, addDiscoveredEntities,
  addHaEntity, removeHaEntity, moveEntityUp, moveEntityDown,
  addHeaderToggle, removeHeaderToggle, moveToggleUp, moveToggleDown,
} = useHAEntityManager()

const showPassword = ref(false)
const showPasswordHa = ref(false)
const testingHa = ref(false)
const haTestResult = ref('')
const haTestSuccess = ref(false)

const haDirectMonitoringEnabled = computed(() => {
  return !!(config.ha_url && config.ha_longlived_token && config.ha_url.trim() && config.ha_longlived_token.trim())
})

const isHaConfigured = computed(() => haDirectMonitoringEnabled.value)

watch(
  [() => config.ha_longlived_token, () => config.ha_url],
  ([token, url]) => {
    config.ha_use_direct_api = !!(token && url && token.trim() && url.trim())
  },
  { immediate: true }
)

const urlRules = [
  (v: string) => !!v || 'URL required',
  (v: string) => v.startsWith('http://') || v.startsWith('https://') || 'Must start with http:// or https://'
]
const portRules = [(v: number) => (v >= 1 && v <= 65535) || 'Port must be 1-65535']
const entityRules = [
  (v: string) => !!v || 'Required',
  (v: string) => v.includes('.') || 'Must contain a dot (domain.entity)'
]
const discoveryHeaders = [
  { title: 'Friendly Name', key: 'friendly_name' },
  { title: 'Entity ID', key: 'entity_id' },
  { title: 'Domain', key: 'domain' }
]

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
      token: config.ha_longlived_token
    })
    haTestResult.value = 'Connection successful'
    haTestSuccess.value = true
  } catch (e: any) {
    haTestResult.value = `Failed: ${e?.toString() || e}`
    haTestSuccess.value = false
  } finally {
    testingHa.value = false
  }
}

async function handleFetchHaEntities() {
  if (!config.ha_url || !config.ha_longlived_token) {
    message.value = 'Please enter HA URL and Token first'
    messageType.value = 'error'
    return
  }
  try {
    await fetchHaEntities(config.ha_url, config.ha_port, config.ha_longlived_token)
  } catch (e: any) {
    message.value = `Discovery failed: ${e?.toString() || e}`
    messageType.value = 'error'
  }
}

async function handleSave() {
  await saveConfig(haEntitiesList.value, headerTogglesList.value)
}

async function closeWindow() {
  try {
    const window = await getCurrentWindow()
    await window.close()
  } catch (e) {
    console.error('JS close failed, trying Rust command:', e)
    try {
      await invoke('close_config_window')
    } catch (e2) {
      console.error('Rust close also failed:', e2)
    }
  }
}

function handleReset() {
  resetToDefaults()
  haEntitiesList.value = []
  headerTogglesList.value = []
}

onMounted(async () => {
  const cfg = await loadConfig()
  loadFromConfig(cfg)
})
</script>

<style scoped>
.fill-height {
  min-height: 100vh;
}
.entity-card.drag-over,
.toggle-card.drag-over {
  background-color: rgba(33, 150, 243, 0.1);
  border: 1px dashed #2196f3;
}
</style>

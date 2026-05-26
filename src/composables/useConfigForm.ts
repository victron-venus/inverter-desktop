import { ref, reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'

export interface AppConfig {
  mqtt_host: string
  mqtt_port: number
  mqtt_login?: string | null
  mqtt_password?: string | null
  mqtt_ha_host?: string
  mqtt_ha_port?: number
  mqtt_ha_login?: string | null
  mqtt_ha_password?: string | null
  ha_longlived_token?: string | null
  ha_url?: string | null
  ha_port?: number | null
  ha_use_direct_api?: boolean
  ha_entities?: Array<{ id: string; label: string; entity: string; domain: string; enabled: boolean }> | null
  header_toggles_config?: Array<{ id: string; label: string; entity: string }> | null
  ha_water_valve_entity?: string | null
  ha_pump_switch_entity?: string | null
  ha_boolean_entities?: Record<string, string> | null
  ha_switch_entities?: Record<string, { label?: string; entity: string }> | null
  header_toggles?: Array<{ id: string; label: string; entity: string }> | null
  color_scheme?: string | null
  portal_id?: string | null
  camera_topic?: string | null
}

const defaultConfig: AppConfig = {
  mqtt_host: 'Cerbo', mqtt_port: 1883, mqtt_login: null, mqtt_password: null,
  mqtt_ha_host: 'HA', mqtt_ha_port: 1883, mqtt_ha_login: null, mqtt_ha_password: null,
  ha_longlived_token: null, ha_url: null, ha_port: null, ha_use_direct_api: false,
  ha_entities: null, header_toggles_config: null,
  ha_water_valve_entity: null, ha_pump_switch_entity: null,
  ha_boolean_entities: null as Record<string, string> | null,
  ha_switch_entities: null as Record<string, { label?: string; entity: string }> | null,
  header_toggles: null as Array<{ id: string; label: string; entity: string }> | null,
  color_scheme: 'dark' as string | null,
  portal_id: null as string | null,
  camera_topic: 'frigate/+/events' as string | null
}

export function useConfigForm() {
  const config = reactive<AppConfig>({ ...defaultConfig })
  const saving = ref(false)
  const message = ref('')
  const messageType = ref<'success' | 'error' | 'info'>('info')

  async function loadConfig() {
    try {
      const loaded = await invoke<AppConfig>('get_config')
      Object.assign(config, loaded)
      if (!config.color_scheme) config.color_scheme = 'dark'
      message.value = ''
    } catch (e) {
      message.value = `Failed to load config: ${e}`
      messageType.value = 'error'
    }
    return config
  }

  async function saveConfig(
    haEntitiesList: Array<{ id: string; label: string; entity: string; domain: string; enabled: boolean }>,
    headerTogglesList: Array<{ id: string; label: string; entity: string }>
  ) {
    haEntitiesList.forEach(e => {
      if (!e.id && e.entity) e.id = e.entity.replace(/\./g, '_')
    })
    headerTogglesList.forEach(t => {
      if (!t.id && t.entity) t.id = t.entity.replace(/\./g, '_')
    })
    config.ha_entities = haEntitiesList
    config.header_toggles_config = headerTogglesList

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
    message.value = 'Reset to defaults (unsaved)'
    messageType.value = 'info'
  }

  function clearMessage() {
    message.value = ''
  }

  return { config, defaultConfig, saving, message, messageType, loadConfig, saveConfig, resetToDefaults, clearMessage }
}

import { invoke } from '@tauri-apps/api/core'
import { reactive, ref } from 'vue'
import type { AppConfig } from '../config'
import { defaultConfig } from '../config'

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
      // Ensure new boolean fields have defaults if missing from store
      if (config.show_ha_sensors === undefined) config.show_ha_sensors = true
      if (config.show_ha_numbers === undefined) config.show_ha_numbers = true
      if (config.show_ha_covers === undefined) config.show_ha_covers = true
      if (config.show_ha_media === undefined) config.show_ha_media = true
      if (config.show_ha_scenes === undefined) config.show_ha_scenes = true
      if (config.show_ha_weather === undefined) config.show_ha_weather = true
      if (config.show_console === undefined) config.show_console = true
      message.value = ''
    } catch (e) {
      message.value = `Failed to load config: ${e}`
      messageType.value = 'error'
    }
    return config
  }

  async function saveConfig(
    haEntitiesList: Array<{
      id: string
      label: string
      entity: string
      domain: string
      enabled: boolean
    }>,
    headerTogglesList: Array<{ id: string; label: string; entity: string }>
  ) {
    haEntitiesList.forEach((e) => {
      if (!e.id && e.entity) e.id = e.entity.replace(/\./g, '_')
    })
    headerTogglesList.forEach((t) => {
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

  return {
    config,
    defaultConfig,
    saving,
    message,
    messageType,
    loadConfig,
    saveConfig,
    resetToDefaults,
    clearMessage,
  }
}

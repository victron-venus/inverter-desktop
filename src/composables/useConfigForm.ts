import { invoke } from '@tauri-apps/api/core'
import { reactive, ref } from 'vue'
import type { AppConfig } from '../config'
import { defaultConfig } from '../config'
import type { DashboardControl } from '../inverterControl'
import { isDashboardControlTarget } from '../dashboardControlTarget'

export function useConfigForm() {
  const config = reactive<AppConfig>({ ...defaultConfig })
  const saving = ref(false)
  const configLoaded = ref(false)
  const message = ref('')
  const messageType = ref<'success' | 'error' | 'info'>('info')

  async function loadConfig() {
    configLoaded.value = false
    try {
      const loaded = await invoke<AppConfig>('get_config')
      Object.assign(config, loaded)
      if (!config.color_scheme) config.color_scheme = 'dark'
      // Ensure new boolean fields have defaults if missing from store
      const boolDefaults: Record<string, boolean> = {
        mqtt_tls: false,
        show_console: true,
        show_advanced_settings: false,
        gateway_enabled: false,
        setup_completed: false,
      }
      for (const [key, val] of Object.entries(boolDefaults)) {
        if (config[key as keyof AppConfig] === undefined) {
          ;(config as Record<string, unknown>)[key] = val
        }
      }
      message.value = ''
      configLoaded.value = true
    } catch (e) {
      message.value = `Failed to load config: ${e}`
      messageType.value = 'error'
      throw e
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
    headerTogglesList: DashboardControl[],
    editableHeaderControls = headerTogglesList
  ): Promise<boolean> {
    if (!configLoaded.value) return false
    const invalidControl = editableHeaderControls.find(
      (control) => !isDashboardControlTarget(control.entity)
    )
    if (invalidControl) {
      message.value = `Invalid header control target: ${invalidControl.entity || '(empty)'}. Choose a supported control target.`
      messageType.value = 'error'
      return false
    }
    function assignMissingIds(entries: Array<{ id: string; entity: string }>) {
      const used = new Set(entries.map((entry) => entry.id).filter(Boolean))
      for (const entry of entries) {
        if (entry.id || !entry.entity) continue
        const base = entry.entity.replace(/\./g, '_')
        let id = base
        let suffix = 2
        while (used.has(id)) id = `${base}_${suffix++}`
        entry.id = id
        used.add(id)
      }
    }
    assignMissingIds(haEntitiesList)
    assignMissingIds(headerTogglesList)
    config.ha_entities = haEntitiesList
    config.header_toggles_config = headerTogglesList

    saving.value = true
    try {
      await invoke('save_config', { config })
      message.value = 'Configuration saved successfully'
      messageType.value = 'success'
      return true
    } catch (e) {
      message.value = `Failed to save config: ${e}`
      messageType.value = 'error'
      return false
    } finally {
      saving.value = false
    }
  }

  function resetToDefaults() {
    // Core defaults do not own installed or unknown modules' persistent data.
    Object.assign(config, defaultConfig, { modules: config.modules })
    message.value = 'Reset to defaults (unsaved)'
    messageType.value = 'info'
  }

  function clearMessage() {
    message.value = ''
  }

  return {
    config,
    defaultConfig,
    configLoaded,
    saving,
    message,
    messageType,
    loadConfig,
    saveConfig,
    resetToDefaults,
    clearMessage,
  }
}

import { invoke } from '@tauri-apps/api/core'
import { computed, ref } from 'vue'
import type {
  JsonEditorSchema,
  PluginSettingsChoices,
  PluginSettingsField,
  PluginSettingsSaveResult,
  PluginSettingsView,
  PluginSettingValue,
} from './types'

/** Only newly entered secrets exist in this editor; native reads return presence flags. */
export function createPluginSettings(
  pluginId: string,
  version: string,
  fallbackError: () => string,
  onBusy: (busy: boolean) => void = () => {},
  invalidValue: (field: string) => string = fallbackError
) {
  const settings = ref<PluginSettingsView | null>(null)
  const values = ref<Record<string, PluginSettingValue>>({})
  const secretChanges = ref<Record<string, string | null>>({})
  const busy = ref(false)
  const error = ref<string | null>(null)
  const saved = ref(false)
  const restartError = ref<string | null>(null)
  const choices = ref<PluginSettingsChoices>({ revision: null, sources: {} })
  const choicesLoading = ref(false)
  const choicesError = ref(false)
  const hasChoices = computed(
    () =>
      settings.value?.fields.some(
        (field) => field.options_source || (field.editor && schemaHasChoices(field.editor.schema))
      ) ?? false
  )
  let choicesGeneration = 0
  let active = true
  let generation = 0

  function schemaHasChoices(schema: JsonEditorSchema): boolean {
    return (
      !!schema['x-options-source'] ||
      Object.values(schema.properties ?? {}).some(schemaHasChoices) ||
      !!(schema.items && schemaHasChoices(schema.items)) ||
      (typeof schema.additionalProperties === 'object' &&
        schemaHasChoices(schema.additionalProperties))
    )
  }
  async function loadChoices() {
    if (!active || !hasChoices.value) return
    const request = ++choicesGeneration
    choicesLoading.value = true
    choicesError.value = false
    try {
      const result = await invoke<PluginSettingsChoices>('get_plugin_settings_choices', {
        pluginId,
      })
      if (active && request === choicesGeneration) choices.value = result
    } catch {
      if (active && request === choicesGeneration) {
        choices.value = { revision: null, sources: {} }
        choicesError.value = true
      }
    } finally {
      if (active && request === choicesGeneration) choicesLoading.value = false
    }
  }

  function setBusy(value: boolean) {
    busy.value = value
    onBusy(value)
  }

  function current(request: number) {
    return active && request === generation
  }

  function message(value: unknown, secrets: Record<string, string | null> = {}) {
    let text = ''
    if (value instanceof Error) text = value.message
    else if (typeof value === 'string') text = value
    const fragments = new Set<string>()
    function collect(value: unknown) {
      if (typeof value === 'string' && value) fragments.add(value)
      else if (Array.isArray(value)) value.forEach(collect)
      else if (value && typeof value === 'object') Object.values(value).forEach(collect)
    }
    for (const secret of Object.values(secrets)) {
      if (!secret) continue
      fragments.add(secret)
      try {
        collect(JSON.parse(secret))
      } catch {
        /* Ordinary secrets are not structured JSON. */
      }
    }
    for (const secret of [...fragments].sort((a, b) => b.length - a.length))
      text = text.split(secret).join('••••')
    return text.slice(0, 512) || fallbackError()
  }

  function apply(view: PluginSettingsView) {
    if (view.plugin_id !== pluginId || view.version !== version) {
      throw new Error(fallbackError())
    }
    // Select only declared ordinary values, even if an unexpected IPC value appears.
    const ordinary = Object.fromEntries(
      view.fields
        .filter((field) => !field.secret && Object.keys(view.values).includes(field.key))
        .map((field) => [field.key, view.values[field.key]])
    )
    settings.value = { ...view, values: ordinary }
    values.value = { ...ordinary }
    secretChanges.value = {}
  }

  async function load() {
    if (!active || busy.value) return
    const request = ++generation
    setBusy(true)
    choicesGeneration += 1
    choices.value = { revision: null, sources: {} }
    choicesLoading.value = false
    choicesError.value = false
    settings.value = null
    values.value = {}
    secretChanges.value = {}
    error.value = null
    saved.value = false
    restartError.value = null
    try {
      const view = await invoke<PluginSettingsView>('get_plugin_settings', { pluginId })
      if (current(request)) {
        apply(view)
        void loadChoices()
      }
    } catch (error_) {
      if (current(request)) error.value = message(error_)
    } finally {
      if (current(request)) setBusy(false)
    }
  }

  function setSecret(key: string, value: string) {
    if (!active || busy.value || !settings.value?.fields.some((f) => f.key === key && f.secret))
      return
    if (value) secretChanges.value[key] = value
    else delete secretChanges.value[key]
    saved.value = false
  }

  function removeSecret(key: string, remove: boolean) {
    if (!active || busy.value || !settings.value?.fields.some((f) => f.key === key && f.secret))
      return
    if (remove) secretChanges.value[key] = null
    else delete secretChanges.value[key]
    saved.value = false
  }

  function ordinaryValue(field: PluginSettingsField) {
    const value = values.value[field.key]
    // An unchecked required checkbox is a valid false value, not a missing setting.
    if (field.type === 'boolean' && field.required && value === undefined) return false
    if (field.type === 'number' || field.type === 'integer') {
      if (value === undefined || value === '') return undefined
      const number = Number(value)
      // Keep the input lexeme until submission: eager numeric coercion can round
      // fractional or oversized integers into a different, apparently valid value.
      const integer = field.type === 'integer'
      if (
        !Number.isFinite(number) ||
        (integer && (!Number.isSafeInteger(number) || !/^[+-]?\d+$/.test(String(value))))
      )
        throw new Error(invalidValue(field.title))
      return number
    }
    return value
  }

  async function save() {
    const view = settings.value
    if (!active || busy.value || !view) return false
    const request = ++generation
    const changes = { ...secretChanges.value }
    // Empty password inputs immediately, including while native save/restart is pending.
    secretChanges.value = {}
    setBusy(true)
    error.value = null
    saved.value = false
    restartError.value = null
    try {
      const ordinary = Object.fromEntries(
        view.fields
          .filter((field) => !field.secret)
          .map((field) => [field.key, ordinaryValue(field)])
          .filter(([, value]) => value !== undefined)
      )
      const result = await invoke<PluginSettingsSaveResult>('save_plugin_settings', {
        pluginId,
        revision: view.revision,
        values: ordinary,
        secretChanges: changes,
      })
      if (!current(request)) return false
      apply(result.settings)
      saved.value = true
      restartError.value = result.restart_error ? message(result.restart_error, changes) : null
      return true
    } catch (error_) {
      if (current(request)) error.value = message(error_, changes)
      return false
    } finally {
      if (current(request)) setBusy(false)
    }
  }

  function stop() {
    active = false
    generation += 1
    choicesGeneration += 1
    choices.value = { revision: null, sources: {} }
    choicesLoading.value = false
    choicesError.value = false
    settings.value = null
    values.value = {}
    secretChanges.value = {}
    error.value = null
    saved.value = false
    restartError.value = null
    setBusy(false)
  }

  return {
    settings,
    choices,
    hasChoices,
    choicesLoading,
    choicesError,
    loadChoices,
    values,
    secretChanges,
    busy,
    error,
    saved,
    restartError,
    load,
    save,
    setSecret,
    removeSecret,
    stop,
  }
}

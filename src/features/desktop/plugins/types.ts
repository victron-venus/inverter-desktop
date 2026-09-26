/** Declarative values validated by the native host; workers never supply view code. */
export type DashboardContribution =
  | { kind: 'text'; id: string; title: string; text: string }
  | { kind: 'metric'; id: string; title: string; value: number; unit?: string | null }
  | {
      kind: 'status'
      id: string
      title: string
      value: string
      tone: 'neutral' | 'success' | 'warning' | 'error'
    }
  | {
      kind: 'action'
      id: string
      title: string
      action_id: string
      label: string
      params: Record<string, unknown>
      state_id?: string
    }
  | {
      kind: 'number_input'
      id: string
      title: string
      action_id: string
      label: string
      state_id?: string
      unit?: string | null
      input_revision: string
      value_scaled: number
      min_scaled: number
      max_scaled: number
      step_scaled: number
      decimal_places: number
    }

export type PresentationIcon =
  | 'home'
  | 'plug'
  | 'light'
  | 'washer'
  | 'dryer'
  | 'dishwasher'
  | 'thermometer'
  | 'gauge'
  | 'blinds'
  | 'play'
  | 'cloud'

export interface PresentationAction {
  id: string
  label: string
}
export type PluginPresentation =
  | {
      kind: 'control'
      id: string
      surface: 'header' | 'home'
      order: number
      title: string
      icon?: PresentationIcon | null
      state: 'on' | 'off' | 'unavailable'
      action?: string | null
    }
  | {
      kind: 'group'
      id: string
      surface: 'sidebar'
      order: number
      title: string
      icon?: PresentationIcon | null
      collapsed: boolean
      rows: Array<{
        id: string
        title: string
        value: string
        actions: PresentationAction[]
        input?: string | null
      }>
    }
  | {
      kind: 'summary'
      id: string
      surface: 'sidebar'
      order: number
      title: string
      icon?: PresentationIcon | null
      visible: boolean
      active: boolean
      text: string
      actions: PresentationAction[]
    }
  | {
      kind: 'weather'
      id: string
      surface: 'sidebar'
      order: number
      title: string
      condition: string
      temperature: string
      unit: string
      forecast: Array<{
        datetime: string
        condition: string
        temperature: string
        templow?: string | null
      }>
    }
  | { kind: 'connection'; id: string; title: string; connected: boolean }

export interface PluginSnapshot {
  plugin_id: string
  instance_id: string | null
  state: 'starting' | 'running' | 'restarting' | 'stopped' | 'failed'
  generation: number
  restart_count: number
  contributions: DashboardContribution[]
  presentation?: PluginPresentation[]
  last_error: string | null
}

export type ActionContribution = Extract<DashboardContribution, { kind: 'action' }>
export type NumberInputContribution = Extract<DashboardContribution, { kind: 'number_input' }>

export interface ManagedPlugin {
  configuration_managed?: boolean
  plugin_id: string
  version: string
  rollback_version: string | null
  enabled: boolean
  permissions: string[]
  runtime: PluginSnapshot | null
  error: string | null
}

export interface PluginGroup {
  id: string
  title: string
  icon: string
  enabled: boolean
  plugin_ids: string[]
}

export interface PluginManagerSnapshot {
  groups?: PluginGroup[]
  configured?: Array<{
    plugin_id: string
    version: string
    declaration_revision: string
    enabled: boolean
    local?: boolean
    state: 'pending' | 'downloading' | 'installing' | 'ready' | 'disabled' | 'failed'
    error: string | null
  }>
  configuration_error?: string | null
  ready: boolean
  error: string | null
  installation_available: boolean
  target: string
  data_revision: string
  plugins: ManagedPlugin[]
}

export interface PluginPackagePreview {
  token: string
  plugin_id: string
  version: string
  current_version: string | null
  permissions: string[]
  target: string
  host_api: string
  publisher_key_id: string
  restart_required: false
}

export type PluginSettingValue = string | number | boolean

export interface JsonEditorSchema {
  type: 'object' | 'array' | 'string' | 'boolean' | 'number' | 'integer'
  title?: string
  description?: string
  properties?: Record<string, JsonEditorSchema>
  required?: string[]
  additionalProperties?: boolean | JsonEditorSchema
  items?: JsonEditorSchema
  minItems?: number
  maxItems?: number
  maxProperties?: number
  minLength?: number
  maxLength?: number
  minimum?: number
  maximum?: number
  enum?: Array<string | number | boolean>
  default?: unknown
  const?: unknown
  'x-options-source'?: string
  'x-options-prefixes'?: string[]
  'x-options-multiple'?: boolean
}
export interface PluginSettingsChoices {
  revision: string | null
  sources: Record<string, Array<{ value: string; label: string }>>
}

export interface PluginSettingsField {
  editor?: { kind: 'json'; schema: JsonEditorSchema }
  options_source?: string
  options_prefixes?: string[]
  options_multiple?: boolean
  key: string
  title: string
  description: string | null
  type: 'string' | 'boolean' | 'number' | 'integer'
  required: boolean
  secret: boolean
  enum: string[] | null
  minimum: number | null
  maximum: number | null
  min_length: number | null
  max_length: number | null
}

export interface PluginSettingsView {
  plugin_id: string
  version: string
  revision: string
  fields: PluginSettingsField[]
  values: Record<string, PluginSettingValue>
  secret_present: Record<string, boolean>
}

export interface PluginSettingsSaveResult {
  settings: PluginSettingsView
  restart_error: string | null
}

export interface RetainedPluginDataRecord {
  record_id: string
  revision: string
  bytes: number
  plugin_id: string | null
}

export interface RetainedPluginDataSnapshot {
  records: RetainedPluginDataRecord[]
  total_bytes: number
  max_records: number
  max_bytes: number
}

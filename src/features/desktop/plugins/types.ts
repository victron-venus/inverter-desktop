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
    }

export interface PluginSnapshot {
  plugin_id: string
  state: 'starting' | 'running' | 'restarting' | 'stopped' | 'failed'
  generation: number
  restart_count: number
  contributions: DashboardContribution[]
  last_error: string | null
}

export type ActionContribution = Extract<DashboardContribution, { kind: 'action' }>

export interface ManagedPlugin {
  plugin_id: string
  version: string
  rollback_version: string | null
  enabled: boolean
  permissions: string[]
  runtime: PluginSnapshot | null
  error: string | null
}

export interface PluginManagerSnapshot {
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

export interface PluginSettingsField {
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

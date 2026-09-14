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
